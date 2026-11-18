//! OS integration: desktop notifications and Windows app identity.

/// AppUserModelID shared with the installer's Start Menu shortcut, so toasts are
/// attributed to "Messenger" and taskbar buttons group correctly.
#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_USER_MODEL_ID: &str = "monoreaz.Messenger";

/// Must run before any window is created.
pub fn init() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let id: Vec<u16> = APP_USER_MODEL_ID.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `id` is a valid NUL-terminated UTF-16 string for the call's duration.
        let result = unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
        if result != 0 {
            log::warn!("SetCurrentProcessExplicitAppUserModelID failed: {result:#x}");
        }
    }
}

/// Shows a desktop notification without blocking the UI thread.
pub fn notify(title: String, body: String) {
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.summary(&title).body(&body).appname("Messenger");
        #[cfg(windows)]
        notification.app_id(APP_USER_MODEL_ID);
        if let Err(error) = notification.show() {
            log::info!("notification failed: {error}");
        }
    });
}

/// Opens a file or URL with the system default handler.
pub fn open(target: String) {
    std::thread::spawn(move || {
        if let Err(error) = open::that_detached(&target) {
            log::warn!("could not open {target}: {error}");
        }
    });
}
