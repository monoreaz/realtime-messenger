//! HTTP client for the messenger backend.
//!
//! Authentication mirrors the web client: a short-lived bearer access token plus a
//! rotating refresh token that the server sets as the `messenger_refresh` cookie.
//! The desktop app manages that cookie itself (instead of a browser cookie jar) so it
//! can persist it encrypted when "Remember me" is enabled.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{COOKIE, HeaderMap, SET_COOKIE};
use reqwest::{Method, RequestBuilder, Response, StatusCode, multipart};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::models::*;
use crate::storage::{SessionStore, StoredSession};

const REFRESH_COOKIE: &str = "messenger_refresh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { status: None, message: message.into() }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status == Some(401)
    }

    /// Network failures (no HTTP status) are worth retrying; HTTP errors are not.
    pub fn is_network(&self) -> bool {
        self.status.is_none()
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(error: reqwest::Error) -> Self {
        let message = if error.is_timeout() {
            "The server did not respond in time. Check your connection.".to_owned()
        } else if error.is_connect() {
            "Could not connect to the server. Check your connection and Tailscale.".to_owned()
        } else if error.is_decode() {
            format!("Unexpected response from the server: {error}")
        } else {
            format!("Network error: {error}")
        };
        Self { status: error.status().map(|status| status.as_u16()), message }
    }
}

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Default)]
struct AuthState {
    access_token: Option<String>,
    refresh_cookie: Option<String>,
    remember: bool,
    username: Option<String>,
}

struct Inner {
    http: reqwest::Client,
    server: String,
    auth: Mutex<AuthState>,
    refresh_lock: tokio::sync::Mutex<()>,
    store: Option<SessionStore>,
}

/// Cheap to clone; all clones share the same session.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("server", &self.inner.server).finish_non_exhaustive()
    }
}

/// Normalises what the user typed as server address into `https://host[:port]`.
pub fn normalize_server(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('/');
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    with_scheme.trim_end_matches("/api").to_owned()
}

