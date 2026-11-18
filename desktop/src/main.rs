//! Messenger Desktop — native Windows client for Realtime Messenger (Rust + iced).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod app;
mod dialogs;
mod format;
mod media;
mod platform;
mod storage;
mod ui;
mod update;

use app::App;

fn window_icon() -> Option<iced::window::Icon> {
    let decoded = image::load_from_memory(include_bytes!("../assets/brand-mark.webp")).ok()?;
    let icon = decoded.resize(256, 256, image::imageops::FilterType::Triangle).into_rgba8();
    let (width, height) = icon.dimensions();
    iced::window::icon::from_rgba(icon.into_raw(), width, height).ok()
}

fn init_logging() {
    let dirs = storage::Dirs::resolve();
    let mut builder = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn,iced_wgpu=warn,cosmic_text=warn"));
    // Release builds have no console on Windows: log to a file next to the cache.
    if !cfg!(debug_assertions) && std::fs::create_dir_all(&dirs.cache).is_ok() {
        let path = dirs.cache.join("messenger.log");
        // Keep the log small: start fresh when it grows past 2 MB.
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 2 * 1024 * 1024) {
            let _ = std::fs::remove_file(&path);
        }
        if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.init();
}

fn main() -> iced::Result {
    init_logging();
    platform::init();
    log::info!("Messenger Desktop {} starting", env!("CARGO_PKG_VERSION"));

    let settings = storage::Settings::load(&storage::Dirs::resolve());
    let size = iced::Size::new(settings.window_width.max(720.0), settings.window_height.max(480.0));

    iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(|_: &App| ui::theme::app_theme())
        .default_font(ui::theme::REGULAR)
        .antialiasing(true)
        .exit_on_close_request(false)
        .window(iced::window::Settings {
            size,
            min_size: Some(iced::Size::new(520.0, 460.0)),
            icon: window_icon(),
            exit_on_close_request: false,
            #[cfg(windows)]
            platform_specific: iced::window::settings::PlatformSpecific {
                drag_and_drop: true,
                skip_taskbar: false,
                undecorated_shadow: false,
                corner_preference: Default::default(),
            },
            ..iced::window::Settings::default()
        })
        .run()
}
