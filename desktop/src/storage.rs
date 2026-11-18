//! Persistent local data: settings (plain JSON) and the remembered session.
//!
//! The refresh token is a credential. On Windows it is encrypted with DPAPI
//! (`CryptProtectData`, bound to the current Windows user) before it touches disk.
//! On other platforms (development builds only) it is stored with 0600 permissions.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVER: &str = "https://messenger-01.tail1a26d2.ts.net";

/// Directories for this app: `%APPDATA%\Messenger` (config) and
/// `%LOCALAPPDATA%\Messenger` (cache, updates) on Windows.
#[derive(Debug, Clone)]
pub struct Dirs {
    pub config: PathBuf,
    pub cache: PathBuf,
}

impl Dirs {
    pub fn resolve() -> Self {
        if let Ok(root) = std::env::var("MESSENGER_DATA_DIR") {
            let root = PathBuf::from(root);
            return Self { config: root.join("config"), cache: root.join("cache") };
        }
        match directories::ProjectDirs::from("", "", "Messenger") {
            Some(dirs) => Self { config: dirs.config_dir().to_path_buf(), cache: dirs.cache_dir().to_path_buf() },
            None => {
                let fallback = std::env::temp_dir().join("Messenger");
                Self { config: fallback.join("config"), cache: fallback.join("cache") }
            }
        }
    }

    pub fn updates(&self) -> PathBuf {
        self.cache.join("updates")
    }

    pub fn downloads_fallback(&self) -> PathBuf {
        directories::UserDirs::new()
            .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| self.cache.join("downloads"))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server: String,
    pub notifications: bool,
    pub notification_previews: bool,
    pub auto_update: bool,
    /// Last username typed on the login screen (not a secret).
    pub last_username: String,
    pub window_width: f32,
    pub window_height: f32,
    pub window_maximized: bool,
    /// Version the user chose to skip in the update prompt.
    pub skipped_version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: DEFAULT_SERVER.to_owned(),
            notifications: true,
            notification_previews: true,
            auto_update: true,
            last_username: String::new(),
            window_width: 1240.0,
            window_height: 800.0,
            window_maximized: false,
            skipped_version: None,
        }
    }
}

impl Settings {
    fn path(dirs: &Dirs) -> PathBuf {
        dirs.config.join("settings.json")
    }

    pub fn load(dirs: &Dirs) -> Self {
        std::fs::read(Self::path(dirs))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dirs: &Dirs) {
        let result = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|bytes| write_atomic(&Self::path(dirs), &bytes, false));
        if let Err(error) = result {
            log::warn!("could not save settings: {error}");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub server: String,
    pub refresh_cookie: String,
    pub username: Option<String>,
}

/// Persists the remembered session; cloned into the API client.
#[derive(Debug, Clone)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(dirs: &Dirs) -> Self {
        Self { path: dirs.config.join("session.bin") }
    }

    pub fn load(&self) -> Option<StoredSession> {
        let sealed = std::fs::read(&self.path).ok()?;
        let plain = match protect::unseal(&sealed) {
            Ok(plain) => plain,
            Err(error) => {
                log::warn!("stored session could not be decrypted: {error}");
                self.clear();
                return None;
            }
        };
        serde_json::from_slice(&plain).ok()
    }

    pub fn save(&self, session: &StoredSession) {
        let result = serde_json::to_vec(session)
            .map_err(std::io::Error::other)
            .and_then(|plain| protect::seal(&plain))
            .and_then(|sealed| write_atomic(&self.path, &sealed, true));
        if let Err(error) = result {
            log::warn!("could not persist session: {error}");
        }
    }

    pub fn clear(&self) {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("could not remove stored session: {error}"),
        }
    }
}

/// Writes via a temporary file + rename so a crash never leaves a half-written file.
fn write_atomic(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)
}

#[cfg(windows)]
mod protect {
    //! DPAPI wrappers. Data sealed here can only be unsealed by the same Windows user.
    use std::io;
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    const ENTROPY: &[u8] = b"realtime-messenger-desktop-session-v1";

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    unsafe fn take(output: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: DPAPI returned a LocalAlloc'd buffer of `cbData` bytes that we own.
        unsafe {
            let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData.cast());
            bytes
        }
    }

    pub fn seal(plain: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(plain);
        let entropy = blob(ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        // SAFETY: all pointers reference live buffers for the duration of the call.
        let ok = unsafe {
            CryptProtectData(&input, ptr::null(), &entropy, ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { take(output) })
    }

    pub fn unseal(sealed: &[u8]) -> io::Result<Vec<u8>> {
        let input = blob(sealed);
        let entropy = blob(ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        // SAFETY: as above.
        let ok = unsafe {
            CryptUnprotectData(&input, ptr::null_mut(), &entropy, ptr::null(), ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { take(output) })
    }
}

#[cfg(not(windows))]
mod protect {
    //! Development fallback: no OS keystore, rely on file permissions (0600).
    pub fn seal(plain: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(plain.to_vec())
    }

    pub fn unseal(sealed: &[u8]) -> std::io::Result<Vec<u8>> {
        Ok(sealed.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(root: &Path) -> Dirs {
        Dirs { config: root.join("config"), cache: root.join("cache") }
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let root = tempfile::tempdir().unwrap();
        let dirs = dirs(root.path());
        assert_eq!(Settings::load(&dirs), Settings::default());
        let settings = Settings { server: "https://x".into(), notifications: false, ..Settings::default() };
        settings.save(&dirs);
        assert_eq!(Settings::load(&dirs), settings);
        // Unknown/missing fields fall back to defaults.
        std::fs::write(dirs.config.join("settings.json"), br#"{"server":"https://y","future":1}"#).unwrap();
        let loaded = Settings::load(&dirs);
        assert_eq!(loaded.server, "https://y");
        assert!(loaded.notifications);
    }

    #[test]
    fn session_round_trip_and_clear() {
        let root = tempfile::tempdir().unwrap();
        let store = SessionStore::new(&dirs(root.path()));
        assert!(store.load().is_none());
        let session = StoredSession { server: "https://x".into(), refresh_cookie: "secret".into(), username: Some("ann".into()) };
        store.save(&session);
        assert_eq!(store.load(), Some(session));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join("config/session.bin")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.clear();
        assert!(store.load().is_none());
        store.clear();
    }
}
