//! Realtime connection to `/ws` as an iced subscription.
//!
//! Protocol (see `backend/app/api/ws.py`): the client sends `{"type":"auth","token":…}`
//! first; the server answers `connection.ready` and `presence.snapshot`, then pushes
//! events. The client may send `ping`, `presence.get`, `typing.start|stop` and
//! `call.*` signalling. The connection reconnects with exponential backoff and
//! refreshes the access token when the server rejects it (close code 1008).

use std::hash::{Hash, Hasher};
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use iced::Subscription;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use super::Client;
use super::models::{Message, User};

const PING_INTERVAL: Duration = Duration::from_secs(25);
/// If nothing (not even a pong) arrives for this long the socket is considered dead.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(70);
const MAX_BACKOFF_SECS: u64 = 30;

/// Events pushed by the server. Some fields are kept for completeness of the wire format.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ServerEvent {
    #[serde(rename = "connection.ready")]
    Ready { user_id: String },
    #[serde(rename = "presence.snapshot")]
    PresenceSnapshot { online_user_ids: Vec<String> },
    #[serde(rename = "presence.online")]
    Online { user_id: String },
    #[serde(rename = "presence.offline")]
    Offline { user_id: String },
    #[serde(rename = "presence.state")]
    PresenceState { user_id: String, online: bool },
    #[serde(rename = "typing.start")]
    TypingStart { chat_id: String, user_id: String },
    #[serde(rename = "typing.stop")]
    TypingStop { chat_id: String, user_id: String },
    #[serde(rename = "message.new")]
    MessageNew { message: Message },
    #[serde(rename = "message.updated")]
    MessageUpdated { message: Message },
    #[serde(rename = "message.deleted")]
    MessageDeleted { message: Message },
    #[serde(rename = "chat.created")]
    ChatCreated { chat_id: String },
    #[serde(rename = "chat.updated")]
    ChatUpdated { chat_id: String },
    #[serde(rename = "chat.read")]
    ChatRead { chat_id: String, user_id: String, read_at: DateTime<Utc> },
    #[serde(rename = "profile.updated")]
    ProfileUpdated { user: User },
    #[serde(rename = "account.blocked")]
    AccountBlocked,
    #[serde(rename = "pong")]
    Pong,
    /// `call.offer`, `call.answer`, `call.ice`, `call.end`, `call.started`, …
    #[serde(skip)]
    Call(serde_json::Value),
    #[serde(skip)]
    Unknown(String),
}

impl ServerEvent {
    pub fn parse(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let kind = value.get("type")?.as_str()?.to_owned();
        if kind.starts_with("call.") {
            return Some(Self::Call(value));
        }
        Some(serde_json::from_value(value).unwrap_or(Self::Unknown(kind)))
    }
}

/// Handle for sending JSON frames on the live connection.
#[derive(Debug, Clone)]
pub struct Connection(mpsc::UnboundedSender<serde_json::Value>);

impl Connection {
    pub fn send(&self, value: serde_json::Value) -> bool {
        self.0.send(value).is_ok()
    }

    pub fn typing(&self, chat_id: &str, active: bool) {
        let kind = if active { "typing.start" } else { "typing.stop" };
        self.send(serde_json::json!({ "type": kind, "chat_id": chat_id }));
    }