pub fn user_agent() -> String {
    format!("MessengerDesktop/{} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS.replace("windows", "Windows"))
}

impl Client {
    pub fn new(server: &str, store: Option<SessionStore>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(user_agent())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP client configuration is valid");
        Self {
            inner: Arc::new(Inner {
                http,
                server: normalize_server(server),
                auth: Mutex::new(AuthState::default()),
                refresh_lock: tokio::sync::Mutex::new(()),
                store,
            }),
        }
    }

    pub fn server(&self) -> &str {
        &self.inner.server
    }

    fn api(&self, path: &str) -> String {
        format!("{}/api{}", self.inner.server, path)
    }

    /// Absolute URL for a media path returned by the API (`getMediaUrl`).
    pub fn media_url(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            path.to_owned()
        } else {
            self.api(path)
        }
    }

    pub fn ws_url(&self) -> String {
        let server = &self.inner.server;
        if let Some(rest) = server.strip_prefix("https://") {
            format!("wss://{rest}/ws")
        } else if let Some(rest) = server.strip_prefix("http://") {
            format!("ws://{rest}/ws")
        } else {
            format!("wss://{server}/ws")
        }
    }

    pub fn access_token(&self) -> Option<String> {
        self.inner.auth.lock().unwrap().access_token.clone()
    }

    pub fn has_refresh_session(&self) -> bool {
        self.inner.auth.lock().unwrap().refresh_cookie.is_some()
    }

    /// Loads a persisted refresh cookie (used at startup before calling [`refresh`]).
    pub fn restore(&self, session: StoredSession) {
        let mut auth = self.inner.auth.lock().unwrap();
        auth.refresh_cookie = Some(session.refresh_cookie);
        auth.remember = true;
        auth.username = session.username;
    }

    fn clear_auth(&self) {
        {
            let mut auth = self.inner.auth.lock().unwrap();
            *auth = AuthState::default();
        }
        if let Some(store) = &self.inner.store {
            store.clear();
        }
    }

    /// Captures a rotated refresh cookie from a response and persists it if remembered.
    fn absorb_cookie(&self, headers: &HeaderMap) {
        let Some(value) = refresh_cookie_from(headers) else { return };
        let mut auth = self.inner.auth.lock().unwrap();
        match value {
            Some(cookie) => {
                auth.refresh_cookie = Some(cookie.clone());
                if auth.remember
                    && let Some(store) = &self.inner.store
                {
                    store.save(&StoredSession {
                        server: self.inner.server.clone(),
                        refresh_cookie: cookie,
                        username: auth.username.clone(),
                    });
                }
            }
            None => {
                auth.refresh_cookie = None;
                if let Some(store) = &self.inner.store {
                    store.clear();
                }
            }
        }
    }

    fn with_cookie(&self, request: RequestBuilder) -> RequestBuilder {
        match self.inner.auth.lock().unwrap().refresh_cookie.clone() {
            Some(cookie) => request.header(COOKIE, format!("{REFRESH_COOKIE}={cookie}")),
            None => request,
        }
    }

    // ----------------------------------------------------------------------------
    // Authentication
    // ----------------------------------------------------------------------------

    pub async fn login(&self, username: &str, password: &str, remember: bool) -> Result<()> {
        let response = self
            .inner
            .http
            .post(self.api("/auth/login"))
            .form(&[("username", username), ("password", password), ("remember_me", if remember { "true" } else { "false" })])
            .send()
            .await?;
        let response = check(response, "Login failed").await?;
        {
            let mut auth = self.inner.auth.lock().unwrap();
            auth.remember = remember;
            auth.username = Some(username.to_owned());
        }
        self.absorb_cookie(response.headers());
        let token: LoginResponse = response.json().await?;
        self.inner.auth.lock().unwrap().access_token = Some(token.access_token);
        Ok(())
    }

    pub async fn register(&self, username: &str, email: &str, password: &str) -> Result<User> {
        #[derive(Serialize)]
        struct Body<'a> {
            username: &'a str,
            email: &'a str,
            password: &'a str,
        }
        let response = self.inner.http.post(self.api("/auth/register")).json(&Body { username, email, password }).send().await?;
        Ok(check(response, "Registration failed").await?.json().await?)
    }

    pub async fn resend_verification(&self, email: &str) -> Result<()> {
        let response = self.inner.http.post(self.api("/auth/resend-verification")).json(&serde_json::json!({ "email": email })).send().await?;
        check(response, "Could not resend verification email").await.map(drop)
    }

    #[allow(dead_code)] // email links open in the browser; kept for API completeness
    pub async fn verify_email(&self, token: &str) -> Result<()> {
        let response = self.inner.http.post(self.api("/auth/verify-email")).json(&serde_json::json!({ "token": token })).send().await?;
        check(response, "Email verification failed").await.map(drop)
    }

    pub async fn forgot_password(&self, email: &str) -> Result<()> {
        let response = self.inner.http.post(self.api("/auth/forgot-password")).json(&serde_json::json!({ "email": email })).send().await?;
        check(response, "Could not send password reset email").await.map(drop)
    }

    #[allow(dead_code)] // reset links open in the browser; kept for API completeness
    pub async fn reset_password(&self, token: &str, new_password: &str) -> Result<()> {
        let response = self
            .inner
            .http
            .post(self.api("/auth/reset-password"))
            .json(&serde_json::json!({ "token": token, "new_password": new_password }))
            .send()
            .await?;
        check(response, "Could not reset password").await.map(drop)
    }

    /// Exchanges the refresh cookie for a new access token. Concurrent callers share one
    /// refresh: the server rotates the cookie, so a second parallel refresh would fail.
    pub async fn refresh(&self) -> Result<String> {
        let stale = self.access_token();
        self.refresh_after(stale).await
    }

    /// Refreshes unless another caller already replaced `token`.
    pub async fn refresh_if_current(&self, token: &str) -> Result<String> {
        self.refresh_after(Some(token.to_owned())).await
    }

    async fn refresh_after(&self, stale: Option<String>) -> Result<String> {
        let _guard = self.inner.refresh_lock.lock().await;
        if let Some(current) = self.access_token()
            && Some(&current) != stale.as_ref()
        {
            return Ok(current);
        }
        if !self.has_refresh_session() {
            return Err(ApiError { status: Some(401), message: "No active session".into() });
        }
        let request = self.with_cookie(self.inner.http.post(self.api("/auth/refresh")));
        let response = request.send().await?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            let error = error_from(response, "Your session has ended. Please sign in again.").await;
            self.clear_auth();
            return Err(ApiError { status: Some(401), ..error });
        }
        let response = check(response, "Could not refresh the session").await?;
        self.absorb_cookie(response.headers());
        let token: LoginResponse = response.json().await?;
        self.inner.auth.lock().unwrap().access_token = Some(token.access_token.clone());
        Ok(token.access_token)
    }

    pub async fn logout(&self) -> Result<()> {
        let request = self.with_cookie(self.inner.http.post(self.api("/auth/logout")));
        let result = match request.send().await {
            Ok(response) => check(response, "Could not log out").await.map(drop),
            Err(error) => Err(error.into()),
        };
        // Forget local credentials even if the server could not be reached.
        self.clear_auth();
        result
    }

    /// Sends an authorized request, refreshing the access token once on 401.
    async fn authorized(&self, build: impl Fn(&reqwest::Client) -> RequestBuilder) -> Result<Response> {
        let token = match self.access_token() {
            Some(token) => token,
            None => self.refresh_after(None).await?,
        };
        let response = self.with_cookie(build(&self.inner.http).bearer_auth(&token)).send().await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        let token = self.refresh_after(Some(token)).await?;
        Ok(self.with_cookie(build(&self.inner.http).bearer_auth(&token)).send().await?)
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str, fallback: &str) -> Result<T> {
        let url = self.api(path);
        let response = self.authorized(|http| http.get(&url)).await?;
        Ok(check(response, fallback).await?.json().await?)
    }

    async fn send_json<B: Serialize, T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<&B>, fallback: &str) -> Result<T> {
        let url = self.api(path);
        let response = self
            .authorized(|http| {
                let request = http.request(method.clone(), &url);
                match body {
                    Some(body) => request.json(body),
                    None => request,
                }
            })
            .await?;
        Ok(check(response, fallback).await?.json().await?)
    }

    async fn send_empty(&self, method: Method, path: &str, fallback: &str) -> Result<()> {
        let url = self.api(path);
        let response = self.authorized(|http| http.request(method.clone(), &url)).await?;
        check(response, fallback).await.map(drop)
    }

    // ----------------------------------------------------------------------------
    // Account
    // ----------------------------------------------------------------------------

    pub async fn me(&self) -> Result<User> {
        self.get_json("/users/me", "Could not load current user").await
    }

    pub async fn update_profile(&self, profile: &ProfileUpdate) -> Result<User> {
        self.send_json(Method::PATCH, "/users/me", Some(profile), "Could not update profile").await
    }

    pub async fn upload_avatar(&self, file: &Path) -> Result<User> {
        let upload = Upload::read(file, &IMAGE_TYPES).await?;
        let url = self.api("/users/me/avatar");
        let response = self.authorized(|http| http.post(&url).multipart(multipart::Form::new().part("file", upload.part()))).await?;
        Ok(check(response, "Could not upload avatar").await?.json().await?)
    }

    pub async fn remove_avatar(&self) -> Result<User> {
        self.send_json::<(), _>(Method::DELETE, "/users/me/avatar", None, "Could not remove avatar").await
    }

    pub async fn change_password(&self, current: &str, new: &str) -> Result<()> {
        let url = self.api("/auth/change-password");
        let body = serde_json::json!({ "current_password": current, "new_password": new });
        let response = self.authorized(|http| http.post(&url).json(&body)).await?;
        check(response, "Could not change password").await.map(drop)
    }

    pub async fn sessions(&self) -> Result<Vec<SessionInfo>> {
        self.get_json("/auth/sessions", "Could not load sessions").await
    }

    pub async fn revoke_session(&self, session_id: &str) -> Result<()> {
        self.send_empty(Method::DELETE, &format!("/auth/sessions/{session_id}"), "Could not log out session").await
    }

    pub async fn logout_other_sessions(&self) -> Result<()> {
        self.send_empty(Method::POST, "/auth/sessions/logout-others", "Could not log out other sessions").await
    }

    // ----------------------------------------------------------------------------
    // Users & chats
    // ----------------------------------------------------------------------------

    pub async fn search_users(&self, query: &str) -> Result<Vec<User>> {
        let url = self.api("/users/search");
        let query = query.trim().to_owned();
        let response = self.authorized(|http| http.get(&url).query(&[("username", &query)])).await?;
        Ok(check(response, "Could not search users").await?.json().await?)
    }

    pub async fn chats(&self) -> Result<Vec<Chat>> {
        self.get_json("/chats", "Could not load chats").await
    }

    pub async fn create_private_chat(&self, user_id: &str) -> Result<Chat> {
        self.send_json::<(), _>(Method::POST, &format!("/chats/private/{user_id}"), None, "Could not create chat").await
    }

    pub async fn create_group(&self, name: &str, member_ids: &[String], username: Option<&str>, description: Option<&str>) -> Result<Chat> {
        let body = serde_json::json!({
            "name": name, "member_ids": member_ids,
            "username": username.filter(|value| !value.is_empty()),
            "description": description.filter(|value| !value.is_empty()),
        });
        self.send_json(Method::POST, "/chats/group", Some(&body), "Could not create group").await
    }

    pub async fn update_group(&self, chat_id: &str, details: &GroupDetails) -> Result<Chat> {
        self.send_json(Method::PATCH, &format!("/chats/{chat_id}/group"), Some(details), "Could not update group").await
    }

    pub async fn mark_read(&self, chat_id: &str) -> Result<()> {
        self.send_empty(Method::POST, &format!("/chats/{chat_id}/read"), "Could not mark chat as read").await
    }

    // ----------------------------------------------------------------------------
    // Messages
    // ----------------------------------------------------------------------------

    /// One page (≤50, oldest first) of messages, optionally older than `before`.
    pub async fn messages(&self, chat_id: &str, before: Option<&str>) -> Result<Vec<Message>> {
        let url = self.api(&format!("/chats/{chat_id}/messages"));
        let before = before.map(str::to_owned);
        let response = self
            .authorized(|http| {
                let request = http.get(&url);
                match &before {
                    Some(id) => request.query(&[("before_message_id", id)]),
                    None => request,
                }
            })
            .await?;
        Ok(check(response, "Could not load messages").await?.json().await?)
    }

    pub async fn pinned_messages(&self, chat_id: &str) -> Result<Vec<Message>> {
        self.get_json(&format!("/chats/{chat_id}/pinned-messages"), "Could not load pinned messages").await
    }

    pub async fn set_pinned(&self, chat_id: &str, message_id: &str, pinned: bool) -> Result<Message> {
        let method = if pinned { Method::POST } else { Method::DELETE };
        let fallback = if pinned { "Could not pin message" } else { "Could not unpin message" };
        self.send_json::<(), _>(method, &format!("/chats/{chat_id}/messages/{message_id}/pin"), None, fallback).await
    }

    pub async fn send_message(&self, chat_id: &str, content: &str, reply_to: Option<&str>) -> Result<Message> {
        let body = serde_json::json!({ "content": content, "reply_to_message_id": reply_to });
        self.send_json(Method::POST, &format!("/chats/{chat_id}/messages"), Some(&body), "Could not send message").await
    }

    pub async fn edit_message(&self, chat_id: &str, message_id: &str, content: &str) -> Result<Message> {
        let body = serde_json::json!({ "content": content });
        self.send_json(Method::PATCH, &format!("/chats/{chat_id}/messages/{message_id}"), Some(&body), "Could not edit message").await
    }

    pub async fn delete_message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        self.send_json::<(), _>(Method::DELETE, &format!("/chats/{chat_id}/messages/{message_id}"), None, "Could not delete message").await
    }

    /// Sends a photo or video (chosen by file extension) with an optional caption.
    pub async fn send_media(&self, chat_id: &str, file: &Path, caption: &str, reply_to: Option<&str>) -> Result<Message> {
        let kind = MediaKind::of(file).ok_or_else(|| ApiError::new("Only JPG, PNG, WEBP images and MP4, WEBM videos can be sent."))?;
        let upload = match kind {
            MediaKind::Image => Upload::read(file, &IMAGE_TYPES).await?,
            MediaKind::Video => Upload::read(file, &VIDEO_TYPES).await?,
        };
        let limit = match kind {
            MediaKind::Image => 10 * 1024 * 1024,
            MediaKind::Video => 50 * 1024 * 1024,
        };
        if upload.bytes.len() > limit {
            return Err(ApiError::new(match kind {
                MediaKind::Image => "Image must be 10 MB or smaller.",
                MediaKind::Video => "Video must be 50 MB or smaller.",
            }));
        }
        let (path, fallback) = match kind {
            MediaKind::Image => ("image", "Could not send image"),
            MediaKind::Video => ("video", "Could not send video"),
        };
        let url = self.api(&format!("/chats/{chat_id}/messages/{path}"));
        let caption = caption.to_owned();
        let reply_to = reply_to.map(str::to_owned);
        let response = self
            .authorized(|http| {
                let mut form = multipart::Form::new().part("file", upload.part()).text("caption", caption.clone());
                if let Some(reply) = &reply_to {
                    form = form.text("reply_to_message_id", reply.clone());
                }
                http.post(&url).multipart(form)
            })
            .await?;
        Ok(check(response, fallback).await?.json().await?)
    }

    // ----------------------------------------------------------------------------
    // Admin & calls
    // ----------------------------------------------------------------------------

    pub async fn admin_stats(&self) -> Result<AdminStats> {
        self.get_json("/admin/stats", "Admin request failed").await
    }

    pub async fn admin_users(&self, query: &str, state: &str, offset: u64, limit: u64) -> Result<AdminUsersPage> {
        let url = self.api("/admin/users");
        let params = [("q", query.to_owned()), ("state", state.to_owned()), ("offset", offset.to_string()), ("limit", limit.to_string())];
        let response = self.authorized(|http| http.get(&url).query(&params)).await?;
        Ok(check(response, "Admin request failed").await?.json().await?)
    }

    pub async fn admin_set_blocked(&self, user_id: &str, blocked: bool) -> Result<AdminUser> {
        let body = serde_json::json!({ "is_blocked": blocked });
        self.send_json(Method::PATCH, &format!("/admin/users/{user_id}/block"), Some(&body), "Admin request failed").await
    }

    #[allow(dead_code)] // for native audio calls (planned); exercised by live tests
    pub async fn ice_servers(&self) -> Result<serde_json::Value> {
        let value: serde_json::Value = self.get_json("/calls/ice", "Could not prepare the call connection. Please try again.").await?;
        Ok(value.get("iceServers").cloned().unwrap_or_default())
    }

    // ----------------------------------------------------------------------------
    // Media
    // ----------------------------------------------------------------------------

    /// Downloads a media file (uploads are public static files on the server).
    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.inner.http.get(url).send().await?;
        let response = check(response, "Could not download file").await?;
        Ok(response.bytes().await?.to_vec())
    }

    /// Streams a download to `target`, reporting progress as (received, total).
    pub async fn download_to(&self, url: &str, target: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<()> {
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;

        let response = self.inner.http.get(url).timeout(Duration::from_secs(60 * 30)).send().await?;
        let response = check(response, "Could not download file").await?;
        let total = response.content_length();
        let partial = target.with_extension("part");
        let mut file = tokio::fs::File::create(&partial).await.map_err(io_error)?;
        let mut received = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await.map_err(io_error)?;
            received += chunk.len() as u64;
            progress(received, total);
        }
        file.flush().await.map_err(io_error)?;
        drop(file);
        tokio::fs::rename(&partial, target).await.map_err(io_error)?;
        Ok(())
    }
}

