//! Modal dialogs: their state, messages, update logic and views.

use iced::widget::{Space, button, column, container, image, row, scrollable, text, text_editor, text_input};
use iced::{Alignment, Element, Length, Task};

use crate::api::models::{AdminStats, AdminUser, Chat, GroupDetails, GroupRole, Message as ChatMessage, User};
use crate::api::{ApiError, Client};
use crate::app::{Main, Message};
use crate::format;
use crate::media::{Key, Size};
use crate::storage::{Dirs, Settings};
use crate::ui::icons::Icon;
use crate::ui::settings as settings_ui;
use crate::ui::theme;
use crate::ui::widgets::{self, Ctx, dialog_header, field_label, gap, hspace, label, modal, strong};
use crate::update::Updater;

pub enum Dialog {
    ConfirmDelete { message: ChatMessage, busy: bool },
    Viewer(Viewer),
    Settings(Box<settings_ui::State>),
    Peer(User),
    Group(GroupProfile),
    CreateGroup(CreateGroup),
    Pinned { chat_id: String },
    Admin(Box<Admin>),
    Update,
}

impl Dialog {
    /// Dialogs that hide the chat (so incoming messages are not "read").
    pub fn covers_chat(&self) -> bool {
        !matches!(self, Dialog::ConfirmDelete { .. })
    }
}

#[derive(Debug, Clone)]
pub struct Viewer {
    pub urls: Vec<String>,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub enum Open {
    Settings,
    PeerProfile(User),
    GroupProfile(String),
    CreateGroup,
    Pinned(String),
    Admin,
    Update,
}

#[derive(Debug, Clone)]
pub enum Msg {
    ConfirmDelete,
    ViewerStep(i32),
    Settings(settings_ui::Msg),
    Group(GroupMsg),
    Create(CreateMsg),
    Admin(AdminMsg),
    OpenMember(User),
    BackToGroup,
    Pinned(String),
}

pub enum Outcome {
    None,
    Task(Task<Message>),
    ProfileUpdated(User),
    ChatUpdated(Chat),
    OpenChat(Chat),
    Jump(String),
    SignedOut(String),
    ServerChanged,
}

pub struct Context<'a> {
    pub client: &'a Client,
    pub me: &'a User,
    pub chats: &'a [Chat],
    pub settings: &'a mut Settings,
    pub dirs: &'a Dirs,
}

fn dialog_task(task: Task<Msg>) -> Outcome {
    Outcome::Task(task.map(Message::Dialog))
}

pub fn open(open: Open, main: &Main, client: &Client, settings: &Settings, updater: &Updater) -> (Dialog, Task<Message>) {
    let _ = updater;
    match open {
        Open::Settings => {
            let (state, task) = settings_ui::State::new(&main.me, settings, client);
            (Dialog::Settings(Box::new(state)), task.map(|msg| Message::Dialog(Msg::Settings(msg))))
        }
        Open::PeerProfile(user) => (Dialog::Peer(user), Task::none()),
        Open::GroupProfile(chat_id) => (Dialog::Group(GroupProfile::new(chat_id)), Task::none()),
        Open::CreateGroup => (Dialog::CreateGroup(CreateGroup::default()), iced::widget::operation::focus(iced::widget::Id::from("group-name"))),
        Open::Pinned(chat_id) => (Dialog::Pinned { chat_id }, Task::none()),
        Open::Admin => {
            let admin = Admin::default();
            let task = admin.refresh(client);
            (Dialog::Admin(Box::new(admin)), task.map(Message::Dialog))
        }
        Open::Update => (Dialog::Update, Task::none()),
    }
}

impl Dialog {
    pub fn update(&mut self, msg: Msg, context: Context<'_>) -> Outcome {
        let client = context.client.clone();
        match (self, msg) {
            (Dialog::ConfirmDelete { message, busy }, Msg::ConfirmDelete) => {
                if *busy {
                    return Outcome::None;
                }
                *busy = true;
                let message = message.clone();
                Outcome::Task(Task::perform(async move { client.delete_message(&message.chat_id, &message.id).await }, Message::Deleted))
            }
            (Dialog::Viewer(viewer), Msg::ViewerStep(step)) => {
                let len = viewer.urls.len() as i32;
                if len == 0 {
                    return Outcome::None;
                }
                viewer.index = ((viewer.index as i32 + step).rem_euclid(len)) as usize;
                let key = Key::new(viewer.urls[viewer.index].clone(), Size::Full);
                Outcome::Task(Task::done(Message::MediaVisible(key)))
            }
            (Dialog::Settings(state), Msg::Settings(msg)) => state.update(msg, context),
            (Dialog::Group(state), Msg::Group(msg)) => state.update(msg, context),
            (Dialog::Group(state), Msg::OpenMember(user)) => {
                state.member = Some(user);
                Outcome::None
            }
            (Dialog::Group(state), Msg::BackToGroup) => {
                state.member = None;
                state.moderate = None;
                Outcome::None
            }
            (Dialog::CreateGroup(state), Msg::Create(msg)) => state.update(msg, context),
            (Dialog::Admin(state), Msg::Admin(msg)) => state.update(msg, context),
            (Dialog::Pinned { .. }, Msg::Pinned(message_id)) => Outcome::Jump(message_id),
            _ => Outcome::None,
        }
    }
}

// ============================================================================
// Group profile
// ============================================================================

