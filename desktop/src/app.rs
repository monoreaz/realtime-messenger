//! Application state machine (Elm architecture): `Message` → `update` → `view`.
//!
//! Screens: `Loading` (restoring a remembered session) → `Auth` → `Main`.
//! All network I/O runs in `Task`s on the Tokio runtime; `update` never blocks.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{Id, operation, scrollable};
use iced::{Point, Subscription, Task, window};

use crate::api::models::{Chat, Message as ChatMessage, User};
use crate::api::ws::{self, Connection, ServerEvent};
use crate::api::{ApiError, Client};
use crate::dialogs::{self, Dialog};
use crate::media::{self, Cache as MediaCache, Key as MediaKey};
use crate::storage::{Dirs, SessionStore, Settings};
use crate::update::{self as updater, Updater};
use crate::{platform, ui};

pub const MESSAGES_SCROLL: &str = "messages";
pub const COMPOSER_INPUT: &str = "composer";
pub const SEARCH_INPUT: &str = "search";
/// Messages kept per cached chat thread before old pages are dropped.
const THREAD_CACHE: usize = 12;
/// Number of loaded messages rendered at once (the rest stay in memory).
const RENDER_WINDOW: usize = 160;
const TYPING_SEND_INTERVAL: Duration = Duration::from_secs(3);
const TYPING_IDLE_STOP: Duration = Duration::from_secs(4);
const TYPING_EXPIRY: Duration = Duration::from_secs(6);

/// The message list is anchored to the bottom, so offset 0 (start) is the newest message.
pub fn scroll_to_bottom<T>() -> Task<T> {
    operation::snap_to(messages_scroll(), operation::RelativeOffset::START)
}

/// Last pointer position, recorded straight from the event stream. Tracking it through
/// messages would rebuild the whole view on every mouse move.
static POINTER: std::sync::Mutex<Point> = std::sync::Mutex::new(Point::ORIGIN);

pub fn pointer_position() -> Point {
    *POINTER.lock().unwrap()
}

pub fn messages_scroll() -> Id {
    Id::from(MESSAGES_SCROLL)
}

pub fn composer_input() -> Id {
    Id::from(COMPOSER_INPUT)
}

pub fn search_input() -> Id {
    Id::from(SEARCH_INPUT)
}

// ============================================================================
// State
// ============================================================================

pub struct App {
    pub dirs: Dirs,
    pub settings: Settings,
    pub store: SessionStore,
    pub client: Client,
    pub screen: Screen,
    pub window: Option<window::Id>,
    pub focused: bool,
    pub updater: Updater,
    pub window_size: iced::Size,
    /// Physical pixels per logical pixel, for composing the backdrop at full resolution.
    pub scale: f32,
    /// Composed backdrop and the window size it was made for (see `ui::backdrop`).
    pub backdrop: Option<(iced::Size, iced::widget::image::Handle)>,
    backdrop_generation: u64,
}

pub enum Screen {
    Loading,
    Auth(ui::auth::State),
    Main(Box<Main>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStatus {
    Connecting,
    Connected,
    Offline,
}

pub struct Main {
    pub me: User,
    pub chats: Vec<Chat>,
    pub online: HashSet<String>,
    /// Offline peers' last-seen times, only where both sides' privacy settings allow it.
    pub last_seen: HashMap<String, DateTime<Utc>>,
    /// Users this account blocked; `None` until loaded.
    pub blocked: Option<Vec<User>>,
    /// The user a block/unblock request is in flight for.
    pub block_busy: Option<String>,
    /// chat id → (user id → last typing signal)
    pub typing: HashMap<String, HashMap<String, Instant>>,
    pub ws_status: WsStatus,
    pub connection: Option<Connection>,
    pub ws_generation: u64,
    pub active: Option<String>,
    pub threads: HashMap<String, Thread>,
    pub thread_order: VecDeque<String>,
    pub search: Search,
    pub composer: Composer,
    pub context_menu: Option<ContextMenu>,
    pub dialog: Option<Dialog>,
    pub media: MediaCache,
    pub toast: Option<Toast>,
    pub typing_sent: Option<(String, Instant)>,
    pub last_input: Option<Instant>,
    pub at_bottom: bool,
    pub viewport_height: f32,
    pub highlight: Option<(String, Instant)>,
    pub msg_scroll: crate::ui::smooth::Smooth,
    pub list_scroll: crate::ui::smooth::Smooth,
    pub pin_cursor: usize,
    pub pending_jump: Option<String>,
    pub call: Option<crate::call::ActiveCall>,
}

impl Main {
    /// The other user of the open private chat.
    pub fn active_peer(&self) -> Option<&User> {
        let active = self.active.as_deref()?;
        self.chats.iter().find(|chat| chat.id == active).filter(|chat| !chat.is_group())?.peer.as_ref()
    }

    pub fn is_blocked(&self, user_id: &str) -> bool {
        self.blocked.as_ref().is_some_and(|blocked| blocked.iter().any(|user| user.id == user_id))
    }

    /// "Online", "Last seen …" or "Offline", with its colour (web `.peer-status`).
    pub fn presence(&self, user_id: &str) -> (String, iced::Color) {
        if self.online.contains(user_id) {
            ("Online".to_owned(), crate::ui::theme::SUCCESS)
        } else if let Some(at) = self.last_seen.get(user_id) {
            (crate::format::last_seen(*at, Utc::now()), crate::ui::theme::TEXT_3)
        } else {
            ("Offline".to_owned(), crate::ui::theme::TEXT_3)
        }
    }
}

#[derive(Debug, Default)]
pub struct Thread {
    /// Oldest first.
    pub messages: Vec<ChatMessage>,
    pub loading: bool,
    pub loading_older: bool,
    pub has_older: bool,
    pub loaded: bool,
    pub pins: Vec<ChatMessage>,
    /// How many of the newest messages are rendered.
    pub window: usize,
    pub error: Option<String>,
}

impl Thread {
    fn upsert(&mut self, message: ChatMessage) {
        match self.messages.iter_mut().rev().find(|item| item.id == message.id) {
            Some(existing) => *existing = message,
            None => {
                let position = self
                    .messages
                    .iter()
                    .rposition(|item| (item.created_at, &item.id) <= (message.created_at, &message.id))
                    .map_or(0, |index| index + 1);
                self.messages.insert(position, message);
                self.window += 1;
            }
        }
    }

    fn update_pins(&mut self, message: &ChatMessage) {
        self.pins.retain(|pin| pin.id != message.id);
        if message.pinned_at.is_some() && !message.is_deleted() {
            self.pins.push(message.clone());
            self.pins.sort_by(|a, b| b.pinned_at.cmp(&a.pinned_at));
        }
    }

    pub fn visible(&self) -> &[ChatMessage] {
        let start = self.messages.len().saturating_sub(self.window.max(RENDER_WINDOW / 2));
        &self.messages[start..]
    }

    pub fn hidden_older(&self) -> usize {
        self.messages.len().saturating_sub(self.window.max(RENDER_WINDOW / 2))
    }
}

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    pub results: Vec<User>,
    pub loading: bool,
    pub searched: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub kind: crate::api::client::MediaKind,
    pub preview: Option<media::Image>,
}

#[derive(Debug, Default)]
pub struct Composer {
    pub text: String,
    pub editing: Option<ChatMessage>,
    pub replying: Option<ChatMessage>,
    pub attachment: Option<Attachment>,
    pub sending: bool,
    /// Drafts per chat id, restored when switching back.
    pub drafts: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub message: ChatMessage,
    pub position: Point,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub until: Instant,
}

// ============================================================================
// Messages
// ============================================================================

#[derive(Debug, Clone)]
pub enum Message {
    WindowOpened(window::Id),
    ScaleFactor(f32),
    BackdropDue(u64),
    BackdropReady(u64, iced::Size, iced::widget::image::Handle),
    Window(window::Id, window::Event),
    Key(Key, keyboard::Modifiers),
    Tick,
    Quit(bool),

    // Session
    Restored(Result<User, ApiError>),
    Auth(ui::auth::Msg),
    LoggedIn(Result<User, ApiError>),
    Logout,
    LoggedOut,

    // Realtime
    Ws(ws::Event),
    ChatsLoaded(Result<Vec<Chat>, ApiError>),
    BlockedLoaded(Result<Vec<User>, ApiError>),

