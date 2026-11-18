//! In-app updates (Telegram-style): check a release manifest, download the new
//! installer in the background, verify it, then install and restart on request.
//!
//! Trust model: the manifest (`latest.json`, published with each GitHub Release)
//! lists the installer URL, size, SHA-256 and an Ed25519 signature. The signature
//! covers `messenger-update-v1\n<version>\n<sha256-hex>\n`, so a tampered file, a
//! swapped manifest entry or a replayed older version are all rejected. Only the
//! public key ships in the app; the signing key lives in a GitHub Actions secret.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use iced::widget::{button, column, container, progress_bar, row, scrollable};
use iced::{Alignment, Element, Length, Subscription, Task};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::app::Message;
use crate::storage::{Dirs, Settings};
use crate::ui::icons::Icon;
use crate::ui::theme;
use crate::ui::widgets::{self, hspace, label, modal, strong};

/// Where the newest manifest lives. Override at build time with MESSENGER_UPDATE_URL.
pub const MANIFEST_URL: &str = match option_env!("MESSENGER_UPDATE_URL") {
    Some(url) => url,
    None => "https://github.com/monoreaz/realtime-messenger/releases/latest/download/latest.json",
};

/// Base64 Ed25519 public key (`desktop/update-public-key.txt`). Empty → updates disabled.
const PUBLIC_KEY: &str = include_str!("../update-public-key.txt");
const PLATFORM: &str = "windows-x86_64";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is valid semver")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: Option<String>,
    pub platforms: std::collections::HashMap<String, Asset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Hash)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub date: Option<String>,
    pub asset: Asset,
}

pub fn signed_message(version: &Version, sha256_hex: &str) -> Vec<u8> {
    format!("messenger-update-v1\n{version}\n{}\n", sha256_hex.to_ascii_lowercase()).into_bytes()
}

fn public_key() -> Option<VerifyingKey> {
    let encoded = PUBLIC_KEY.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('#'))?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    VerifyingKey::from_bytes(&bytes.try_into().ok()?).ok()
}

pub fn updates_enabled() -> bool {
    public_key().is_some()
}

/// Verifies the manifest signature for a release (before downloading anything).
pub fn verify_release(release: &Release, key: &VerifyingKey) -> Result<(), String> {
    let signature = base64::engine::general_purpose::STANDARD.decode(release.asset.signature.trim()).map_err(|_| "The update signature is malformed.")?;
    let signature = Signature::from_slice(&signature).map_err(|_| "The update signature is malformed.")?;
    key.verify(&signed_message(&release.version, &release.asset.sha256), &signature).map_err(|_| "The update signature is invalid.".to_owned())
}