pub struct GroupProfile {
    pub chat_id: String,
    pub editing: bool,
    pub name: String,
    pub username: String,
    pub description: text_editor::Content,
    pub saving: bool,
    pub error: Option<String>,
    pub member: Option<User>,
    pub moderate: Option<Moderation>,
}

pub struct Moderation {
    pub user: User,
    pub busy: bool,
    pub blocked: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum GroupMsg {
    Edit,
    CancelEdit,
    Name(String),
    Username(String),
    Description(text_editor::Action),
    Save,
    Saved(Result<Chat, ApiError>),
    Moderate(User),
    Block,
    Blocked(Result<AdminUser, ApiError>),
}

impl GroupProfile {
    fn new(chat_id: String) -> Self {
        Self {
            chat_id,
            editing: false,
            name: String::new(),
            username: String::new(),
            description: text_editor::Content::new(),
            saving: false,
            error: None,
            member: None,
            moderate: None,
        }
    }

    fn update(&mut self, msg: GroupMsg, context: Context<'_>) -> Outcome {
        let client = context.client.clone();
        match msg {
            GroupMsg::Edit => {
                if let Some(chat) = context.chats.iter().find(|chat| chat.id == self.chat_id) {
                    self.name = chat.name.clone().unwrap_or_default();
                    self.username = chat.username.clone().unwrap_or_default();
                    self.description = text_editor::Content::with_text(chat.description.as_deref().unwrap_or_default());
                }
                self.error = None;
                self.editing = true;
            }
            GroupMsg::CancelEdit => {
                if !self.saving {
                    self.editing = false;
                }
            }
            GroupMsg::Name(value) => self.name = value.chars().take(100).collect(),
            GroupMsg::Username(value) => self.username = value.trim_start_matches('@').to_lowercase().chars().take(32).collect(),
            GroupMsg::Description(action) => {
                self.description.perform(action);
                let text = self.description.text();
                if text.chars().count() > 500 {
                    self.description = text_editor::Content::with_text(&text.chars().take(500).collect::<String>());
                }
            }
            GroupMsg::Save => {
                if self.saving || self.name.trim().is_empty() {
                    return Outcome::None;
                }
                if let Some(error) = validate_group_username(&self.username) {
                    self.error = Some(error);
                    return Outcome::None;
                }
                self.saving = true;
                self.error = None;
                let details = GroupDetails {
                    name: self.name.trim().to_owned(),
                    username: Some(self.username.trim().to_owned()).filter(|value| !value.is_empty()),
                    description: Some(self.description.text().trim().to_owned()).filter(|value| !value.is_empty()),
                };
                let chat_id = self.chat_id.clone();
                return dialog_task(Task::perform(async move { client.update_group(&chat_id, &details).await }, |result| Msg::Group(GroupMsg::Saved(result))));
            }
            GroupMsg::Saved(result) => {
                self.saving = false;
                match result {
                    Ok(chat) => {
                        self.editing = false;
                        return Outcome::ChatUpdated(chat);
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            GroupMsg::Moderate(user) => {
                self.moderate = Some(Moderation { user, busy: false, blocked: false, error: None });
            }
            GroupMsg::Block => {
                let Some(moderation) = &mut self.moderate else { return Outcome::None };
                if moderation.busy || moderation.blocked {
                    return Outcome::None;
                }
                moderation.busy = true;
                let user_id = moderation.user.id.clone();
                return dialog_task(Task::perform(async move { client.admin_set_blocked(&user_id, true).await }, |result| Msg::Group(GroupMsg::Blocked(result))));
            }
            GroupMsg::Blocked(result) => {
                if let Some(moderation) = &mut self.moderate {
                    moderation.busy = false;
                    match result {
                        Ok(_) => moderation.blocked = true,
                        Err(error) => moderation.error = Some(error.message),
                    }
                }
            }
        }
        Outcome::None
    }
}

fn validate_group_username(username: &str) -> Option<String> {
    let username = username.trim();
    if username.is_empty() {
        return None;
    }
    let valid = (3..=32).contains(&username.len()) && username.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    (!valid).then(|| "Group username must be 3–32 lowercase letters, numbers or underscores.".to_owned())
}

// ============================================================================
// Create group
// ============================================================================

#[derive(Default)]
pub struct CreateGroup {
    pub name: String,
    pub username: String,
    pub description: text_editor::Content,
    pub query: String,
    pub results: Vec<User>,
    pub searching: bool,
    pub selected: Vec<User>,
    pub saving: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CreateMsg {
    Name(String),
    Username(String),
    Description(text_editor::Action),
    Query(String),
    Results(String, Result<Vec<User>, ApiError>),
    Toggle(User),
    Submit,
    Created(Result<Chat, ApiError>),
}

impl CreateGroup {
    fn update(&mut self, msg: CreateMsg, context: Context<'_>) -> Outcome {
        let client = context.client.clone();
        match msg {
            CreateMsg::Name(value) => self.name = value.chars().take(100).collect(),
            CreateMsg::Username(value) => self.username = value.trim_start_matches('@').to_lowercase().chars().take(32).collect(),
            CreateMsg::Description(action) => self.description.perform(action),
            CreateMsg::Query(query) => {
                self.query = query;
                self.results.clear();
                let trimmed = self.query.trim().to_owned();
                self.searching = !trimmed.is_empty();
                if trimmed.is_empty() {
                    return Outcome::None;
                }
                return dialog_task(Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        let result = client.search_users(&trimmed).await;
                        (trimmed, result)
                    },
                    |(query, result)| Msg::Create(CreateMsg::Results(query, result)),
                ));
            }
            CreateMsg::Results(query, result) => {
                if query != self.query.trim() {
                    return Outcome::None;
                }
                self.searching = false;
                match result {
                    Ok(users) => self.results = users.into_iter().filter(|user| user.id != context.me.id).collect(),
                    Err(error) => self.error = Some(error.message),
                }
            }
            CreateMsg::Toggle(user) => {
                if let Some(index) = self.selected.iter().position(|item| item.id == user.id) {
                    self.selected.remove(index);
                } else if self.selected.len() < 99 {
                    self.selected.push(user);
                }
            }
            CreateMsg::Submit => {
                if self.saving || self.name.trim().is_empty() || self.selected.len() < 2 {
                    return Outcome::None;
                }
                if let Some(error) = validate_group_username(&self.username) {
                    self.error = Some(error);
                    return Outcome::None;
                }
                self.saving = true;
                self.error = None;
                let name = self.name.trim().to_owned();
                let ids: Vec<String> = self.selected.iter().map(|user| user.id.clone()).collect();
                let username = self.username.trim().to_owned();
                let description = self.description.text().trim().to_owned();
                return dialog_task(Task::perform(
                    async move { client.create_group(&name, &ids, Some(&username), Some(&description)).await },
                    |result| Msg::Create(CreateMsg::Created(result)),
                ));
            }
            CreateMsg::Created(result) => {
                self.saving = false;
                match result {
                    Ok(chat) => return Outcome::OpenChat(chat),
                    Err(error) => self.error = Some(error.message),
                }
            }
        }
        Outcome::None
    }

    fn contacts<'a>(&self, me: &str, chats: &'a [Chat]) -> Vec<&'a User> {
        let mut seen = std::collections::HashSet::new();
        chats
            .iter()
            .filter(|chat| !chat.is_group())
            .filter_map(|chat| chat.peer.as_ref())
            .filter(|peer| peer.id != me && seen.insert(peer.id.clone()))
            .collect()
    }
}

// ============================================================================
// Admin panel
// ============================================================================

pub const ADMIN_PAGE: u64 = 25;

#[derive(Default)]
pub struct Admin {
    pub stats: Option<AdminStats>,
    pub users: Vec<AdminUser>,
    pub total: u64,
    pub query: String,
    pub search: String,
    pub state: AdminFilter,
    pub offset: u64,
    pub loading: bool,
    pub saving: bool,
    pub target: Option<AdminUser>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub request: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AdminFilter {
    #[default]
    All,
    Active,
    Blocked,
}

impl AdminFilter {
    pub const ALL: [AdminFilter; 3] = [AdminFilter::All, AdminFilter::Active, AdminFilter::Blocked];