    // Navigation
    OpenChat(String),
    CloseChat,
    MessagesLoaded { chat_id: String, before: Option<String>, result: Result<Vec<ChatMessage>, ApiError> },
    PinsLoaded { chat_id: String, result: Result<Vec<ChatMessage>, ApiError> },
    Scrolled(scrollable::Viewport),
    ListScrolled(scrollable::Viewport),
    WheelMessages(f32),
    WheelList(f32),
    Frame(Instant),
    JumpOffset(f32, f32),
    ScrollToLatest,
    MarkedRead(Result<(), ApiError>),
    JumpTo(String),
    CyclePin,

    // Search
    SearchChanged(String),
    SearchSubmit,
    SearchResults(String, Result<Vec<User>, ApiError>),
    SearchClear,
    SelectUser(User),
    PrivateChatReady(Result<Chat, ApiError>),

    // Composer
    ComposerChanged(String),
    ComposerSubmit,
    Sent { chat_id: String, result: Result<ChatMessage, ApiError> },
    Edited(Result<ChatMessage, ApiError>),
    Attach,
    AttachPicked(Option<PathBuf>),
    AttachmentPreview(PathBuf, Result<media::Image, String>),
    ClearAttachment,
    PasteFromClipboard,
    ClipboardImage(Option<PathBuf>),
    Reply(ChatMessage),
    CancelReply,
    Edit(ChatMessage),
    CancelEdit,

    // Message actions
    ShowContextMenu(ChatMessage),
    HideContextMenu,
    CopyText(String),
    TogglePin(ChatMessage),
    PinChanged(Result<ChatMessage, ApiError>),
    AskDelete(ChatMessage),
    Deleted(Result<ChatMessage, ApiError>),

    // Blocking
    SetBlocked(User, bool),
    BlockChanged { user: User, block: bool, result: Result<(), ApiError> },

    // Media
    MediaVisible(MediaKey),
    MediaLoaded(MediaKey, Result<media::Image, String>),
    OpenImage(String),
    OpenVideo(String),
    FileReady(Result<PathBuf, ApiError>),
    SaveMedia(String),
    SaveMediaTo(String, Option<PathBuf>),
    Saved(Result<PathBuf, ApiError>),

    // Dialogs & misc
    OpenDialog(dialogs::Open),
    Dialog(dialogs::Msg),
    CloseDialog,
    DismissToast,
    Failure(String),
    Update(updater::Msg),

    // Calls
    StartCall(String, String),
    AcceptCall,
    EndCall,
    ToggleMute,
    CallUpdate(String, crate::call::engine::Update),
}

// ============================================================================
// Boot
// ============================================================================

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let dirs = Dirs::resolve();
        let settings = Settings::load(&dirs);
        let store = SessionStore::new(&dirs);
        let stored = store.load();
        let server = stored.as_ref().map(|session| session.server.clone()).unwrap_or_else(|| settings.server.clone());
        let client = Client::new(&server, Some(store.clone()));
        let updater = Updater::new(&dirs);
        let window_size = iced::Size::new(settings.window_width, settings.window_height);

        let mut tasks = vec![updater::startup_task(&settings)];
        let screen = match stored {
            Some(session) => {
                client.restore(session);
                let restore = client.clone();
                tasks.push(Task::perform(async move { restore.refresh().await?; restore.me().await }, Message::Restored));
                Screen::Loading
            }
            None => Screen::Auth(ui::auth::State::new(&settings)),
        };