/// Picks the newer release for this platform from a manifest, if any.
pub fn select_release(manifest: Manifest, current: &Version) -> Result<Option<Release>, String> {
    let version = Version::parse(manifest.version.trim_start_matches('v')).map_err(|_| "The update manifest has an invalid version.".to_owned())?;
    if version <= *current {
        return Ok(None);
    }
    let asset = manifest.platforms.get(PLATFORM).cloned().ok_or_else(|| "No Windows build in this release.".to_owned())?;
    if !asset.url.starts_with("https://") || asset.sha256.len() != 64 {
        return Err("The update manifest is invalid.".into());
    }
    Ok(Some(Release { version, notes: manifest.notes, date: manifest.pub_date, asset }))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading { release: Release, received: u64, total: u64 },
    Ready { release: Release, path: PathBuf },
    Installing,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Msg {
    Check,
    Checked(Result<Option<Release>, String>),
    Download,
    Progress(u64, u64),
    Downloaded(Result<PathBuf, String>),
    Install,
    Later,
    Skip,
    OpenDialog,
}

pub struct Updater {
    pub state: State,
    dir: PathBuf,
    /// Set when the user dismissed the banner for this session.
    pub dismissed: bool,
    auto: bool,
}

pub fn startup_task(settings: &Settings) -> Task<Message> {
    if !settings.auto_update || !updates_enabled() {
        return Task::none();
    }
    Task::perform(tokio::time::sleep(Duration::from_secs(4)), |_| Message::Update(Msg::Check))
}

async fn fetch_manifest() -> Result<Option<Release>, String> {
    let http = reqwest::Client::builder()
        .user_agent(crate::api::client::user_agent())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let response = http.get(MANIFEST_URL).send().await.map_err(|error| format!("Could not check for updates: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(format!("Could not check for updates (HTTP {}).", response.status().as_u16()));
    }
    let manifest: Manifest = response.json().await.map_err(|_| "The update manifest could not be read.".to_owned())?;
    let release = select_release(manifest, &current_version())?;
    if let Some(release) = &release {
        let key = public_key().ok_or("Updates are not configured in this build.")?;
        verify_release(release, &key)?;
    }
    Ok(release)
}

impl Updater {
    pub fn new(dirs: &Dirs) -> Self {
        let dir = dirs.updates();
        let mut updater = Self { state: State::Idle, dir, dismissed: false, auto: true };
        updater.cleanup_installed();
        updater
    }

    /// Removes installers for versions that are already installed.
    fn cleanup_installed(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let current = current_version();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = name.strip_prefix("Messenger-Setup-").and_then(|rest| rest.strip_suffix(".exe")).and_then(|v| Version::parse(v).ok());
            let stale = match version {
                Some(version) => version <= current,
                None => name.ends_with(".part"),
            };
            if stale {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    pub fn banner_visible(&self) -> bool {
        !self.dismissed && matches!(self.state, State::Available(_) | State::Downloading { .. } | State::Ready { .. })
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        let mut subscriptions = Vec::new();
        if self.auto && updates_enabled() {
            subscriptions.push(iced::time::every(CHECK_INTERVAL).map(|_| Msg::Check));
        }
        if let State::Downloading { release, .. } = &self.state {
            subscriptions.push(Subscription::run_with(Job { release: release.clone(), dir: self.dir.clone() }, |job| download(job.clone())));
        }
        Subscription::batch(subscriptions)
    }

    pub fn update(&mut self, msg: Msg, settings: &mut Settings, dirs: &Dirs) -> Task<Msg> {
        self.auto = settings.auto_update;
        match msg {
            Msg::Check => {
                if matches!(self.state, State::Checking | State::Downloading { .. } | State::Ready { .. } | State::Installing) {
                    return Task::none();
                }
                if !updates_enabled() {
                    self.state = State::Failed("Updates are not configured in this build.".into());
                    return Task::none();
                }
                self.state = State::Checking;
                return Task::perform(fetch_manifest(), Msg::Checked);
            }
            Msg::Checked(result) => match result {
                Ok(Some(release)) => {
                    let skipped = settings.skipped_version.as_deref() == Some(release.version.to_string().as_str());
                    let existing = self.dir.join(installer_name(&release.version));
                    if existing.exists() && sha256_file(&existing).is_ok_and(|hash| hash == release.asset.sha256.to_ascii_lowercase()) {
                        self.state = State::Ready { release, path: existing };
                    } else if settings.auto_update && !skipped {
                        self.state = State::Downloading { total: release.asset.size, release, received: 0 };
                    } else {
                        self.dismissed = skipped;
                        self.state = State::Available(release);
                    }
                }
                Ok(None) => self.state = State::UpToDate,
                Err(error) => {
                    log::info!("update check failed: {error}");
                    self.state = State::Failed(error);
                }
            },
            Msg::Download => {
                let available = match &self.state {
                    State::Available(release) => Some(release.clone()),
                    _ => None,
                };
                match available {
                    Some(release) => {
                        self.dismissed = false;
                        self.state = State::Downloading { total: release.asset.size, release, received: 0 };
                    }
                    None if matches!(self.state, State::Failed(_) | State::Idle | State::UpToDate) => {
                        return self.update(Msg::Check, settings, dirs);
                    }
                    None => {}
                }
            }
            Msg::Progress(received, total) => {
                if let State::Downloading { received: current, total: expected, .. } = &mut self.state {
                    *current = received;
                    if total > 0 {
                        *expected = total;
                    }
                }
            }
            Msg::Downloaded(result) => {
                let State::Downloading { release, .. } = self.state.clone() else { return Task::none() };
                self.state = match result {
                    Ok(path) => State::Ready { release, path },
                    Err(error) => State::Failed(error),
                };
            }
            Msg::Install => {
                let State::Ready { path, release } = self.state.clone() else { return Task::none() };
                // Re-verify right before executing: the file sat on disk since download.
                match verify_file(&path, &release) {
                    Ok(()) => {}
                    Err(error) => {
                        let _ = std::fs::remove_file(&path);
                        self.state = State::Failed(error);
                        return Task::none();
                    }
                }
                settings.save(dirs);
                match launch_installer(&path) {
                    Ok(()) => {
                        self.state = State::Installing;
                        return iced::exit();
                    }
                    Err(error) => self.state = State::Failed(error),
                }
            }
            Msg::Later => self.dismissed = true,
            Msg::Skip => {
                if let State::Available(release) | State::Ready { release, .. } = &self.state {
                    settings.skipped_version = Some(release.version.to_string());
                    settings.save(dirs);
                }
                self.dismissed = true;
            }
            Msg::OpenDialog => {}
        }
        Task::none()
    }

    pub fn on_exit(&self) {}
}

fn installer_name(version: &Version) -> String {
    format!("Messenger-Setup-{version}.exe")
}

fn verify_file(path: &Path, release: &Release) -> Result<(), String> {
    let key = public_key().ok_or("Updates are not configured in this build.")?;
    verify_release(release, &key)?;
    let hash = sha256_file(path).map_err(|error| format!("Could not read the update: {error}"))?;
    if hash != release.asset.sha256.to_ascii_lowercase() {
        return Err("The downloaded update is corrupted. It will be downloaded again.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn launch_installer(path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    // Inno Setup: per-user, no UI; the installer restarts the app when it finishes.
    std::process::Command::new(path)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/UPDATE"])
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(drop)
        .map_err(|error| format!("Could not start the installer: {error}"))
}

#[cfg(not(windows))]
fn launch_installer(path: &Path) -> Result<(), String> {
    Err(format!("Installing updates is only supported on Windows ({}).", path.display()))
}

#[derive(Clone)]
struct Job {
    release: Release,
    dir: PathBuf,
}

impl std::hash::Hash for Job {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.release.hash(state);
    }
}

fn download(job: Job) -> impl futures::Stream<Item = Msg> {
    iced::stream::channel(32, async move |mut output: futures::channel::mpsc::Sender<Msg>| {
        use futures::SinkExt;
        let result = download_inner(&job, &mut output).await;
        let _ = output.send(Msg::Downloaded(result)).await;
        futures::future::pending::<()>().await;
    })
}

async fn download_inner(job: &Job, output: &mut futures::channel::mpsc::Sender<Msg>) -> Result<PathBuf, String> {
    use futures::{SinkExt, StreamExt};
    use tokio::io::AsyncWriteExt;

    tokio::fs::create_dir_all(&job.dir).await.map_err(|error| format!("Could not prepare the download folder: {error}"))?;
    let target = job.dir.join(installer_name(&job.release.version));
    let partial = target.with_extension("part");
    let http = reqwest::Client::builder()
        .user_agent(crate::api::client::user_agent())
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let response = http.get(&job.release.asset.url).send().await.map_err(|error| format!("Download failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("Download failed (HTTP {}).", response.status().as_u16()));
    }
    let total = response.content_length().unwrap_or(job.release.asset.size);
    if total > 512 * 1024 * 1024 {
        return Err("The update is unexpectedly large.".into());
    }
    let mut file = tokio::fs::File::create(&partial).await.map_err(|error| format!("Could not save the update: {error}"))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut last_report = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Download interrupted: {error}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|error| format!("Could not save the update: {error}"))?;
        received += chunk.len() as u64;
        if received - last_report > 256 * 1024 || received == total {
            last_report = received;
            let _ = output.send(Msg::Progress(received, total)).await;
        }
    }
    file.flush().await.map_err(|error| error.to_string())?;
    drop(file);
    let hash = hex(&hasher.finalize());
    if hash != job.release.asset.sha256.to_ascii_lowercase() || received != job.release.asset.size {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err("The downloaded update failed verification and was discarded.".into());
    }
    tokio::fs::rename(&partial, &target).await.map_err(|error| format!("Could not save the update: {error}"))?;
    Ok(target)
}

// ============================================================================
// Views
// ============================================================================

fn umsg(msg: Msg) -> Message {
    Message::Update(msg)
}

/// Compact banner at the bottom of the chat list (like Telegram's "Update" button).
pub fn banner(updater: &Updater) -> Option<Element<'_, Message>> {
    if !updater.banner_visible() {
        return None;
    }
    let (title, detail, action): (String, String, Option<(&str, Message)>) = match &updater.state {
        State::Available(release) => (format!("Update available · {}", release.version), "A new version of Messenger is ready to download.".into(), Some(("Update", umsg(Msg::Download)))),
        State::Downloading { release, received, total } => {
            let percent = if *total > 0 { (*received as f64 / *total as f64 * 100.0).round() as u32 } else { 0 };
            (format!("Downloading {} · {percent}%", release.version), "You can keep chatting.".into(), None)
        }
        State::Ready { release, .. } => (format!("Update ready · {}", release.version), "Restart Messenger to finish updating.".into(), Some(("Restart", umsg(Msg::Install)))),
        _ => return None,
    };
    let mut content = column![
        row![
            container(widgets::icon(Icon::Refresh, 16.0, theme::ACCENT)).center(30).style(theme::pill(theme::ACCENT, theme::ACCENT_SOFT)),
            column![strong(title, theme::TEXT_SM, theme::TEXT_1), label(detail, theme::TEXT_XS, theme::TEXT_2)].spacing(1).width(Length::Fill),
            widgets::icon_button(Icon::Close, Some(umsg(Msg::Later)), 28.0),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(8);
    if let State::Downloading { received, total, .. } = &updater.state {
        let fraction = if *total > 0 { *received as f32 / *total as f32 } else { 0.0 };
        content = content.push(progress_bar(0.0..=1.0, fraction).girth(4).style(progress_style));
    }
    let mut actions = row![
        button(label("What's new", theme::TEXT_XS, theme::TEXT_2)).padding([4, 8]).style(theme::link_button).on_press(Message::OpenDialog(crate::dialogs::Open::Update)),
        hspace()
    ]
    .align_y(Alignment::Center);
    if let Some((name, message)) = action {
        actions = actions.push(widgets::small_primary(name, Some(message)));
    }
    content = content.push(actions);
    Some(container(content).padding(12).width(Length::Fill).style(theme::update_banner).into())
}

fn progress_style(_: &iced::Theme) -> progress_bar::Style {
    progress_bar::Style {
        background: theme::SURFACE_3.into(),
        bar: theme::ACCENT.into(),
        border: iced::Border { radius: 999.0.into(), ..Default::default() },
    }
}

/// "Updates" card in Settings → Application.
pub fn settings_card(updater: &Updater) -> Element<'_, Message> {
    let status: String = match &updater.state {
        _ if !updates_enabled() => "Updates are not configured in this development build.".into(),
        State::Idle => format!("You are on version {}.", current_version()),
        State::Checking => "Checking for updates…".into(),
        State::UpToDate => format!("Messenger {} is up to date.", current_version()),
        State::Available(release) => format!("Version {} is available.", release.version),
        State::Downloading { release, received, total } => format!("Downloading {} · {} of {}", release.version, crate::format::megabytes(*received), crate::format::megabytes(*total)),
        State::Ready { release, .. } => format!("Version {} is ready to install.", release.version),
        State::Installing => "Installing…".into(),
        State::Failed(error) => error.clone(),
    };
    let action: Element<'_, Message> = match &updater.state {
        State::Available(_) => widgets::small_primary("Download", Some(umsg(Msg::Download))),
        State::Ready { .. } => widgets::small_primary("Restart and update", Some(umsg(Msg::Install))),
        State::Checking | State::Downloading { .. } | State::Installing => widgets::small_button("Check now", None),
        _ => widgets::small_button("Check now", updates_enabled().then_some(umsg(Msg::Check))),
    };
    let mut content = column![
        row![
            column![strong("Updates", theme::TEXT_MD, theme::TEXT_1), label(status, theme::TEXT_SM, theme::TEXT_2)].spacing(3).width(Length::Fill),
            action,
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    ]
    .spacing(10);
    if let State::Downloading { received, total, .. } = &updater.state {
        let fraction = if *total > 0 { *received as f32 / *total as f32 } else { 0.0 };
        content = content.push(progress_bar(0.0..=1.0, fraction).girth(4).style(progress_style));
    }
    container(content).padding(16).width(Length::Fill).style(theme::card).into()
}

/// Release notes dialog.
pub fn dialog(updater: &Updater) -> Element<'_, Message> {
    let release = match &updater.state {
        State::Available(release) | State::Ready { release, .. } | State::Downloading { release, .. } => Some(release),
        _ => None,
    };
    let Some(release) = release else {
        return modal(column![widgets::dialog_header("Updates", Some(Message::CloseDialog)), crate::update::settings_card(updater)].spacing(14).into(), 440.0, Some(Message::CloseDialog));
    };
    let notes = if release.notes.trim().is_empty() { "No release notes." } else { release.notes.as_str() };
    let mut actions = row![
        button(label("Skip this version", theme::TEXT_SM, theme::TEXT_3)).padding([8, 10]).style(theme::link_button).on_press(umsg(Msg::Skip)),
        hspace(),
        widgets::small_button("Later", Some(Message::CloseDialog)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    actions = match &updater.state {
        State::Available(_) => actions.push(widgets::small_primary("Update", Some(umsg(Msg::Download)))),
        State::Ready { .. } => actions.push(widgets::small_primary("Restart and update", Some(umsg(Msg::Install)))),
        _ => actions,
    };
    let content = column![
        widgets::dialog_header(format!("Messenger {}", release.version), Some(Message::CloseDialog)),
        label(
            format!("You have {}. {}", current_version(), release.date.as_deref().map(|date| format!("Released {date}.")).unwrap_or_default()),
            theme::TEXT_SM,
            theme::TEXT_3
        ),
        container(scrollable(label(notes, theme::TEXT_MD, theme::TEXT_1)).height(Length::Shrink).style(theme::scroll)).padding(14).max_height(360).width(Length::Fill).style(theme::card),
        settings_card(updater),
        actions,
    ]
    .spacing(12);
    modal(content.into(), 500.0, Some(Message::CloseDialog))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn manifest(version: &str, signature: &str, sha: &str) -> Manifest {
        serde_json::from_value(serde_json::json!({
            "version": version, "notes": "Fixes", "pub_date": "2026-11-20",
            "platforms": {"windows-x86_64": {"url": "https://example.com/Messenger-Setup.exe", "sha256": sha, "size": 3, "signature": signature}}
        }))
        .unwrap()
    }

    #[test]
    fn selects_only_newer_versions() {
        let sha = "a".repeat(64);
        let current = Version::parse("0.2.0").unwrap();
        assert!(select_release(manifest("0.1.9", "", &sha), &current).unwrap().is_none());
        assert!(select_release(manifest("0.2.0", "", &sha), &current).unwrap().is_none());
        let release = select_release(manifest("v0.3.0", "", &sha), &current).unwrap().unwrap();
        assert_eq!(release.version, Version::parse("0.3.0").unwrap());
        assert!(select_release(manifest("0.3.0", "", "short"), &current).is_err());
    }

    #[test]
    fn verifies_signatures_and_rejects_tampering() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let file = b"new installer bytes";
        let sha = hex(&Sha256::digest(file));
        let version = Version::parse("1.2.3").unwrap();
        let signature = base64::engine::general_purpose::STANDARD.encode(key.sign(&signed_message(&version, &sha)).to_bytes());
        let release = select_release(manifest("1.2.3", &signature, &sha), &Version::parse("1.0.0").unwrap()).unwrap().unwrap();
        assert!(verify_release(&release, &key.verifying_key()).is_ok());

        // Same signature presented for a different version (replay) must fail.
        let replay = select_release(manifest("1.2.4", &signature, &sha), &Version::parse("1.0.0").unwrap()).unwrap().unwrap();
        assert!(verify_release(&replay, &key.verifying_key()).is_err());

        // A different file hash must fail.
        let tampered = select_release(manifest("1.2.3", &signature, &"b".repeat(64)), &Version::parse("1.0.0").unwrap()).unwrap().unwrap();
        assert!(verify_release(&tampered, &key.verifying_key()).is_err());

        // A different key must fail.
        assert!(verify_release(&release, &SigningKey::from_bytes(&[9u8; 32]).verifying_key()).is_err());
    }

    #[test]
    fn hashes_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