    fn query(self) -> &'static str {
        match self {
            AdminFilter::All => "all",
            AdminFilter::Active => "active",
            AdminFilter::Blocked => "blocked",
        }
    }
}

impl std::fmt::Display for AdminFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AdminFilter::All => "All accounts",
            AdminFilter::Active => "Active",
            AdminFilter::Blocked => "Blocked",
        })
    }
}

#[derive(Debug, Clone)]
pub enum AdminMsg {
    Loaded(u64, Result<(AdminStats, crate::api::models::AdminUsersPage), ApiError>),
    Query(String),
    Search,
    Filter(AdminFilter),
    Page(i64),
    Refresh,
    Target(Option<AdminUser>),
    Apply,
    Applied(Result<AdminUser, ApiError>),
}

impl Admin {
    fn refresh(&self, client: &Client) -> Task<Msg> {
        let client = client.clone();
        let (search, state, offset, request) = (self.search.clone(), self.state.query(), self.offset, self.request);
        Task::perform(
            async move {
                let (stats, page) = futures::join!(client.admin_stats(), client.admin_users(&search, state, offset, ADMIN_PAGE));
                Ok((stats?, page?))
            },
            move |result| Msg::Admin(AdminMsg::Loaded(request, result)),
        )
    }

    fn reload(&mut self, client: &Client) -> Outcome {
        self.request += 1;
        self.loading = true;
        self.error = None;
        dialog_task(self.refresh(client))
    }