        let mut app = Self {
            dirs,
            settings,
            store,
            client,
            screen,
            window: None,
            focused: true,
            updater,
            window_size,
            scale: 1.0,
            backdrop: None,
            backdrop_generation: 0,
        };
        tasks.push(app.schedule_backdrop());
        (app, Task::batch(tasks))
    }

    /// Recompose the backdrop once the window size has settled (resizes arrive per frame).
    fn schedule_backdrop(&mut self) -> Task<Message> {
        self.backdrop_generation += 1;
        let generation = self.backdrop_generation;
        let delay = if self.backdrop.is_none() { 0 } else { 120 };
        Task::perform(async move { tokio::time::sleep(std::time::Duration::from_millis(delay)).await; generation }, Message::BackdropDue)
    }

    pub fn title(&self) -> String {
        match &self.screen {
            Screen::Main(main) => {
                let unread: u32 = main.chats.iter().filter(|chat| chat.is_listed()).map(|chat| chat.unread_count).sum();
                if unread > 0 { format!("Messenger ({unread})") } else { "Messenger".to_owned() }
            }
            _ => "Messenger".to_owned(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            window::open_events().map(Message::WindowOpened),
            window::events().map(|(id, event)| Message::Window(id, event)),
            // Raw key presses (including ones a focused text field consumed), so shortcuts
            // like Esc or Ctrl+K also work while typing.
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => Some(Message::Key(key, modifiers)),
                _ => None,
            }),
        ];
        if let Screen::Main(main) = &self.screen {
            subscriptions.push(
                ws::subscription(ws::Config { client: self.client.clone(), generation: main.ws_generation }).map(Message::Ws),
            );
            // A coarse tick only while something time-based is pending keeps idle CPU at zero.
            // Per-frame updates only while something is animating.
            let highlight_fading = main.highlight.as_ref().is_some_and(|(_, at)| at.elapsed() < crate::ui::chat::HIGHLIGHT);
            if main.msg_scroll.is_animating() || main.list_scroll.is_animating() || highlight_fading || main.highlight.is_some() {
                subscriptions.push(window::frames().map(Message::Frame));
            }
            if let Some(call) = &main.call {
                subscriptions.push(call.subscription().map(|(id, update)| Message::CallUpdate(id, update)));
            }
            let needs_tick = main.call.as_ref().is_some_and(|call| call.started_at.is_some())
                || !main.typing.is_empty()
                || main.typing_sent.is_some()
                || main.toast.is_some();
            if needs_tick {
                subscriptions.push(iced::time::every(Duration::from_millis(500)).map(|_| Message::Tick));
            } else if main.active_peer().is_some_and(|peer| !main.online.contains(&peer.id) && main.last_seen.contains_key(&peer.id)) {
                // Keeps a relative "Last seen 5 minutes ago" current.
                subscriptions.push(iced::time::every(Duration::from_secs(30)).map(|_| Message::Tick));
            }
        }
        subscriptions.push(iced::event::listen_with(|event, _, _| {
            if let iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) = event {
                *POINTER.lock().unwrap() = position;
            }
            None
        }));
        subscriptions.push(self.updater.subscription().map(Message::Update));
        Subscription::batch(subscriptions)
    }

    fn save_settings(&self) {
        self.settings.save(&self.dirs);
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        if let Screen::Main(main) = &mut self.screen {
            main.toast = Some(Toast { text: text.into(), error, until: Instant::now() + Duration::from_secs(if error { 8 } else { 4 }) });
        }
    }

    // ========================================================================
    // Update
    // ========================================================================

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowOpened(id) => {
                self.window = Some(id);
                let scale = window::scale_factor(id).map(Message::ScaleFactor);
                if self.settings.window_maximized {
                    return Task::batch([window::maximize(id, true), scale]);
                }
                scale
            }
            Message::ScaleFactor(scale) => {
                self.scale = scale;
                self.schedule_backdrop()
            }
            Message::BackdropDue(generation) => {
                if generation != self.backdrop_generation {
                    return Task::none();
                }
                let (size, scale) = (self.window_size, self.scale);
                Task::perform(
                    async move { tokio::task::spawn_blocking(move || ui::backdrop::compose(size, scale)).await.ok() },
                    move |handle| match handle {
                        Some(handle) => Message::BackdropReady(generation, size, handle),
                        None => Message::BackdropDue(u64::MAX),
                    },
                )
            }
            Message::BackdropReady(generation, size, handle) => {
                if generation == self.backdrop_generation || self.backdrop.is_none() {
                    self.backdrop = Some((size, handle));
                }
                Task::none()
            }
            Message::Window(id, event) => self.window_event(id, event),
            Message::Key(key, modifiers) => self.key(key, modifiers),
            Message::Tick => self.tick(),
            Message::Quit(maximized) => {
                self.settings.window_maximized = maximized;
                self.save_settings();
                self.updater.on_exit();
                iced::exit()
            }

            Message::Restored(result) => match result {
                Ok(me) => self.enter_main(me),
                Err(error) => {
                    // A network failure keeps the remembered session for the next start.
                    let mut auth = ui::auth::State::new(&self.settings);
                    auth.error = Some(if error.is_network() {
                        format!("{} You can retry signing in.", error.message)
                    } else {
                        "Your session has ended. Please sign in again.".to_owned()
                    });
                    auth.can_retry_restore = error.is_network() && self.client.has_refresh_session();
                    self.screen = Screen::Auth(auth);
                    Task::none()
                }
            },
            Message::Auth(msg) => self.auth(msg),
            Message::LoggedIn(result) => {
                let Screen::Auth(auth) = &mut self.screen else { return Task::none() };
                auth.busy = false;
                match result {
                    Ok(me) => self.enter_main(me),
                    Err(error) => {
                        auth.set_login_error(error.message);
                        Task::none()
                    }
                }
            }
            Message::Logout => {
                let client = self.client.clone();
                Task::perform(async move { client.logout().await }, |_| Message::LoggedOut)
            }
            Message::LoggedOut => {
                self.screen = Screen::Auth(ui::auth::State::new(&self.settings));
                Task::none()
            }
            Message::Update(msg) => {
                let task = self.updater.update(msg, &mut self.settings, &self.dirs);
                task.map(Message::Update)
            }
            other => match &mut self.screen {
                Screen::Main(_) => self.main_update(other),
                _ => Task::none(),
            },
        }
    }

    fn window_event(&mut self, _id: window::Id, event: window::Event) -> Task<Message> {
        match event {
            window::Event::Focused => {
                self.focused = true;
                return self.mark_active_read();
            }
            window::Event::Unfocused => self.focused = false,
            window::Event::Resized(size) => {
                self.window_size = size;
                if size.width > 200.0 && size.height > 200.0 {
                    self.settings.window_width = size.width;
                    self.settings.window_height = size.height;
                }
                return self.schedule_backdrop();
            }
            window::Event::Rescaled(scale) => {
                self.scale = scale;
                return self.schedule_backdrop();
            }
            window::Event::CloseRequested => {
                return match self.window {
                    Some(id) => window::is_maximized(id).map(Message::Quit),
                    None => Task::done(Message::Quit(false)),
                };
            }
            _ => {}
        }
        Task::none()
    }

    fn enter_main(&mut self, me: User) -> Task<Message> {
        self.settings.last_username = me.username.clone();
        self.settings.server = self.client.server().to_owned();
        self.save_settings();
        let main = Main {
            me,
            chats: Vec::new(),
            online: HashSet::new(),
            last_seen: HashMap::new(),
            blocked: None,
            block_busy: None,
            typing: HashMap::new(),
            ws_status: WsStatus::Connecting,
            connection: None,
            ws_generation: 0,
            active: None,
            threads: HashMap::new(),
            thread_order: VecDeque::new(),
            search: Search::default(),
            composer: Composer::default(),
            context_menu: None,
            dialog: None,
            media: MediaCache::default(),
            toast: None,
            typing_sent: None,
            last_input: None,
            at_bottom: true,
            viewport_height: 600.0,
            highlight: None,
            msg_scroll: Default::default(),
            list_scroll: Default::default(),
            pin_cursor: 0,
            pending_jump: None,
            call: None,
        };
        self.screen = Screen::Main(Box::new(main));
        Task::batch([self.load_chats(), self.load_blocked()])
    }

    fn load_blocked(&self) -> Task<Message> {
        let client = self.client.clone();
        Task::perform(async move { client.blocked_users().await }, Message::BlockedLoaded)
    }

    fn load_chats(&self) -> Task<Message> {
        let client = self.client.clone();
        Task::perform(async move { client.chats().await }, Message::ChatsLoaded)
    }

    fn auth(&mut self, msg: ui::auth::Msg) -> Task<Message> {
        let Screen::Auth(auth) = &mut self.screen else { return Task::none() };
        match auth.update(msg) {
            ui::auth::Action::None => Task::none(),
            ui::auth::Action::Login { server, username, password, remember } => {
                if normalize(&server) != self.client.server() {
                    self.client = Client::new(&server, Some(self.store.clone()));
                }
                self.settings.server = self.client.server().to_owned();
                self.settings.last_username = username.clone();
                self.save_settings();
                let client = self.client.clone();
                Task::perform(
                    async move {
                        client.login(&username, &password, remember).await?;
                        client.me().await
                    },
                    Message::LoggedIn,
                )
            }
            ui::auth::Action::RetryRestore => {
                let client = self.client.clone();
                self.screen = Screen::Loading;
                Task::perform(async move { client.refresh().await?; client.me().await }, Message::Restored)
            }
            ui::auth::Action::Request { server, request } => {
                if normalize(&server) != self.client.server() {
                    self.client = Client::new(&server, Some(self.store.clone()));
                }
                let client = self.client.clone();
                Task::perform(async move { request.run(&client).await }, |result| Message::Auth(ui::auth::Msg::Done(result)))
            }
        }
    }

    fn key(&mut self, key: Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        match key.as_ref() {
            Key::Named(Named::Escape) => {
                if main.context_menu.take().is_some() {
                    return Task::none();
                }
                if main.dialog.is_some() {
                    return self.main_update(Message::CloseDialog);
                }
                if main.composer.editing.is_some() {
                    return self.main_update(Message::CancelEdit);
                }
                if main.composer.replying.is_some() {
                    return self.main_update(Message::CancelReply);
                }
                if main.composer.attachment.is_some() {
                    return self.main_update(Message::ClearAttachment);
                }
                if !main.search.query.is_empty() {
                    return self.main_update(Message::SearchClear);
                }
                if main.active.is_some() {
                    return self.main_update(Message::CloseChat);
                }
                Task::none()
            }
            Key::Character("k") | Key::Character("f") if modifiers.command() && main.dialog.is_none() => {
                Task::batch([operation::focus(search_input()), operation::select_all(search_input())])
            }
            Key::Character("v") if modifiers.command() && main.active.is_some() && main.dialog.is_none() && main.composer.editing.is_none() => {
                self.main_update(Message::PasteFromClipboard)
            }
            Key::Character(",") if modifiers.command() => self.main_update(Message::OpenDialog(dialogs::Open::Settings)),
            Key::Named(Named::Tab) if modifiers.control() && main.dialog.is_none() => {
                let visible: Vec<&Chat> = main.chats.iter().filter(|chat| chat.is_listed() || Some(&chat.id) == main.active.as_ref()).collect();
                if visible.is_empty() {
                    return Task::none();
                }
                let current = main.active.as_ref().and_then(|id| visible.iter().position(|chat| &chat.id == id));
                let next = match (current, modifiers.shift()) {
                    (None, _) => 0,
                    (Some(index), false) => (index + 1) % visible.len(),
                    (Some(index), true) => (index + visible.len() - 1) % visible.len(),
                };
                let id = visible[next].id.clone();
                self.main_update(Message::OpenChat(id))
            }
            Key::Named(Named::ArrowUp)
                if main.composer.text.is_empty() && main.composer.editing.is_none() && main.dialog.is_none() && main.active.is_some() =>
            {
                // Like Telegram: ↑ in an empty composer edits your last message.
                let me = main.me.id.clone();
                let last = main
                    .active
                    .as_ref()
                    .and_then(|id| main.threads.get(id))
                    .and_then(|thread| thread.messages.iter().rev().find(|m| m.sender_id == me && !m.is_deleted() && !m.is_call && !m.content.is_empty()).cloned());
                match last {
                    Some(message) => self.main_update(Message::Edit(message)),
                    None => Task::none(),
                }
            }
            _ => Task::none(),
        }
    }

    fn tick(&mut self) -> Task<Message> {
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let now = Instant::now();
        for users in main.typing.values_mut() {
            users.retain(|_, at| now.duration_since(*at) < TYPING_EXPIRY);
        }
        main.typing.retain(|_, users| !users.is_empty());
        if main.toast.as_ref().is_some_and(|toast| toast.until <= now) {
            main.toast = None;
        }
        if let Some((chat_id, _)) = &main.typing_sent
            && main.last_input.is_none_or(|at| now.duration_since(at) > TYPING_IDLE_STOP)
        {
            if let Some(connection) = &main.connection {
                connection.typing(chat_id, false);
            }
            main.typing_sent = None;
        }
        Task::none()
    }

    fn mark_active_read(&mut self) -> Task<Message> {
        let focused = self.focused;
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let Some(chat_id) = main.active.clone() else { return Task::none() };
        if !focused || main.dialog.as_ref().is_some_and(Dialog::covers_chat) {
            return Task::none();
        }
        let Some(chat) = main.chats.iter_mut().find(|chat| chat.id == chat_id) else { return Task::none() };
        if chat.unread_count == 0 {
            return Task::none();
        }
        chat.unread_count = 0;
        let client = self.client.clone();
        Task::perform(async move { client.mark_read(&chat_id).await }, Message::MarkedRead)
    }

    // ========================================================================
    // Main screen
    // ========================================================================

    fn main_update(&mut self, message: Message) -> Task<Message> {
        let client = self.client.clone();
        let focused = self.focused;
        let notifications = self.settings.notifications;
        let previews = self.settings.notification_previews;
        let cursor = pointer_position();
        let Screen::Main(main) = &mut self.screen else { return Task::none() };

        match message {
            Message::Ws(event) => return self.realtime(event),
            Message::ChatsLoaded(result) => match result {
                Ok(chats) => {
                    main.chats = chats;
                    sort_chats(&mut main.chats);
                    // Keep the open chat's header fresh (e.g. after a group rename).
                    if let Some(active) = &main.active
                        && !main.chats.iter().any(|chat| &chat.id == active)
                    {
                        main.active = None;
                    }
                }
                Err(error) => return self.fail(error),
            },
            // On failure the list stays empty rather than blocking the UI, as in the web app.
            Message::BlockedLoaded(result) => main.blocked = Some(result.unwrap_or_default()),
            Message::SetBlocked(user, block) => {
                if main.block_busy.is_some() {
                    return Task::none();
                }
                main.block_busy = Some(user.id.clone());
                return Task::perform(
                    {
                        let user_id = user.id.clone();
                        async move { if block { client.block_user(&user_id).await.map(drop) } else { client.unblock_user(&user_id).await } }
                    },
                    move |result| Message::BlockChanged { user: user.clone(), block, result },
                );
            }
            Message::BlockChanged { user, block, result } => {
                main.block_busy = None;
                if let Some(dialogs::Dialog::Peer { confirming, .. }) = &mut main.dialog {
                    *confirming = false;
                }
                match result {
                    Ok(()) => {
                        let blocked = main.blocked.get_or_insert_with(Vec::new);
                        blocked.retain(|item| item.id != user.id);
                        if block {
                            blocked.insert(0, user);
                        }
                    }
                    Err(error) => return self.fail(error),
                }
            }
            Message::OpenChat(chat_id) => return self.open_chat(chat_id),
            Message::CloseChat => {
                self.stash_draft();
                if let Screen::Main(main) = &mut self.screen {
                    main.active = None;
                    main.context_menu = None;
                }
            }
            Message::MessagesLoaded { chat_id, before, result } => {
                let is_active = main.active.as_deref() == Some(chat_id.as_str());
                let mut tasks = Vec::new();
                if let Some(thread) = main.threads.get_mut(&chat_id) {
                    match result {
                        Ok(page) => {
                            let full = page.len() >= 50;
                            if before.is_some() {
                                thread.loading_older = false;
                                thread.has_older = full;
                                let added = page.len();
                                let mut merged = page;
                                merged.retain(|m| !thread.messages.iter().any(|existing| existing.id == m.id));
                                merged.append(&mut thread.messages);
                                thread.messages = merged;
                                thread.window += added;
                            } else {
                                thread.loading = false;
                                // Reconcile the latest page with what was cached (edits/deletes while away).
                                if thread.loaded && thread.messages.len() > page.len() && page.first().is_some_and(|first| thread.messages.iter().any(|m| m.id == first.id)) {
                                    let keep_until = thread.messages.iter().position(|m| m.id == page[0].id).unwrap_or(0);
                                    thread.messages.truncate(keep_until);
                                    thread.messages.extend(page);
                                } else {
                                    thread.messages = page;
                                    thread.has_older = full;
                                    thread.window = RENDER_WINDOW;
                                }
                                thread.loaded = true;
                                thread.error = None;
                            }
                            if let Some(target) = main.pending_jump.clone()
                                && is_active
                            {
                                tasks.push(Task::done(Message::JumpTo(target)));
                            }
                        }
                        Err(error) => {
                            thread.loading = false;
                            thread.loading_older = false;
                            if error.is_unauthorized() {
                                return self.session_expired(error.message);
                            }
                            thread.error = Some(error.message.clone());
                            main.pending_jump = None;
                        }
                    }
                }
                if is_active && before.is_none() {
                    tasks.push(scroll_to_bottom());
                    tasks.push(self.mark_active_read());
                }
                return Task::batch(tasks);
            }
            Message::PinsLoaded { chat_id, result } => {
                if let (Some(thread), Ok(pins)) = (main.threads.get_mut(&chat_id), result) {
                    thread.pins = pins;
                    main.pin_cursor = 0;
                }
            }
            Message::ListScrolled(viewport) => main.list_scroll.observe(&viewport),
            Message::WheelMessages(notches) => {
                // Bottom-anchored: scrolling up moves away from the newest message.
                main.msg_scroll.wheel(notches, true);
            }
            Message::WheelList(notches) => main.list_scroll.wheel(notches, false),
            Message::JumpOffset(offset, max) => {
                main.msg_scroll.max = main.msg_scroll.max.max(max);
                main.msg_scroll.animate_to(offset);
            }
            Message::Frame(now) => {
                if main.highlight.as_ref().is_some_and(|(_, at)| at.elapsed() >= crate::ui::chat::HIGHLIGHT) {
                    main.highlight = None;
                }
                let mut tasks = Vec::new();
                if let Some(offset) = main.msg_scroll.step(now) {
                    tasks.push(operation::scroll_to(messages_scroll(), offset));
                }
                if let Some(offset) = main.list_scroll.step(now) {
                    tasks.push(operation::scroll_to(crate::ui::sidebar::chat_list_scroll(), offset));
                }
                return Task::batch(tasks);
            }
            Message::Scrolled(viewport) => {
                main.msg_scroll.observe(&viewport);
                let offset = viewport.absolute_offset().y; // distance from the bottom (anchored)
                let content = viewport.content_bounds().height;
                let height = viewport.bounds().height;
                main.viewport_height = height;
                let was_bottom = main.at_bottom;
                main.at_bottom = offset < 48.0;
                let near_top = content - height - offset < 400.0;
                let mut tasks = Vec::new();
                if main.at_bottom && !was_bottom {
                    if let Some(thread) = main.active.as_ref().and_then(|id| main.threads.get_mut(id)) {
                        // Back at the newest message: render only the default window again.
                        thread.window = RENDER_WINDOW;
                    }
                    tasks.push(self.mark_active_read());
                }
                if near_top {
                    tasks.push(self.load_older());
                }
                return Task::batch(tasks);
            }
            Message::ScrollToLatest => {
                if let Some(thread) = main.active.as_ref().and_then(|id| main.threads.get_mut(id)) {
                    thread.window = RENDER_WINDOW;
                }
                main.at_bottom = true;
                main.msg_scroll.animate_to(0.0);
                return self.mark_active_read();
            }
            Message::MarkedRead(result) => {
                if let Err(error) = result {
                    log::info!("mark read failed: {error}");
                }
            }
            Message::JumpTo(message_id) => return self.jump_to(message_id),
            Message::CyclePin => {
                let Some(thread) = main.active.as_ref().and_then(|id| main.threads.get(id)) else { return Task::none() };
                if thread.pins.is_empty() {
                    return Task::none();
                }
                let index = main.pin_cursor % thread.pins.len();
                let target = thread.pins[index].id.clone();
                main.pin_cursor = (index + 1) % thread.pins.len();
                return self.jump_to(target);
            }

            Message::SearchChanged(query) => {
                main.search.query = query;
                if main.search.query.trim().is_empty() {
                    main.search.results.clear();
                    main.search.searched = None;
                    main.search.loading = false;
                    return Task::none();
                }
                // Search as you type, debounced by the request itself (results for stale queries are dropped).
                let query = main.search.query.trim().to_owned();
                main.search.loading = true;
                return Task::perform(
                    async move {
                        tokio::time::sleep(Duration::from_millis(220)).await;
                        let result = client.search_users(&query).await;
                        (query, result)
                    },
                    |(query, result)| Message::SearchResults(query, result),
                );
            }
            Message::SearchSubmit => {
                if let Some(first) = main.search.results.first().cloned() {
                    return self.main_update(Message::SelectUser(first));
                }
                let query = main.search.query.trim().to_owned();
                if query.is_empty() {
                    return Task::none();
                }
                main.search.loading = true;
                return Task::perform(async move { (query.clone(), client.search_users(&query).await) }, |(query, result)| Message::SearchResults(query, result));
            }
            Message::SearchResults(query, result) => {
                if query != main.search.query.trim() {
                    return Task::none();
                }
                main.search.loading = false;
                main.search.searched = Some(query);
                match result {
                    Ok(users) => main.search.results = users,
                    Err(error) => return self.fail(error),
                }
            }
            Message::SearchClear => {
                main.search = Search::default();
            }
            Message::SelectUser(user) => {
                main.search = Search::default();
                if let Some(chat) = main.chats.iter().find(|chat| !chat.is_group() && chat.peer.as_ref().is_some_and(|peer| peer.id == user.id)) {
                    let id = chat.id.clone();
                    return self.open_chat(id);
                }
                return Task::perform(async move { client.create_private_chat(&user.id).await }, Message::PrivateChatReady);
            }
            Message::PrivateChatReady(result) => match result {
                Ok(chat) => {
                    let id = chat.id.clone();
                    upsert_chat(&mut main.chats, chat);
                    return self.open_chat(id);
                }
                Err(error) => return self.fail(error),
            },

            Message::ComposerChanged(text) => {
                main.composer.text = text.chars().take(4000).collect();
                main.last_input = Some(Instant::now());
                if main.composer.editing.is_none()
                    && !main.composer.text.trim().is_empty()
                    && let (Some(chat_id), Some(connection)) = (&main.active, &main.connection)
                {
                    let due = match &main.typing_sent {
                        Some((sent_chat, at)) => sent_chat != chat_id || at.elapsed() >= TYPING_SEND_INTERVAL,
                        None => true,
                    };
                    if due {
                        connection.typing(chat_id, true);
                        main.typing_sent = Some((chat_id.clone(), Instant::now()));
                    }
                }
            }
            Message::ComposerSubmit => return self.submit(),
            Message::Sent { chat_id, result } => {
                main.composer.sending = false;
                match result {
                    Ok(message) => {
                        main.composer.drafts.remove("__failed");
                        apply_message(main, message, false);
                        if main.active.as_deref() == Some(chat_id.as_str()) {
                            main.at_bottom = true;
                            return scroll_to_bottom();
                        }
                    }
                    Err(error) => {
                        if main.composer.text.is_empty()
                            && let Some(draft) = main.composer.drafts.remove("__failed")
                        {
                            main.composer.text = draft;
                        }
                        return self.fail(error);
                    }
                }
            }
            Message::Edited(result) => {
                main.composer.sending = false;
                match result {
                    Ok(message) => apply_message(main, message, false),
                    Err(error) => return self.fail(error),
                }
            }
            Message::Attach => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Attach photo or video")
                            .add_filter("Photos and videos", &["jpg", "jpeg", "png", "webp", "mp4", "webm"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::AttachPicked,
                );
            }
            Message::AttachPicked(path) => {
                let Some(path) = path else { return Task::none() };
                return self.set_attachment(path);
            }
            Message::AttachmentPreview(path, result) => {
                if let (Some(attachment), Ok(image)) = (&mut main.composer.attachment, result)
                    && attachment.path == path
                {
                    attachment.preview = Some(image);
                }
            }
            Message::ClearAttachment => main.composer.attachment = None,
            Message::PasteFromClipboard => {
                let temp = self.dirs.cache.join("clipboard");
                return Task::perform(async move { tokio::task::spawn_blocking(move || clipboard_image(&temp)).await.ok().flatten() }, Message::ClipboardImage);
            }
            Message::ClipboardImage(path) => {
                if let Some(path) = path {
                    return self.set_attachment(path);
                }
            }
            Message::Reply(message) => {
                main.context_menu = None;
                main.composer.editing = None;
                main.composer.replying = Some(message);
                return operation::focus(composer_input());
            }
            Message::CancelReply => main.composer.replying = None,
            Message::Edit(message) => {
                main.context_menu = None;
                main.composer.replying = None;
                main.composer.attachment = None;
                if main.composer.editing.is_none() && !main.composer.text.is_empty() {
                    main.composer.drafts.insert("__before_edit".into(), main.composer.text.clone());
                }
                main.composer.text = message.content.clone();
                main.composer.editing = Some(message);
                return Task::batch([operation::focus(composer_input()), operation::move_cursor_to_end(composer_input())]);
            }
            Message::CancelEdit => {
                main.composer.editing = None;
                main.composer.text = main.composer.drafts.remove("__before_edit").unwrap_or_default();
            }

            Message::ShowContextMenu(message) => {
                main.context_menu = Some(ContextMenu { message, position: cursor });
            }
            Message::HideContextMenu => main.context_menu = None,
            Message::CopyText(text) => {
                main.context_menu = None;
                return iced::clipboard::write(text);
            }
            Message::TogglePin(message) => {
                main.context_menu = None;
                let pinned = message.pinned_at.is_none();
                return Task::perform(async move { client.set_pinned(&message.chat_id, &message.id, pinned).await }, Message::PinChanged);
            }
            Message::PinChanged(result) => match result {
                Ok(message) => apply_message(main, message, false),
                Err(error) => return self.fail(error),
            },
            Message::AskDelete(message) => {
                main.context_menu = None;
                main.dialog = Some(Dialog::ConfirmDelete { message, busy: false });
            }
            Message::Deleted(result) => {
                main.dialog = None;
                match result {
                    Ok(message) => apply_message(main, message, false),
                    Err(error) => return self.fail(error),
                }
            }

            Message::MediaVisible(key) => {
                if main.media.begin(&key) {
                    return Task::perform(media::load(client, key.clone()), move |result| Message::MediaLoaded(key.clone(), result));
                }
            }
            Message::MediaLoaded(key, result) => main.media.finish(key, result),
            Message::OpenImage(url) => {
                let urls = main.active.as_ref().and_then(|id| main.threads.get(id)).map(|thread| {
                    thread.messages.iter().filter(|m| !m.is_deleted()).filter_map(|m| m.image_url.as_deref()).map(|path| client.media_url(path)).collect::<Vec<_>>()
                });
                let urls = urls.unwrap_or_else(|| vec![url.clone()]);
                let index = urls.iter().position(|item| *item == url).unwrap_or(0);
                main.dialog = Some(Dialog::Viewer(dialogs::Viewer { urls, index }));
                let key = MediaKey::new(url, media::Size::Full);
                return self.main_update(Message::MediaVisible(key));
            }
            Message::OpenVideo(url) => {
                main.toast = Some(Toast { text: "Opening video…".into(), error: false, until: Instant::now() + Duration::from_secs(4) });
                let target = media_cache_path(&self.dirs, &url);
                return Task::perform(
                    async move {
                        if !target.exists() {
                            if let Some(parent) = target.parent() {
                                tokio::fs::create_dir_all(parent).await.map_err(crate::api::client::io_error)?;
                            }
                            client.download_to(&url, &target, |_, _| {}).await?;
                        }
                        Ok(target)
                    },
                    Message::FileReady,
                );
            }
            Message::FileReady(result) => match result {
                Ok(path) => {
                    main.toast = None;
                    platform::open(path.to_string_lossy().into_owned());
                }
                Err(error) => return self.fail(error),
            },
            Message::SaveMedia(url) => {
                main.context_menu = None;
                let name = url.rsplit('/').next().unwrap_or("download").to_owned();
                let directory = self.dirs.downloads_fallback();
                return Task::perform(
                    async move {
                        let picked = rfd::AsyncFileDialog::new().set_title("Save as").set_directory(&directory).set_file_name(&name).save_file().await;
                        (url, picked.map(|file| file.path().to_path_buf()))
                    },
                    |(url, path)| Message::SaveMediaTo(url, path),
                );
            }
            Message::SaveMediaTo(url, path) => {
                let Some(path) = path else { return Task::none() };
                return Task::perform(
                    async move {
                        client.download_to(&url, &path, |_, _| {}).await?;
                        Ok(path)
                    },
                    Message::Saved,
                );
            }
            Message::Saved(result) => match result {
                Ok(path) => {
                    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    main.toast = Some(Toast { text: format!("Saved {name}"), error: false, until: Instant::now() + Duration::from_secs(4) });
                }
                Err(error) => return self.fail(error),
            },

            Message::OpenDialog(open) => {
                main.context_menu = None;
                let (dialog, task) = dialogs::open(open, main, &client, &self.settings, &self.updater);
                main.dialog = Some(dialog);
                return task;
            }
            Message::Dialog(msg) => {
                let Some(dialog) = main.dialog.as_mut() else { return Task::none() };
                let context = dialogs::Context { client: &client, me: &main.me, chats: &main.chats, settings: &mut self.settings, dirs: &self.dirs };
                let outcome = dialog.update(msg, context);
                return self.dialog_outcome(outcome);
            }
            Message::CloseDialog => {
                main.dialog = None;
                return self.mark_active_read();
            }
            Message::StartCall(..) | Message::AcceptCall | Message::EndCall | Message::ToggleMute | Message::CallUpdate(..) => {
                return self.call_message(message);
            }
            Message::DismissToast => main.toast = None,
            Message::Failure(text) => self.toast(text, true),
            _ => {}
        }
        let _ = (focused, notifications, previews);
        Task::none()
    }

    fn dialog_outcome(&mut self, outcome: dialogs::Outcome) -> Task<Message> {
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        match outcome {
            dialogs::Outcome::None => Task::none(),
            dialogs::Outcome::Task(task) => task,
            dialogs::Outcome::ProfileUpdated(user) => {
                main.me = user;
                Task::none()
            }
            dialogs::Outcome::ChatUpdated(chat) => {
                upsert_chat(&mut main.chats, chat);
                Task::none()
            }
            dialogs::Outcome::OpenChat(chat) => {
                let id = chat.id.clone();
                upsert_chat(&mut main.chats, chat);
                main.dialog = None;
                self.open_chat(id)
            }
            dialogs::Outcome::Jump(message_id) => {
                main.dialog = None;
                self.jump_to(message_id)
            }
            dialogs::Outcome::SignedOut(reason) => self.session_expired(reason),
            dialogs::Outcome::ServerChanged => {
                // Signing out is required to switch servers.
                self.main_update(Message::CloseDialog).chain(Task::done(Message::Logout))
            }
        }
    }

    fn fail(&mut self, error: ApiError) -> Task<Message> {
        if error.is_unauthorized() {
            return self.session_expired(error.message);
        }
        self.toast(error.message, true);
        Task::none()
    }

    fn session_expired(&mut self, reason: String) -> Task<Message> {
        let mut auth = ui::auth::State::new(&self.settings);
        auth.error = Some(if reason.is_empty() { "Your session has ended. Please sign in again.".into() } else { reason });
        self.screen = Screen::Auth(auth);
        Task::none()
    }

    fn stash_draft(&mut self) {
        let Screen::Main(main) = &mut self.screen else { return };
        let Some(active) = main.active.clone() else { return };
        if main.composer.editing.is_some() {
            main.composer.editing = None;
            main.composer.text = main.composer.drafts.remove("__before_edit").unwrap_or_default();
        }
        let text = std::mem::take(&mut main.composer.text);
        if text.is_empty() {
            main.composer.drafts.remove(&active);
        } else {
            main.composer.drafts.insert(active, text);
        }
        main.composer.replying = None;
        main.composer.attachment = None;
        if let (Some((chat_id, _)), Some(connection)) = (main.typing_sent.take(), &main.connection) {
            connection.typing(&chat_id, false);
        }
    }

    fn open_chat(&mut self, chat_id: String) -> Task<Message> {
        if let Screen::Main(main) = &self.screen
            && main.active.as_deref() == Some(chat_id.as_str())
        {
            return Task::batch([operation::focus(composer_input()), self.mark_active_read()]);
        }
        self.stash_draft();
        let client = self.client.clone();
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        main.active = Some(chat_id.clone());
        main.context_menu = None;
        main.at_bottom = true;
        main.msg_scroll.reset();
        main.pin_cursor = 0;
        main.pending_jump = None;
        main.composer.text = main.composer.drafts.remove(&chat_id).unwrap_or_default();

        // LRU of cached threads for instant switching.
        main.thread_order.retain(|id| id != &chat_id);
        main.thread_order.push_back(chat_id.clone());
        while main.thread_order.len() > THREAD_CACHE {
            if let Some(old) = main.thread_order.pop_front() {
                main.threads.remove(&old);
            }
        }
        let thread = main.threads.entry(chat_id.clone()).or_insert_with(|| Thread { has_older: true, window: RENDER_WINDOW, ..Thread::default() });
        thread.window = RENDER_WINDOW;
        thread.loading = true;

        let messages_client = client.clone();
        let messages_chat = chat_id.clone();
        let pins_chat = chat_id.clone();
        let mut tasks = vec![
            Task::perform(async move { messages_client.messages(&messages_chat, None).await }, move |result| Message::MessagesLoaded {
                chat_id: chat_id.clone(),
                before: None,
                result,
            }),
            Task::perform(
                {
                    let chat_id = pins_chat.clone();
                    async move { client.pinned_messages(&chat_id).await }
                },
                move |result| Message::PinsLoaded { chat_id: pins_chat.clone(), result },
            ),
            scroll_to_bottom(),
            operation::focus(composer_input()),
        ];
        if let (Some(connection), Some(chat)) = (&main.connection, main.chats.iter().find(|chat| Some(&chat.id) == main.active.as_ref()))
            && let Some(peer) = &chat.peer
        {
            connection.request_presence(&peer.id);
        }
        tasks.push(self.mark_active_read());
        Task::batch(tasks)
    }

    fn load_older(&mut self) -> Task<Message> {
        let client = self.client.clone();
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let Some(chat_id) = main.active.clone() else { return Task::none() };
        let Some(thread) = main.threads.get_mut(&chat_id) else { return Task::none() };
        // First reveal already-loaded messages, then fetch from the server.
        if thread.hidden_older() > 0 {
            thread.window = (thread.window + RENDER_WINDOW / 2).min(thread.messages.len());
            return Task::none();
        }
        if thread.loading || thread.loading_older || !thread.has_older || !thread.loaded {
            return Task::none();
        }
        let Some(oldest) = thread.messages.first().map(|m| m.id.clone()) else { return Task::none() };
        thread.loading_older = true;
        let before = Some(oldest.clone());
        Task::perform(async move { client.messages(&chat_id.clone(), Some(&oldest)).await.map(|page| (chat_id, page)) }, move |result| match result {
            Ok((chat_id, page)) => Message::MessagesLoaded { chat_id, before: before.clone(), result: Ok(page) },
            Err(error) => Message::Failure(error.message),
        })
    }

    /// Scrolls to a message, loading older pages first if it is not loaded yet.
    fn jump_to(&mut self, message_id: String) -> Task<Message> {
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let Some(chat_id) = main.active.clone() else { return Task::none() };
        let Some(thread) = main.threads.get_mut(&chat_id) else { return Task::none() };
        match thread.messages.iter().position(|m| m.id == message_id) {
            Some(index) => {
                main.pending_jump = None;
                let needed = thread.messages.len() - index + 10;
                if needed > thread.window {
                    thread.window = needed;
                }
                main.highlight = Some((message_id.clone(), Instant::now()));
                ui::chat::scroll_to_message(message_id).map(|(offset, max)| Message::JumpOffset(offset, max))
            }
            None if thread.has_older && thread.loaded => {
                main.pending_jump = Some(message_id);
                thread.window = thread.messages.len();
                if thread.loading_older {
                    return Task::none();
                }
                self.load_older()
            }
            None => {
                main.pending_jump = None;
                self.toast("The original message is no longer available.", false);
                Task::none()
            }
        }
    }

    fn set_attachment(&mut self, path: PathBuf) -> Task<Message> {
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let Some(kind) = crate::api::client::MediaKind::of(&path) else {
            self.toast("Only JPG, PNG, WEBP images and MP4, WEBM videos can be sent.", true);
            return Task::none();
        };
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        main.composer.editing = None;
        main.composer.attachment = Some(Attachment { path: path.clone(), name, size, kind, preview: None });
        let mut tasks = vec![operation::focus(composer_input())];
        if kind == crate::api::client::MediaKind::Image {
            let preview_path = path.clone();
            tasks.push(Task::perform(media::load_file(preview_path, media::Size::Avatar), move |result| Message::AttachmentPreview(path.clone(), result)));
        }
        Task::batch(tasks)
    }

    fn submit(&mut self) -> Task<Message> {
        let client = self.client.clone();
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let Some(chat_id) = main.active.clone() else { return Task::none() };
        if main.composer.sending {
            return Task::none();
        }
        let text = main.composer.text.trim().to_owned();

        if let Some(editing) = main.composer.editing.clone() {
            if text.is_empty() {
                return Task::none();
            }
            main.composer.editing = None;
            main.composer.text = main.composer.drafts.remove("__before_edit").unwrap_or_default();
            if text == editing.content {
                return Task::none();
            }
            main.composer.sending = true;
            return Task::perform(async move { client.edit_message(&editing.chat_id, &editing.id, &text).await }, Message::Edited);
        }

        let attachment = main.composer.attachment.take();
        if text.is_empty() && attachment.is_none() {
            return Task::none();
        }
        let reply_to = main.composer.replying.take().map(|message| message.id);
        main.composer.text.clear();
        main.composer.drafts.insert("__failed".into(), text.clone());
        main.composer.sending = true;
        if let (Some((typing_chat, _)), Some(connection)) = (main.typing_sent.take(), &main.connection) {
            connection.typing(&typing_chat, false);
        }
        let result_chat = chat_id.clone();
        Task::perform(
            async move {
                match attachment {
                    Some(attachment) => client.send_media(&chat_id, &attachment.path, &text, reply_to.as_deref()).await,
                    None => client.send_message(&chat_id, &text, reply_to.as_deref()).await,
                }
            },
            move |result| Message::Sent { chat_id: result_chat.clone(), result },
        )
    }

    // ========================================================================
    // Realtime events
    // ========================================================================

    fn realtime(&mut self, event: ws::Event) -> Task<Message> {
        let focused = self.focused;
        let notifications = self.settings.notifications;
        let previews = self.settings.notification_previews;
        let window = self.window;
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        match event {
            ws::Event::Connected(connection) => {
                let reconnect = main.ws_status == WsStatus::Offline;
                main.connection = Some(connection);
                main.ws_status = WsStatus::Connected;
                main.media.clear_failures();
                // Catch up on anything missed while disconnected.
                let mut tasks = vec![self.load_chats()];
                if reconnect && let Screen::Main(main) = &self.screen && let Some(active) = main.active.clone() {
                    let client = self.client.clone();
                    let chat_id = active.clone();
                    tasks.push(Task::perform(async move { client.messages(&active, None).await }, move |result| Message::MessagesLoaded {
                        chat_id: chat_id.clone(),
                        before: None,
                        result,
                    }));
                }
                return Task::batch(tasks);
            }
            ws::Event::Disconnected { .. } => {
                main.ws_status = WsStatus::Offline;
                main.connection = None;
                main.online.clear();
                main.typing.clear();
            }
            ws::Event::SessionExpired(reason) => return self.session_expired(reason),
            ws::Event::Server(event) => match event {
                ServerEvent::Ready { .. } | ServerEvent::Pong | ServerEvent::Unknown(_) => {}
                ServerEvent::PresenceSnapshot { online_user_ids, last_seen } => {
                    main.online = online_user_ids.into_iter().collect();
                    main.last_seen = last_seen.into_iter().filter_map(|(user_id, at)| Some((user_id, at?))).collect();
                }
                ServerEvent::Online { user_id } => {
                    main.online.insert(user_id);
                }
                ServerEvent::Offline { user_id, last_seen_at } => {
                    main.online.remove(&user_id);
                    set_last_seen(main, user_id, last_seen_at);
                }
                ServerEvent::PresenceState { user_id, online, last_seen_at } => {
                    if online {
                        main.online.insert(user_id.clone());
                    } else {
                        main.online.remove(&user_id);
                    }
                    set_last_seen(main, user_id, last_seen_at);
                }
                ServerEvent::LastSeen { user_id, last_seen_at } => set_last_seen(main, user_id, last_seen_at),
                ServerEvent::BlocksUpdated => return self.load_blocked(),
                ServerEvent::TypingStart { chat_id, user_id } => {
                    main.typing.entry(chat_id).or_default().insert(user_id, Instant::now());
                }
                ServerEvent::TypingStop { chat_id, user_id } => {
                    if let Some(users) = main.typing.get_mut(&chat_id) {
                        users.remove(&user_id);
                    }
                }
                ServerEvent::MessageNew { message } => {
                    let incoming = message.sender_id != main.me.id;
                    let chat_id = message.chat_id.clone();
                    let known = main.chats.iter().any(|chat| chat.id == chat_id);
                    let is_active = main.active.as_deref() == Some(chat_id.as_str());
                    if incoming && let Some(users) = main.typing.get_mut(&chat_id) {
                        users.remove(&message.sender_id);
                    }
                    let notify_text = (incoming && (!focused || !is_active) && notifications)
                        .then(|| notification_for(main, &message, previews));
                    let stick = is_active && main.at_bottom;
                    apply_message(main, message, incoming && !(is_active && focused && main.at_bottom));
                    let mut tasks = Vec::new();
                    if !known {
                        tasks.push(self.load_chats());
                    }
                    if let Some((title, body)) = notify_text {
                        platform::notify(title, body);
                        if !focused && let Some(id) = window {
                            tasks.push(window::request_user_attention(id, Some(window::UserAttention::Informational)));
                        }
                    }
                    if stick {
                        tasks.push(scroll_to_bottom());
                        if incoming {
                            tasks.push(self.mark_active_read());
                        }
                    }
                    return Task::batch(tasks);
                }
                ServerEvent::MessageUpdated { message } | ServerEvent::MessageDeleted { message } => apply_message(main, message, false),
                ServerEvent::ChatCreated { .. } | ServerEvent::ChatUpdated { .. } => return self.load_chats(),
                ServerEvent::ChatRead { chat_id, user_id, read_at } => {
                    if user_id != main.me.id
                        && let Some(chat) = main.chats.iter_mut().find(|chat| chat.id == chat_id)
                    {
                        chat.peer_last_read_at = Some(chat.peer_last_read_at.map_or(read_at, |current| current.max(read_at)));
                    }
                }
                ServerEvent::ProfileUpdated { user } => {
                    for chat in &mut main.chats {
                        if let Some(peer) = &mut chat.peer
                            && peer.id == user.id
                        {
                            *peer = user.clone();
                        }
                        for member in &mut chat.members {
                            if member.user.id == user.id {
                                member.user = user.clone();
                            }
                        }
                    }
                }
                ServerEvent::AccountBlocked => return self.session_expired("Your account is blocked".into()),
                ServerEvent::Call(value) => return self.call_signal(value),
            },
        }
        Task::none()
    }

    fn send_signal(main: &Main, value: serde_json::Value) -> bool {
        main.connection.as_ref().is_some_and(|connection| connection.send(value))
    }

    fn call_message(&mut self, message: Message) -> Task<Message> {
        use crate::call::{ActiveCall, Phase, engine::{Command, Update}};
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        match message {
            Message::StartCall(peer_id, name) => {
                if main.call.is_some() {
                    return Task::none();
                }
                if main.connection.is_none() {
                    self.toast("You are offline. Try again when the connection is back.", true);
                    return Task::none();
                }
                main.call = Some(ActiveCall::outgoing(peer_id, name));
            }
            Message::AcceptCall => {
                if let Some(call) = &mut main.call
                    && call.phase == Phase::Incoming
                {
                    call.accepted = true;
                    call.phase = Phase::Connecting;
                }
            }
            Message::EndCall => {
                if let Some(call) = main.call.take() {
                    Self::send_signal(main, serde_json::json!({ "type": "call.end", "call_id": call.id, "peer_id": call.peer_id }));
                    if let Some(handle) = &call.handle {
                        handle.send(Command::Hangup);
                    }
                }
            }
            Message::ToggleMute => {
                if let Some(call) = &mut main.call {
                    call.muted = !call.muted;
                    if let Some(handle) = &call.handle {
                        handle.set_muted(call.muted);
                    }
                }
            }
            Message::CallUpdate(id, update) => {
                let Some(call) = main.call.as_mut().filter(|call| call.id == id) else { return Task::none() };
                let (call_id, peer_id) = (call.id.clone(), call.peer_id.clone());
                match update {
                    Update::Ready(handle) => {
                        handle.set_muted(call.muted);
                        for candidate in call.pending_candidates.drain(..) {
                            handle.send(Command::RemoteCandidate(candidate));
                        }
                        if let Some(answer) = call.pending_answer.take() {
                            handle.send(Command::RemoteAnswer(answer));
                        }
                        call.handle = Some(handle);
                    }
                    Update::Description { kind, sdp } => {
                        let event = if kind == "offer" { "call.offer" } else { "call.answer" };
                        let sent = Self::send_signal(main, serde_json::json!({ "type": event, "call_id": call_id, "peer_id": peer_id, "sdp": { "type": kind, "sdp": sdp } }));
                        if !sent {
                            main.call = None;
                            self.toast("No connection to the server.", true);
                        }
                    }
                    Update::Candidate(candidate) => {
                        Self::send_signal(main, serde_json::json!({ "type": "call.ice", "call_id": call_id, "peer_id": peer_id, "candidate": candidate }));
                    }
                    Update::Connected => {
                        call.phase = Phase::Connected;
                        call.started_at.get_or_insert_with(Instant::now);
                        Self::send_signal(main, serde_json::json!({ "type": "call.connected", "call_id": call_id, "peer_id": peer_id }));
                    }
                    Update::Failed(error) => {
                        Self::send_signal(main, serde_json::json!({ "type": "call.end", "call_id": call_id, "peer_id": peer_id, "reason": "failed" }));
                        main.call = None;
                        self.toast(error, true);
                    }
                    Update::Ended { .. } => {}
                }
            }
            _ => {}
        }
        Task::none()
    }

    /// WebRTC signalling relayed by the server (see `backend/app/api/ws.py`).
    fn call_signal(&mut self, value: serde_json::Value) -> Task<Message> {
        use crate::call::{ActiveCall, Phase, engine::Command};
        let notifications = self.settings.notifications;
        let window = self.window;
        let Screen::Main(main) = &mut self.screen else { return Task::none() };
        let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let kind = text("type").unwrap_or_default();
        let call_id = text("call_id").unwrap_or_default();
        let sdp = value.get("sdp").and_then(|sdp| sdp.get("sdp")).and_then(|sdp| sdp.as_str()).map(str::to_owned);
        match kind.as_str() {
            "call.offer" => {
                let expired = value.get("expires_at").and_then(|v| v.as_i64()).is_some_and(|at| at <= Utc::now().timestamp_millis());
                let (Some(peer_id), Some(offer)) = (text("user_id"), sdp) else { return Task::none() };
                if expired || main.call.as_ref().is_some_and(|call| call.id == call_id) {
                    return Task::none();
                }
                if main.call.is_some() {
                    Self::send_signal(main, serde_json::json!({ "type": "call.end", "call_id": call_id, "peer_id": peer_id, "reason": "busy" }));
                    return Task::none();
                }
                let name = text("name").unwrap_or_else(|| "Someone".into());
                if notifications {
                    platform::notify("Incoming audio call".into(), format!("{name} is calling you"));
                }
                main.call = Some(ActiveCall::incoming(call_id, peer_id, name, offer));
                if let Some(id) = window {
                    return Task::batch([window::request_user_attention(id, Some(window::UserAttention::Critical)), window::gain_focus(id)]);
                }
            }
            "call.answer" => {
                let Some(call) = main.call.as_mut().filter(|call| call.id == call_id && call.outgoing) else { return Task::none() };
                let Some(answer) = sdp else { return Task::none() };
                if call.phase != Phase::Connected {
                    call.phase = Phase::Connecting;
                }
                match &call.handle {
                    Some(handle) => handle.send(Command::RemoteAnswer(answer)),
                    None => call.pending_answer = Some(answer),
                }
            }
            "call.ice" => {
                let Some(call) = main.call.as_mut().filter(|call| call.id == call_id) else { return Task::none() };
                let Some(candidate) = value.get("candidate").cloned() else { return Task::none() };
                match &call.handle {
                    Some(handle) => handle.send(Command::RemoteCandidate(candidate)),
                    None => call.pending_candidates.push(candidate),
                }
            }
            "call.started" => {
                if let Some(call) = main.call.as_mut().filter(|call| call.id == call_id) {
                    call.phase = Phase::Connected;
                    call.started_at.get_or_insert_with(Instant::now);
                }
            }
            "call.end" => {
                if main.call.as_ref().is_some_and(|call| call.id == call_id) {
                    let call = main.call.take().expect("checked above");
                    if let Some(handle) = &call.handle {
                        handle.send(Command::Hangup);
                    }
                    let mine_declined = !call.outgoing && call.phase == Phase::Incoming;
                    if let Some(reason) = crate::call::end_reason(text("reason").as_deref()).filter(|_| !mine_declined) {
                        self.toast(reason, false);
                    }
                }
            }
            _ => {}
        }
        Task::none()
    }

    pub fn view(&self) -> iced::Element<'_, Message> {
        ui::view(self)
    }
}