pub fn io_error(error: std::io::Error) -> ApiError {
    ApiError::new(format!("File error: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

const IMAGE_TYPES: [(&str, &str); 4] = [("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("png", "image/png"), ("webp", "image/webp")];
const VIDEO_TYPES: [(&str, &str); 2] = [("mp4", "video/mp4"), ("webm", "video/webm")];

impl MediaKind {
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        if IMAGE_TYPES.iter().any(|(ext, _)| *ext == extension) {
            Some(Self::Image)
        } else if VIDEO_TYPES.iter().any(|(ext, _)| *ext == extension) {
            Some(Self::Video)
        } else {
            None
        }
    }
}

struct Upload {
    name: String,
    mime: &'static str,
    /// Shared so the multipart form can be rebuilt if the token must be refreshed.
    bytes: Arc<Vec<u8>>,
}

impl Upload {
    async fn read(path: &Path, types: &[(&str, &'static str)]) -> Result<Self> {
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default().to_ascii_lowercase();
        let mime = types
            .iter()
            .find(|(ext, _)| *ext == extension)
            .map(|(_, mime)| *mime)
            .ok_or_else(|| ApiError::new("This file type is not supported."))?;
        let bytes = tokio::fs::read(path).await.map_err(io_error)?;
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("upload").to_owned();
        Ok(Self { name, mime, bytes: Arc::new(bytes) })
    }

    fn part(&self) -> multipart::Part {
        multipart::Part::bytes((*self.bytes).clone())
            .file_name(self.name.clone())
            .mime_str(self.mime)
            .expect("static MIME types are valid")
    }
}

/// `Some(Some(value))` when the response sets the refresh cookie, `Some(None)` when it
/// deletes it, `None` when the response does not touch it.
fn refresh_cookie_from(headers: &HeaderMap) -> Option<Option<String>> {
    for header in headers.get_all(SET_COOKIE) {
        let Ok(raw) = header.to_str() else { continue };
        let mut parts = raw.split(';');
        let Some((name, value)) = parts.next().and_then(|pair| pair.split_once('=')) else { continue };
        if name.trim() != REFRESH_COOKIE {
            continue;
        }
        let value = value.trim().trim_matches('"');
        let expired = parts.any(|attribute| {
            let attribute = attribute.trim().to_ascii_lowercase();
            attribute == "max-age=0" || attribute.starts_with("expires=thu, 01 jan 1970")
        });
        return Some(if value.is_empty() || expired { None } else { Some(value.to_owned()) });
    }
    None
}

async fn check(response: Response, fallback: &str) -> Result<Response> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(error_from(response, fallback).await)
    }
}

/// Extracts FastAPI's `detail` (a string, or a list of validation errors).
async fn error_from(response: Response, fallback: &str) -> ApiError {
    let status = response.status().as_u16();
    let message = match response.json::<serde_json::Value>().await {
        Ok(body) => detail_message(&body).unwrap_or_else(|| fallback.to_owned()),
        Err(_) => fallback.to_owned(),
    };
    ApiError { status: Some(status), message }
}

fn detail_message(body: &serde_json::Value) -> Option<String> {
    match body.get("detail")? {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(items) => items
            .first()
            .and_then(|item| item.get("msg"))
            .and_then(|msg| msg.as_str())
            .map(|msg| msg.trim_start_matches("Value error, ").to_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn normalizes_server_addresses() {
        assert_eq!(normalize_server("messenger-01.tail1a26d2.ts.net/"), "https://messenger-01.tail1a26d2.ts.net");
        assert_eq!(normalize_server(" http://10.0.0.5:8080/api "), "http://10.0.0.5:8080");
        let client = Client::new("https://example.net", None);
        assert_eq!(client.ws_url(), "wss://example.net/ws");
        assert_eq!(client.media_url("/uploads/a.png"), "https://example.net/api/uploads/a.png");
        assert_eq!(client.media_url("https://cdn/x.png"), "https://cdn/x.png");
    }

    #[test]
    fn reads_refresh_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("other=1; Path=/"));
        assert_eq!(refresh_cookie_from(&headers), None);
        headers.append(SET_COOKIE, HeaderValue::from_static("messenger_refresh=abc-123; HttpOnly; Max-Age=2592000; Path=/; SameSite=lax; Secure"));
        assert_eq!(refresh_cookie_from(&headers), Some(Some("abc-123".into())));

        let mut deleted = HeaderMap::new();
        deleted.append(SET_COOKIE, HeaderValue::from_static("messenger_refresh=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/"));
        assert_eq!(refresh_cookie_from(&deleted), Some(None));
    }

    #[test]
    fn extracts_fastapi_errors() {
        assert_eq!(detail_message(&serde_json::json!({"detail": "Nope"})).as_deref(), Some("Nope"));
        let validation = serde_json::json!({"detail": [{"msg": "Value error, Message cannot be empty", "loc": ["body"]}]});
        assert_eq!(detail_message(&validation).as_deref(), Some("Message cannot be empty"));
        assert_eq!(detail_message(&serde_json::json!({})), None);
    }

    #[test]
    fn classifies_media() {
        assert_eq!(MediaKind::of(Path::new("a.JPG")), Some(MediaKind::Image));
        assert_eq!(MediaKind::of(Path::new("clip.webm")), Some(MediaKind::Video));
        assert_eq!(MediaKind::of(Path::new("doc.pdf")), None);
    }
}