    fn update(&mut self, msg: AdminMsg, context: Context<'_>) -> Outcome {
        let client = context.client.clone();
        match msg {
            AdminMsg::Loaded(request, result) => {
                if request != self.request {
                    return Outcome::None;
                }
                self.loading = false;
                match result {
                    Ok((stats, page)) => {
                        if self.offset > 0 && page.total <= self.offset {
                            self.offset = (page.total.div_ceil(ADMIN_PAGE).saturating_sub(1)) * ADMIN_PAGE;
                            return self.reload(&client);
                        }
                        self.stats = Some(stats);
                        self.users = page.users;
                        self.total = page.total;
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            AdminMsg::Query(query) => self.query = query.chars().take(100).collect(),
            AdminMsg::Search => {
                self.offset = 0;
                self.search = self.query.trim().to_owned();
                return self.reload(&client);
            }
            AdminMsg::Filter(filter) => {
                self.state = filter;
                self.offset = 0;
                return self.reload(&client);
            }
            AdminMsg::Page(direction) => {
                self.offset = if direction < 0 { self.offset.saturating_sub(ADMIN_PAGE) } else { self.offset + ADMIN_PAGE };
                return self.reload(&client);
            }
            AdminMsg::Refresh => return self.reload(&client),
            AdminMsg::Target(target) => {
                self.target = target;
                self.notice = None;
            }
            AdminMsg::Apply => {
                let Some(target) = &self.target else { return Outcome::None };
                if self.saving {
                    return Outcome::None;
                }
                self.saving = true;
                self.error = None;
                let (id, block) = (target.id.clone(), !target.is_blocked);
                return dialog_task(Task::perform(async move { client.admin_set_blocked(&id, block).await }, |result| Msg::Admin(AdminMsg::Applied(result))));
            }
            AdminMsg::Applied(result) => {
                self.saving = false;
                match result {
                    Ok(user) => {
                        self.target = None;
                        self.notice = Some(format!("@{} {}.", user.username, if user.is_blocked { "blocked" } else { "unblocked" }));
                        return self.reload(&client);
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
        }
        Outcome::None
    }
}

// ============================================================================
// Views
// ============================================================================

fn close() -> Option<Message> {
    Some(Message::CloseDialog)
}

fn dmsg(msg: Msg) -> Message {
    Message::Dialog(msg)
}

pub fn view<'a>(dialog: &'a Dialog, main: &'a Main, ctx: Ctx<'a>, app: &'a crate::app::App) -> Element<'a, Message> {
    match dialog {
        Dialog::ConfirmDelete { busy, .. } => confirm_delete(*busy),
        Dialog::Viewer(viewer) => image_viewer(viewer, ctx),
        Dialog::Settings(state) => settings_ui::view(state, main, ctx, app),
        Dialog::Peer(user) => peer_profile(user, ctx, main.online.contains(&user.id), close()),
        Dialog::Group(state) => match main.chats.iter().find(|chat| chat.id == state.chat_id) {
            Some(chat) => group_profile(state, chat, main, ctx),
            None => modal(text("This group is no longer available.").into(), 380.0, close()),
        },
        Dialog::CreateGroup(state) => create_group(state, main, ctx),
        Dialog::Pinned { chat_id } => pinned_list(main, chat_id),
        Dialog::Admin(state) => admin_panel(state, main),
        Dialog::Update => crate::update::dialog(&app.updater),
    }
}

fn confirm_delete<'a>(busy: bool) -> Element<'a, Message> {
    let content = column![
        container(widgets::icon(Icon::Trash, 22.0, theme::DANGER)).center(48).style(theme::pill(theme::DANGER, theme::DANGER_SOFT)),
        gap(6.0),
        strong("Delete message?", theme::TEXT_LG, theme::TEXT_1),
        label("This message will be deleted for everyone.", theme::TEXT_MD, theme::TEXT_2),
        gap(10.0),
        row![
            widgets::small_button("Cancel", (!busy).then_some(Message::CloseDialog)),
            widgets::small_danger(if busy { "Deleting..." } else { "Delete" }, (!busy).then_some(dmsg(Msg::ConfirmDelete))),
        ]
        .spacing(10),
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    modal(content.into(), 340.0, (!busy).then_some(Message::CloseDialog))
}

fn image_viewer<'a>(viewer: &'a Viewer, ctx: Ctx<'a>) -> Element<'a, Message> {
    let url = &viewer.urls[viewer.index.min(viewer.urls.len().saturating_sub(1))];
    let key = Key::new(url.clone(), Size::Full);
    let stage: Element<'a, Message> = match ctx.media.get(&key) {
        Some(picture) => image::viewer(picture.handle.clone()).width(Length::Fill).height(Length::Fill).max_scale(8.0).into(),
        None if ctx.media.failed(&key) => container(label("Could not load the image.", theme::TEXT_MD, theme::TEXT_2)).center(Length::Fill).into(),
        None => container(label("Loading…", theme::TEXT_MD, theme::TEXT_2)).center(Length::Fill).into(),
    };
    let many = viewer.urls.len() > 1;
    let nav = |glyph, step| -> Element<'a, Message> {
        button(container(widgets::icon(glyph, 22.0, theme::TEXT_1)).center(44)).padding(0).style(theme::floating_round_button).on_press(dmsg(Msg::ViewerStep(step))).into()
    };
    let top = row![
        label(format!("{} / {}", viewer.index + 1, viewer.urls.len()), theme::TEXT_SM, theme::TEXT_2),
        hspace(),
        widgets::small_button("Save as…", Some(Message::SaveMedia(url.clone()))),
        button(container(widgets::icon(Icon::Close, 20.0, theme::TEXT_1)).center(40)).padding(0).style(theme::floating_round_button).on_press(Message::CloseDialog),
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let middle: Element<'a, Message> = if many {
        row![nav(Icon::ChevronLeft, -1), stage, nav(Icon::ChevronRight, 1)].spacing(12).align_y(Alignment::Center).into()
    } else {
        stage
    };
    container(column![top, middle].spacing(12)).padding(18).width(Length::Fill).height(Length::Fill).style(theme::image_viewer).into()
}

pub fn peer_profile<'a>(user: &'a User, ctx: Ctx<'a>, online: bool, on_close: Option<Message>) -> Element<'a, Message> {
    let bio = user.bio.as_deref().map(str::trim).filter(|bio| !bio.is_empty()).unwrap_or("No bio yet");
    let content = column![
        row![hspace(), widgets::icon_button(Icon::Close, on_close.clone(), 34.0)],
        container(widgets::avatar(ctx, user, 96.0, online)).center_x(Length::Fill),
        gap(6.0),
        container(text(user.name()).size(theme::TEXT_XL).font(theme::SEMIBOLD)).center_x(Length::Fill),
        container(label(if online { "Online" } else { "Offline" }, theme::TEXT_SM, if online { theme::SUCCESS } else { theme::TEXT_3 })).center_x(Length::Fill),
        gap(10.0),
        container(
            column![
                widgets::info_row("Username", label(format!("@{}", user.username), theme::TEXT_MD, theme::TEXT_1).into()),
                widgets::info_row("Bio", label(bio, theme::TEXT_MD, if user.bio.is_some() { theme::TEXT_1 } else { theme::TEXT_3 }).into()),
            ]
            .spacing(14)
        )
        .padding(16)
        .width(Length::Fill)
        .style(theme::card),
    ]
    .spacing(4);
    modal(content.into(), 360.0, on_close)
}

fn group_profile<'a>(state: &'a GroupProfile, chat: &'a Chat, main: &'a Main, ctx: Ctx<'a>) -> Element<'a, Message> {
    if let Some(user) = &state.member {
        return peer_profile(user, ctx, main.online.contains(&user.id), Some(dmsg(Msg::BackToGroup)));
    }
    if let Some(moderation) = &state.moderate {
        let body: Element<'a, Message> = if moderation.blocked {
            label("Account blocked.", theme::TEXT_MD, theme::SUCCESS).into()
        } else {
            column![
                label("Block this account across the entire app. The user will be signed out and will not be able to sign in.", theme::TEXT_MD, theme::TEXT_2),
                widgets::small_danger(if moderation.busy { "Blocking…" } else { "Block account" }, (!moderation.busy).then_some(dmsg(Msg::Group(GroupMsg::Block)))),
            ]
            .spacing(12)
            .into()
        };
        let mut content = column![dialog_header(format!("@{}", moderation.user.username), Some(dmsg(Msg::BackToGroup))), body].spacing(14);
        if let Some(error) = &moderation.error {
            content = content.push(widgets::error_text(error.as_str()));
        }
        return modal(content.into(), 380.0, Some(dmsg(Msg::BackToGroup)));
    }

    let is_admin = chat.is_admin(&main.me.id);
    if state.editing {
        let saving = state.saving;
        let mut form = column![
            dialog_header("Edit group", (!saving).then_some(dmsg(Msg::Group(GroupMsg::CancelEdit)))),
            field_label("Group name"),
            text_input("Group name", &state.name).on_input(|v| dmsg(Msg::Group(GroupMsg::Name(v)))).padding(11).style(theme::field),
            field_label("Group username"),
            text_input("username", &state.username).on_input(|v| dmsg(Msg::Group(GroupMsg::Username(v)))).padding(11).style(theme::field),
            field_label("Description"),
            text_editor(&state.description)
                .placeholder("What is this group about?")
                .on_action(|action| dmsg(Msg::Group(GroupMsg::Description(action))))
                .height(110)
                .padding(11)
                .style(theme::editor),
            container(label(format!("{}/500", state.description.text().chars().count()), theme::TEXT_XS, theme::TEXT_3)).align_right(Length::Fill),
        ]
        .spacing(8);
        if let Some(error) = &state.error {
            form = form.push(widgets::error_text(error.as_str()));
        }
        form = form.push(
            row![
                hspace(),
                widgets::small_button("Cancel", (!saving).then_some(dmsg(Msg::Group(GroupMsg::CancelEdit)))),
                widgets::small_primary(if saving { "Saving…" } else { "Save changes" }, (!saving && !state.name.trim().is_empty()).then_some(dmsg(Msg::Group(GroupMsg::Save)))),
            ]
            .spacing(10),
        );
        return modal(form.into(), 440.0, None);
    }

    let mut members: Vec<_> = chat.members.iter().collect();
    members.sort_by_key(|member| member.group_role != GroupRole::Admin);
    let site_admin = main.me.is_admin;
    let list = column(members.into_iter().map(|member| {
        let user = &member.user;
        let mut name = row![strong(user.name(), theme::TEXT_MD, theme::TEXT_1)];
        if member.group_role == GroupRole::Admin {
            name = name.push(container(label("Admin", theme::TEXT_XS, theme::ACCENT).font(theme::SEMIBOLD)).padding([1, 7]).style(theme::pill(theme::ACCENT, theme::ACCENT_SOFT)));
        }
        let online = main.online.contains(&user.id) || user.id == main.me.id;
        let item = row![
            widgets::avatar(ctx, user, 38.0, online && user.id != main.me.id),
            column![name.spacing(8).align_y(Alignment::Center), label(format!("@{}", user.username), theme::TEXT_SM, theme::TEXT_3)].spacing(1),
            hspace(),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let entry = button(item).padding([7, 8]).width(Length::Fill).style(theme::list_button).on_press(dmsg(Msg::OpenMember(user.clone())));
        let moderatable = site_admin && user.id != main.me.id && !user.is_admin;
        if moderatable {
            iced::widget::mouse_area(entry).on_right_press(dmsg(Msg::Group(GroupMsg::Moderate(user.clone())))).into()
        } else {
            entry.into()
        }
    }))
    .spacing(2);

    let mut details = column![
        widgets::info_row(
            "Username",
            label(chat.username.as_deref().map(|name| format!("@{name}")).unwrap_or_else(|| "Not set".into()), theme::TEXT_MD, if chat.username.is_some() { theme::ACCENT } else { theme::TEXT_3 }).into()
        ),
        widgets::info_row(
            "Description",
            label(chat.description.as_deref().filter(|d| !d.trim().is_empty()).unwrap_or("No description"), theme::TEXT_MD, if chat.description.is_some() { theme::TEXT_1 } else { theme::TEXT_3 }).into()
        ),
    ]
    .spacing(14);
    if is_admin {
        details = details.push(
            button(row![widgets::icon(Icon::Edit, 16.0, theme::TEXT_1), label("Edit group info", theme::TEXT_SM, theme::TEXT_1)].spacing(8).align_y(Alignment::Center))
                .padding([8, 12])
                .style(theme::secondary_button)
                .on_press(dmsg(Msg::Group(GroupMsg::Edit))),
        );
    }

    let content = column![
        dialog_header("Group info", close()),
        column![
            widgets::group_avatar(&chat.id, 84.0),
            strong(chat.title(), theme::TEXT_XL, theme::TEXT_1),
            label(format!("{} participants", chat.members.len()), theme::TEXT_SM, theme::TEXT_3),
        ]
        .spacing(6)
        .align_x(Alignment::Center)
        .width(Length::Fill),
        container(details).padding(16).width(Length::Fill).style(theme::card),
        row![strong("Participants", theme::TEXT_MD, theme::TEXT_1), label(chat.members.len().to_string(), theme::TEXT_SM, theme::TEXT_3)].spacing(8),
        scrollable(list).height(Length::Fill).style(theme::scroll),
    ]
    .spacing(14);
    modal(container(content).height(620).into(), 440.0, close())
}

fn create_group<'a>(state: &'a CreateGroup, main: &'a Main, ctx: Ctx<'a>) -> Element<'a, Message> {
    let saving = state.saving;
    let contacts = state.contacts(&main.me.id, &main.chats);
    let visible: Vec<&User> = if state.query.trim().is_empty() { contacts } else { state.results.iter().collect() };
    let listing: Element<'a, Message> = if state.searching {
        label("Searching…", theme::TEXT_SM, theme::TEXT_3).into()
    } else if !state.query.trim().is_empty() && state.results.is_empty() {
        label("No users found.", theme::TEXT_SM, theme::TEXT_3).into()
    } else if visible.is_empty() {
        label("Search by username to add participants.", theme::TEXT_SM, theme::TEXT_3).into()
    } else {
        column(visible.into_iter().map(|user| {
            let checked = state.selected.iter().any(|item| item.id == user.id);
            let check: Element<'a, Message> = if checked {
                container(widgets::icon(Icon::Check, 14.0, theme::PRIMARY_TEXT)).center(20).style(theme::badge).into()
            } else {
                container(Space::new()).width(20).height(20).style(|_| container::Style {
                    border: iced::Border { color: theme::LINE_STRONG, width: 1.5, radius: 999.0.into() },
                    ..Default::default()
                }).into()
            };
            button(
                row![
                    check,
                    widgets::avatar(ctx, user, 34.0, false),
                    column![strong(user.name(), theme::TEXT_MD, theme::TEXT_1), label(format!("@{}", user.username), theme::TEXT_XS, theme::TEXT_3)],
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .style(theme::list_button)
            .on_press_maybe((!saving && (checked || state.selected.len() < 99)).then(|| dmsg(Msg::Create(CreateMsg::Toggle(user.clone())))))
            .into()
        }))
        .spacing(2)
        .into()
    };
    let chips = row(state.selected.iter().map(|user| {
        button(row![label(user.name(), theme::TEXT_SM, theme::TEXT_1), widgets::icon(Icon::Close, 12.0, theme::TEXT_2)].spacing(6).align_y(Alignment::Center))
            .padding([5, 10])
            .style(theme::secondary_button)
            .on_press_maybe((!saving).then(|| dmsg(Msg::Create(CreateMsg::Toggle(user.clone())))))
            .into()
    }))
    .spacing(6)
    .wrap();

    let can_create = !saving && !state.name.trim().is_empty() && state.selected.len() >= 2;
    let submit_label = if saving { "Creating…".to_owned() } else { format!("Create group · {} participants", state.selected.len() + 1) };
    let mut content = column![
        dialog_header("New group", (!saving).then_some(Message::CloseDialog)),
        field_label("Group name"),
        text_input("Friends, family, team…", &state.name)
            .id(iced::widget::Id::from("group-name"))
            .on_input(|v| dmsg(Msg::Create(CreateMsg::Name(v))))
            .padding(11)
            .style(theme::field),
        row![field_label("Group username"), label("Optional · 3–32 letters, numbers or underscores", theme::TEXT_XS, theme::TEXT_3)].spacing(8).align_y(Alignment::Center),
        text_input("username", &state.username).on_input(|v| dmsg(Msg::Create(CreateMsg::Username(v)))).padding(11).style(theme::field),
        row![field_label("Description"), label("Optional", theme::TEXT_XS, theme::TEXT_3)].spacing(8),
        text_editor(&state.description)
            .placeholder("What is this group about?")
            .on_action(|action| dmsg(Msg::Create(CreateMsg::Description(action))))
            .height(64)
            .padding(11)
            .style(theme::editor),
        field_label("Add participants"),
        text_input("Search by username", &state.query).on_input(|v| dmsg(Msg::Create(CreateMsg::Query(v)))).padding(11).style(theme::field),
    ]
    .spacing(8);
    if state.query.trim().is_empty() {
        content = content.push(label("Your chats", theme::TEXT_XS, theme::TEXT_3).font(theme::SEMIBOLD));
    }
    content = content
        .push(scrollable(listing).height(Length::Fill).style(theme::scroll))
        .push(label("Select at least two people. You will be added automatically.", theme::TEXT_XS, theme::TEXT_3))
        .push(chips);
    if let Some(error) = &state.error {
        content = content.push(widgets::error_text(error.as_str()));
    }
    content = content.push(widgets::primary_button(submit_label, can_create.then(|| dmsg(Msg::Create(CreateMsg::Submit)))));
    // Fixed-height card; only the participant list scrolls (an outer scrollable around
    // the whole form clipped its children incorrectly).
    modal(container(content).height(Length::Fixed(700.0)).into(), 460.0, None)
}

fn pinned_list<'a>(main: &'a Main, chat_id: &'a str) -> Element<'a, Message> {
    let pins: Vec<&ChatMessage> = main.threads.get(chat_id).map(|thread| thread.pins.iter().collect()).unwrap_or_default();
    let mut ordered = pins;
    ordered.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| b.id.cmp(&a.id)));
    let body: Element<'a, Message> = if ordered.is_empty() {
        label("No pinned messages.", theme::TEXT_MD, theme::TEXT_3).into()
    } else {
        scrollable(
            column(ordered.into_iter().map(|message| {
                button(
                    row![
                        container(widgets::icon(Icon::Pin, 16.0, theme::ACCENT)).center(32).style(theme::pill(theme::ACCENT, theme::ACCENT_SOFT)),
                        column![
                            label(format::ellipsize(&message.preview(), 80), theme::TEXT_MD, theme::TEXT_1),
                            label(format::session_date(Some(message.created_at)), theme::TEXT_XS, theme::TEXT_3),
                        ]
                        .spacing(2)
                        .width(Length::Fill),
                        widgets::icon(Icon::ChevronRight, 16.0, theme::TEXT_3),
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 10])
                .width(Length::Fill)
                .style(theme::list_button)
                .on_press(dmsg(Msg::Pinned(message.id.clone())))
                .into()
            }))
            .spacing(2),
        )
        .height(Length::Shrink)
        .style(theme::scroll)
        .into()
    };
    let count = main.threads.get(chat_id).map_or(0, |thread| thread.pins.len());
    let content = column![
        row![strong("Pinned messages", theme::TEXT_XL, theme::TEXT_1), label(count.to_string(), theme::TEXT_SM, theme::TEXT_3), hspace(), widgets::icon_button(Icon::Close, close(), 36.0)]
            .spacing(8)
            .align_y(Alignment::Center),
        body,
    ]
    .spacing(14);
    modal(content.into(), 440.0, close())
}