fn normalize(server: &str) -> String {
    crate::api::client::normalize_server(server)
}

pub fn sort_chats(chats: &mut [Chat]) {
    chats.sort_by(|a, b| b.activity_at().cmp(&a.activity_at()));
}

fn upsert_chat(chats: &mut Vec<Chat>, chat: Chat) {
    match chats.iter_mut().find(|item| item.id == chat.id) {
        Some(existing) => *existing = chat,
        None => chats.push(chat),
    }
    sort_chats(chats);
}

/// Applies a new/updated/deleted message to the chat list and cached thread.
fn set_last_seen(main: &mut Main, user_id: String, at: Option<DateTime<Utc>>) {
    match at {
        Some(at) => main.last_seen.insert(user_id, at),
        None => main.last_seen.remove(&user_id),
    };
}

fn apply_message(main: &mut Main, message: ChatMessage, count_unread: bool) {
    if let Some(chat) = main.chats.iter_mut().find(|chat| chat.id == message.chat_id) {
        let newer = chat.last_message.as_ref().is_none_or(|last| (last.created_at, &last.id) <= (message.created_at, &message.id));
        let same = chat.last_message.as_ref().is_some_and(|last| last.id == message.id);
        if message.is_deleted() && same {
            // Fall back to the previous visible message if we have it cached.
            chat.last_message = main
                .threads
                .get(&message.chat_id)
                .and_then(|thread| thread.messages.iter().rev().find(|m| m.id != message.id && !m.is_deleted()))
                .map(ChatMessage::to_last_message);
        } else if !message.is_deleted() && (newer || same) {
            let fresh = !same;
            chat.last_message = Some(message.to_last_message());
            if fresh && count_unread {
                chat.unread_count += 1;
            }
        }
    }
    sort_chats(&mut main.chats);
    if let Some(thread) = main.threads.get_mut(&message.chat_id) {
        thread.update_pins(&message);
        if thread.loaded {
            thread.upsert(message);
        }
    }
}