    pub fn request_presence(&self, user_id: &str) {
        self.send(serde_json::json!({ "type": "presence.get", "user_id": user_id }));
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Event {
    /// Authenticated and ready; outgoing frames go through the handle.
    Connected(Connection),
    Disconnected { retry_in: Duration },
    Server(ServerEvent),
    /// The refresh session is gone: the user must sign in again.
    SessionExpired(String),
}

/// Identifies one subscription; bumping `generation` forces a fresh connection.
#[derive(Clone)]
pub struct Config {
    pub client: Client,
    pub generation: u64,
}

impl Hash for Config {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.client.server().hash(state);
        self.generation.hash(state);
    }
}

pub fn subscription(config: Config) -> Subscription<Event> {
    Subscription::run_with(config, |config| run(config.clone()))
}

enum Outcome {
    /// Closed after a successful session; reconnect promptly.
    Dropped,
    /// The server rejected the token.
    Rejected,
    Failed,
}

/// The raw event stream behind [`subscription`] (also used by integration tests).
pub fn run(config: Config) -> impl futures::Stream<Item = Event> {
    iced::stream::channel(128, async move |mut output: futures::channel::mpsc::Sender<Event>| {
        let client = config.client;
        let mut attempt: u32 = 0;
        loop {
            let token = match client.access_token() {
                Some(token) => token,
                None => match client.refresh().await {
                    Ok(token) => token,
                    Err(error) if error.is_unauthorized() => {
                        let _ = output.send(Event::SessionExpired(error.message)).await;
                        return futures::future::pending().await;
                    }
                    Err(_) => {
                        attempt += 1;
                        backoff(&mut output, attempt).await;
                        continue;
                    }
                },
            };

            match session(&client.ws_url(), &token, &mut output).await {
                Outcome::Dropped => attempt = 1,
                Outcome::Failed => attempt += 1,
                Outcome::Rejected => {
                    attempt += 1;
                    match client.refresh_if_current(&token).await {
                        Ok(_) => {}
                        Err(error) if error.is_unauthorized() => {
                            let _ = output.send(Event::SessionExpired(error.message)).await;
                            return futures::future::pending().await;
                        }
                        Err(_) => {}
                    }
                }
            }
            backoff(&mut output, attempt).await;
        }
    })
}

async fn backoff(output: &mut futures::channel::mpsc::Sender<Event>, attempt: u32) {
    let seconds = 2u64.saturating_pow(attempt.saturating_sub(1).min(5)).min(MAX_BACKOFF_SECS);
    let retry_in = Duration::from_secs(seconds);
    let _ = output.send(Event::Disconnected { retry_in }).await;
    tokio::time::sleep(retry_in).await;
}

async fn session(url: &str, token: &str, output: &mut futures::channel::mpsc::Sender<Event>) -> Outcome {
    let connect = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(url));
    let stream = match connect.await {
        Ok(Ok((stream, _response))) => stream,
        Ok(Err(error)) => {
            log::info!("websocket connect failed: {error}");
            return Outcome::Failed;
        }
        Err(_) => {
            log::info!("websocket connect timed out");
            return Outcome::Failed;
        }
    };
    let (mut write, mut read) = stream.split();
    let auth = serde_json::json!({ "type": "auth", "token": token }).to_string();
    if write.send(WsMessage::text(auth)).await.is_err() {
        return Outcome::Failed;
    }

    let (sender, mut outgoing) = mpsc::unbounded_channel::<serde_json::Value>();
    let mut ready = false;
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    let mut last_seen = tokio::time::Instant::now();

    loop {
        tokio::select! {
            frame = read.next() => {
                last_seen = tokio::time::Instant::now();
                match frame {
                    Some(Ok(WsMessage::Text(text))) => {
                        let Some(event) = ServerEvent::parse(text.as_str()) else { continue };
                        if matches!(event, ServerEvent::Pong) {
                            continue;
                        }
                        if !ready && matches!(event, ServerEvent::Ready { .. }) {
                            ready = true;
                            let _ = output.send(Event::Connected(Connection(sender.clone()))).await;
                        }
                        if output.send(Event::Server(event)).await.is_err() {
                            return Outcome::Dropped;
                        }
                    }
                    Some(Ok(WsMessage::Close(frame))) => {
                        let policy = frame.as_ref().is_some_and(|frame| frame.code == CloseCode::Policy);
                        return if policy { Outcome::Rejected } else { Outcome::Dropped };
                    }
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        log::info!("websocket error: {error}");
                        return if ready { Outcome::Dropped } else { Outcome::Failed };
                    }
                    None => return if ready { Outcome::Dropped } else { Outcome::Failed },
                }
            }
            Some(value) = outgoing.recv() => {
                if write.send(WsMessage::text(value.to_string())).await.is_err() {
                    return Outcome::Dropped;
                }
            }
            _ = ping.tick() => {
                if last_seen.elapsed() > SILENCE_TIMEOUT {
                    log::info!("websocket silent for too long; reconnecting");
                    return Outcome::Dropped;
                }
                let frame = serde_json::json!({ "type": "ping" }).to_string();
                if write.send(WsMessage::text(frame)).await.is_err() {
                    return Outcome::Dropped;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_events() {
        let ready = ServerEvent::parse(r#"{"type":"connection.ready","user_id":"u1"}"#).unwrap();
        assert!(matches!(ready, ServerEvent::Ready { user_id } if user_id == "u1"));

        let typing = ServerEvent::parse(r#"{"type":"typing.start","chat_id":"c","user_id":"u"}"#).unwrap();
        assert!(matches!(typing, ServerEvent::TypingStart { .. }));

        let read = ServerEvent::parse(r#"{"type":"chat.read","chat_id":"c","user_id":"u","read_at":"2026-11-17T10:00:00.5+00:00"}"#).unwrap();
        assert!(matches!(read, ServerEvent::ChatRead { .. }));

        let call = ServerEvent::parse(r#"{"type":"call.offer","call_id":"x","sdp":{}}"#).unwrap();
        assert!(matches!(call, ServerEvent::Call(_)));

        let unknown = ServerEvent::parse(r#"{"type":"future.thing","x":1}"#).unwrap();
        assert!(matches!(unknown, ServerEvent::Unknown(kind) if kind == "future.thing"));

        assert!(ServerEvent::parse("not json").is_none());
        assert!(matches!(ServerEvent::parse(r#"{"type":"account.blocked"}"#), Some(ServerEvent::AccountBlocked)));
    }
}