fn admin_panel<'a>(state: &'a Admin, main: &'a Main) -> Element<'a, Message> {
    let busy = state.saving || state.target.is_some();
    let stat = |name: &'static str, value: Option<u64>| -> Element<'a, Message> {
        container(column![label(name, theme::TEXT_XS, theme::TEXT_3), strong(value.map(|v| v.to_string()).unwrap_or_else(|| "—".into()), theme::TEXT_XL, theme::TEXT_1)].spacing(4))
            .padding(12)
            .width(Length::Fill)
            .style(theme::card)
            .into()
    };
    let stats = state.stats.as_ref();
    let grid = column![
        row![stat("Users", stats.map(|s| s.users)), stat("Online now", stats.map(|s| s.online_users)), stat("Groups", stats.map(|s| s.groups))].spacing(10),
        row![stat("Private chats", stats.map(|s| s.private_chats)), stat("Messages", stats.map(|s| s.messages)), stat("Blocked", stats.map(|s| s.blocked_users))].spacing(10),
    ]
    .spacing(10);

    let filters = row![
        text_input("Name, username or email", &state.query)
            .on_input_maybe((!busy).then_some(|v| dmsg(Msg::Admin(AdminMsg::Query(v)))))
            .on_submit(dmsg(Msg::Admin(AdminMsg::Search)))
            .padding(10)
            .style(theme::field),
        iced::widget::pick_list(AdminFilter::ALL, Some(state.state), |filter| dmsg(Msg::Admin(AdminMsg::Filter(filter)))).padding(10).text_size(theme::TEXT_SM),
        widgets::small_button("Search", (!state.loading && !busy).then_some(dmsg(Msg::Admin(AdminMsg::Search)))),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let rows: Element<'a, Message> = if state.loading && state.users.is_empty() {
        label("Loading users…", theme::TEXT_SM, theme::TEXT_3).into()
    } else if state.users.is_empty() {
        label("No users found.", theme::TEXT_SM, theme::TEXT_3).into()
    } else {
        column(state.users.iter().map(|user| {
            let you = user.id == main.me.id;
            let (badge, color, background) = if user.is_blocked {
                ("Blocked", theme::DANGER, theme::DANGER_SOFT)
            } else if user.is_admin {
                ("Admin", theme::ACCENT, theme::ACCENT_SOFT)
            } else {
                ("Active", theme::SUCCESS, theme::SUCCESS_SOFT)
            };
            let action: Element<'a, Message> = if user.is_admin || you {
                label("Protected", theme::TEXT_XS, theme::TEXT_3).into()
            } else {
                button(label(if user.is_blocked { "Unblock" } else { "Block" }, theme::TEXT_SM, if user.is_blocked { theme::TEXT_1 } else { theme::DANGER }))
                    .padding([5, 10])
                    .style(if user.is_blocked { theme::secondary_button } else { theme::danger_text_button })
                    .on_press_maybe((!busy).then(|| dmsg(Msg::Admin(AdminMsg::Target(Some(user.clone()))))))
                    .into()
            };
            row![
                column![
                    strong(user.display_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| user.username.clone()), theme::TEXT_SM, theme::TEXT_1),
                    label(format!("@{}{}", user.username, if you { " · you" } else { "" }), theme::TEXT_XS, theme::TEXT_3)
                ]
                .width(Length::FillPortion(3)),
                column![
                    label(user.email.clone().unwrap_or_else(|| "—".into()), theme::TEXT_SM, theme::TEXT_2),
                    label(if user.email.is_none() { "" } else if user.email_verified_at.is_some() { "Verified" } else { "Not verified" }, theme::TEXT_XS, theme::TEXT_3)
                ]
                .width(Length::FillPortion(3)),
                label(user.created_at.with_timezone(&chrono::Local).format("%-d %b %Y").to_string(), theme::TEXT_SM, theme::TEXT_2).width(Length::FillPortion(2)),
                container(container(label(badge, theme::TEXT_XS, color).font(theme::SEMIBOLD)).padding([2, 8]).style(theme::pill(color, background))).width(Length::FillPortion(2)),
                container(action).width(Length::FillPortion(2)),
            ]
            .spacing(10)
            .padding([8, 4])
            .align_y(Alignment::Center)
            .into()
        }))
        .into()
    };

    let mut content = column![
        row![
            column![
                label("MESSENGER", theme::TEXT_XS, theme::TEXT_3).font(theme::SEMIBOLD),
                strong("Admin panel", theme::TEXT_2XL, theme::TEXT_1),
                label("Overview and account management", theme::TEXT_SM, theme::TEXT_2)
            ]
            .spacing(2),
            hspace(),
            widgets::icon_button(Icon::Close, (!state.saving).then_some(Message::CloseDialog), 36.0)
        ]
        .align_y(Alignment::Start),
    ]
    .spacing(14);
    if let Some(error) = &state.error {
        content = content.push(row![widgets::error_text(error.as_str()), widgets::small_button("Retry", Some(dmsg(Msg::Admin(AdminMsg::Refresh))))].spacing(8).align_y(Alignment::Center));
    }
    if let Some(notice) = &state.notice {
        content = content.push(widgets::success_text(notice.as_str()));
    }
    content = content.push(grid).push(
        row![
            strong("Users", theme::TEXT_LG, theme::TEXT_1),
            label(state.total.to_string(), theme::TEXT_SM, theme::TEXT_3),
            hspace(),
            widgets::small_button("Refresh", (!state.loading && !busy).then_some(dmsg(Msg::Admin(AdminMsg::Refresh))))
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    content = content.push(filters);
    if let Some(target) = &state.target {
        let block = !target.is_blocked;
        content = content.push(
            container(
                column![
                    strong(format!("{} @{}?", if block { "Block" } else { "Unblock" }, target.username), theme::TEXT_MD, theme::TEXT_1),
                    label(
                        if block { "The user will be signed out and unable to sign in or send messages. Their messages will remain." } else { "This user will be able to sign in again." },
                        theme::TEXT_SM,
                        theme::TEXT_2
                    ),
                    row![
                        if block {
                            widgets::small_danger(if state.saving { "Saving…" } else { "Block account" }, (!state.saving).then_some(dmsg(Msg::Admin(AdminMsg::Apply))))
                        } else {
                            widgets::small_primary(if state.saving { "Saving…" } else { "Unblock account" }, (!state.saving).then_some(dmsg(Msg::Admin(AdminMsg::Apply))))
                        },
                        widgets::small_button("Cancel", (!state.saving).then_some(dmsg(Msg::Admin(AdminMsg::Target(None))))),
                    ]
                    .spacing(8)
                ]
                .spacing(8),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::card),
        );
    }
    let header = row![
        label("User", theme::TEXT_XS, theme::TEXT_3).width(Length::FillPortion(3)),
        label("Email", theme::TEXT_XS, theme::TEXT_3).width(Length::FillPortion(3)),
        label("Joined", theme::TEXT_XS, theme::TEXT_3).width(Length::FillPortion(2)),
        label("Status", theme::TEXT_XS, theme::TEXT_3).width(Length::FillPortion(2)),
        label("Action", theme::TEXT_XS, theme::TEXT_3).width(Length::FillPortion(2)),
    ]
    .spacing(10)
    .padding([0, 4]);
    let range = if state.total > 0 {
        format!("{}–{} of {}", state.offset + 1, (state.offset + ADMIN_PAGE).min(state.total), state.total)
    } else {
        "0 users".to_owned()
    };
    content = content.push(header).push(widgets::divider()).push(scrollable(rows).height(Length::Fill).style(theme::scroll)).push(
        row![
            label(range, theme::TEXT_SM, theme::TEXT_3),
            hspace(),
            widgets::small_button("Previous", (!state.loading && !busy && state.offset > 0).then_some(dmsg(Msg::Admin(AdminMsg::Page(-1))))),
            widgets::small_button("Next", (!state.loading && !busy && state.offset + ADMIN_PAGE < state.total).then_some(dmsg(Msg::Admin(AdminMsg::Page(1))))),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    modal(container(content).height(760).into(), 860.0, (!state.saving).then_some(Message::CloseDialog))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_usernames() {
        assert!(validate_group_username("").is_none());
        assert!(validate_group_username("team_42").is_none());
        assert!(validate_group_username("ab").is_some());
        assert!(validate_group_username("Has-Dash").is_some());
    }
}