fn notification_for(main: &Main, message: &ChatMessage, previews: bool) -> (String, String) {
    let chat = main.chats.iter().find(|chat| chat.id == message.chat_id);
    let sender = chat
        .and_then(|chat| chat.member(&message.sender_id).map(|member| member.user.name().to_owned()).or_else(|| chat.peer.as_ref().map(|peer| peer.name().to_owned())))
        .unwrap_or_else(|| "New message".to_owned());
    let title = match chat {
        Some(chat) if chat.is_group() => format!("{sender} · {}", chat.title()),
        _ => sender,
    };
    let body = if previews { crate::format::ellipsize(&message.preview(), 140) } else { "New message".to_owned() };
    (title, body)
}

fn media_cache_path(dirs: &Dirs, url: &str) -> PathBuf {
    let name = url.rsplit('/').next().unwrap_or("media").chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect::<String>();
    dirs.cache.join("media").join(if name.is_empty() { "media".to_owned() } else { name })
}

/// Saves an image from the clipboard as PNG (for Ctrl+V), if there is one.
fn clipboard_image(directory: &std::path::Path) -> Option<PathBuf> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let buffer = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())?;
    std::fs::create_dir_all(directory).ok()?;
    let path = directory.join(format!("pasted-{}.png", Utc::now().format("%Y%m%d-%H%M%S")));
    buffer.save(&path).ok()?;
    Some(path)
}
