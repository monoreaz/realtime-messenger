//! Settings window (`App.tsx` profile overlay): Profile, Security, Active sessions,
//! Privacy — plus a desktop-only "Application" tab (notifications, updates, server).

use std::path::PathBuf;

use iced::widget::{Space, button, column, container, row, scrollable, text_editor, text_input, toggler};
use iced::{Alignment, Element, Length, Task};

use super::icons::Icon;
use super::theme;
use super::widgets::{self, Ctx, field_label, gap, hspace, label, modal, strong};
use crate::api::models::{Privacy, ProfileUpdate, SessionInfo, User};
use crate::api::{ApiError, Client};
use crate::app::{Main, Message};
use crate::dialogs::{Context, Msg as DialogMsg, Outcome};
use crate::format;
use crate::storage::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Profile,
    Security,
    Sessions,
    Privacy,
    Application,
}

pub struct State {
    pub tab: Tab,
    pub display_name: String,
    pub username: String,
    pub bio: text_editor::Content,
    pub saving: bool,
    pub uploading: bool,
    pub current_password: String,
    pub new_password: String,
    pub confirm_password: String,
    pub changing_password: bool,
    pub sessions: Vec<SessionInfo>,
    pub sessions_loading: bool,
    pub session_action: Option<String>,
    pub logging_out_others: bool,
    /// "everyone" / "nobody"; `None` until loaded.
    pub last_seen: Option<String>,
    pub privacy_saving: bool,
    pub server: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Tab(Tab),
    DisplayName(String),
    Username(String),
    Bio(text_editor::Action),
    SaveProfile,
    ProfileSaved(Result<User, ApiError>),
    PickAvatar,
    AvatarPicked(Option<PathBuf>),
    RemoveAvatar,
    AvatarDone(Result<User, ApiError>),
    CurrentPassword(String),
    NewPassword(String),
    ConfirmPassword(String),
    ChangePassword,
    PasswordChanged(Result<(), ApiError>),
    LoadSessions,
    Sessions(Result<Vec<SessionInfo>, ApiError>),
    Revoke(String),
    Revoked(String, Result<(), ApiError>),
    LogoutOthers,
    LoggedOutOthers(Result<(), ApiError>),
    PrivacyLoaded(Result<Privacy, ApiError>),
    SetLastSeen(String),
    /// Carries the previous value, restored if saving fails.
    PrivacySaved(Option<String>, Result<Privacy, ApiError>),
    Notifications(bool),
    Previews(bool),
    AutoUpdate(bool),
    Server(String),
    ApplyServer,
}

fn wrap(msg: Msg) -> Message {
    Message::Dialog(DialogMsg::Settings(msg))
}

impl State {
    pub fn new(me: &User, settings: &Settings, client: &Client) -> (Self, Task<Msg>) {
        let state = Self {
            tab: Tab::Profile,
            display_name: me.display_name.clone().unwrap_or_default(),
            username: me.username.clone(),
            bio: text_editor::Content::with_text(me.bio.as_deref().unwrap_or_default()),
            saving: false,
            uploading: false,
            current_password: String::new(),
            new_password: String::new(),
            confirm_password: String::new(),
            changing_password: false,
            sessions: Vec::new(),
            sessions_loading: false,
            session_action: None,
            logging_out_others: false,
            last_seen: None,
            privacy_saving: false,
            server: settings.server.clone(),
            error: None,
            notice: None,
        };
        let _ = client;
        (state, Task::none())
    }

    fn load_sessions(&mut self, client: &Client) -> Outcome {
        self.sessions_loading = true;
        let client = client.clone();
        Outcome::Task(Task::perform(async move { client.sessions().await }, |result| wrap(Msg::Sessions(result))))
    }

    pub fn update(&mut self, msg: Msg, context: Context<'_>) -> Outcome {
        let client = context.client.clone();
        match msg {
            Msg::Tab(tab) => {
                self.tab = tab;
                self.error = None;
                self.notice = None;
                if tab == Tab::Sessions {
                    return self.load_sessions(&client);
                }
                if tab == Tab::Privacy {
                    let blocked = client.clone();
                    return Outcome::Task(Task::batch([
                        Task::perform(async move { client.privacy().await }, |result| wrap(Msg::PrivacyLoaded(result))),
                        Task::perform(async move { blocked.blocked_users().await }, Message::BlockedLoaded),
                    ]));
                }
            }
            Msg::DisplayName(value) => self.display_name = value.chars().take(64).collect(),
            Msg::Username(value) => self.username = value.trim_start_matches('@').chars().filter(|c| !c.is_whitespace()).take(32).collect(),
            Msg::Bio(action) => {
                self.bio.perform(action);
                let bio = self.bio.text();
                if bio.chars().count() > 160 {
                    self.bio = text_editor::Content::with_text(&bio.chars().take(160).collect::<String>());
                }
            }
            Msg::SaveProfile => {
                if self.saving || self.username.trim().is_empty() {
                    return Outcome::None;
                }
                let username = self.username.trim().to_lowercase();
                if !(3..=32).contains(&username.len()) || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.error = Some("Username must be 3–32 letters, numbers or underscores.".into());
                    return Outcome::None;
                }
                self.saving = true;
                self.error = None;
                self.notice = None;
                let update = ProfileUpdate {
                    username,
                    display_name: Some(self.display_name.trim().to_owned()).filter(|v| !v.is_empty()),
                    bio: Some(self.bio.text().trim().to_owned()).filter(|v| !v.is_empty()),
                };
                return Outcome::Task(Task::perform(async move { client.update_profile(&update).await }, |result| wrap(Msg::ProfileSaved(result))));
            }
            Msg::ProfileSaved(result) => {
                self.saving = false;
                match result {
                    Ok(user) => {
                        self.notice = Some("Profile updated".into());
                        self.username = user.username.clone();
                        return Outcome::ProfileUpdated(user);
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::PickAvatar => {
                return Outcome::Task(Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose a profile photo")
                            .add_filter("Images", &["jpg", "jpeg", "png", "webp"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    |path| wrap(Msg::AvatarPicked(path)),
                ));
            }
            Msg::AvatarPicked(path) => {
                let Some(path) = path else { return Outcome::None };
                if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > 5 * 1024 * 1024 {
                    self.error = Some("Avatar must be 5 MB or smaller.".into());
                    return Outcome::None;
                }
                self.uploading = true;
                self.error = None;
                return Outcome::Task(Task::perform(async move { client.upload_avatar(&path).await }, |result| wrap(Msg::AvatarDone(result))));
            }
            Msg::RemoveAvatar => {
                self.uploading = true;
                return Outcome::Task(Task::perform(async move { client.remove_avatar().await }, |result| wrap(Msg::AvatarDone(result))));
            }
            Msg::AvatarDone(result) => {
                self.uploading = false;
                match result {
                    Ok(user) => return Outcome::ProfileUpdated(user),
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::CurrentPassword(value) => self.current_password = value,
            Msg::NewPassword(value) => self.new_password = value,
            Msg::ConfirmPassword(value) => self.confirm_password = value,
            Msg::ChangePassword => {
                if self.changing_password {
                    return Outcome::None;
                }
                if self.new_password.chars().count() < 8 {
                    self.error = Some("New password must be at least 8 characters.".into());
                    return Outcome::None;
                }
                if self.new_password != self.confirm_password {
                    self.error = Some("Passwords do not match.".into());
                    return Outcome::None;
                }
                self.changing_password = true;
                self.error = None;
                let (current, new) = (self.current_password.clone(), self.new_password.clone());
                return Outcome::Task(Task::perform(async move { client.change_password(&current, &new).await }, |result| wrap(Msg::PasswordChanged(result))));
            }
            Msg::PasswordChanged(result) => {
                self.changing_password = false;
                match result {
                    // The server revokes every session after a password change.
                    Ok(()) => return Outcome::SignedOut("Password changed. Please sign in with your new password.".into()),
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::LoadSessions => return self.load_sessions(&client),
            Msg::Sessions(result) => {
                self.sessions_loading = false;
                match result {
                    Ok(mut sessions) => {
                        sessions.sort_by(|a, b| b.current.cmp(&a.current).then(b.last_used_at.cmp(&a.last_used_at)));
                        self.sessions = sessions;
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::Revoke(id) => {
                self.session_action = Some(id.clone());
                return Outcome::Task(Task::perform(
                    {
                        let id = id.clone();
                        async move { client.revoke_session(&id).await }
                    },
                    move |result| wrap(Msg::Revoked(id.clone(), result)),
                ));
            }
            Msg::Revoked(id, result) => {
                self.session_action = None;
                match result {
                    Ok(()) => {
                        self.sessions.retain(|session| session.id != id);
                        self.notice = Some("Session logged out".into());
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::LogoutOthers => {
                self.logging_out_others = true;
                return Outcome::Task(Task::perform(async move { client.logout_other_sessions().await }, |result| wrap(Msg::LoggedOutOthers(result))));
            }
            Msg::LoggedOutOthers(result) => {
                self.logging_out_others = false;
                match result {
                    Ok(()) => {
                        self.sessions.retain(|session| session.current);
                        self.notice = Some("All other sessions were logged out".into());
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
            Msg::PrivacyLoaded(result) => match result {
                Ok(privacy) => self.last_seen = Some(privacy.last_seen),
                Err(error) => self.error = Some(error.message),
            },
            Msg::SetLastSeen(value) => {
                if self.privacy_saving || self.last_seen.is_none() || self.last_seen.as_deref() == Some(value.as_str()) {
                    return Outcome::None;
                }
                let previous = self.last_seen.replace(value.clone());
                self.privacy_saving = true;
                self.error = None;
                self.notice = None;
                let privacy = Privacy { last_seen: value };
                return Outcome::Task(Task::perform(async move { client.update_privacy(&privacy).await }, move |result| {
                    wrap(Msg::PrivacySaved(previous.clone(), result))
                }));
            }
            Msg::PrivacySaved(previous, result) => {
                self.privacy_saving = false;
                match result {
                    Ok(privacy) => {
                        self.last_seen = Some(privacy.last_seen);
                        self.notice = Some("Privacy settings saved".into());
                    }
                    Err(error) => {
                        self.last_seen = previous;
                        self.error = Some(error.message);
                    }
                }
            }
            Msg::Notifications(enabled) => {
                context.settings.notifications = enabled;
                context.settings.save(context.dirs);
            }
            Msg::Previews(enabled) => {
                context.settings.notification_previews = enabled;
                context.settings.save(context.dirs);
            }
            Msg::AutoUpdate(enabled) => {
                context.settings.auto_update = enabled;
                context.settings.save(context.dirs);
            }
            Msg::Server(value) => self.server = value,
            Msg::ApplyServer => {
                let server = crate::api::client::normalize_server(&self.server);
                if server == context.client.server() {
                    return Outcome::None;
                }
                context.settings.server = server;
                context.settings.save(context.dirs);
                return Outcome::ServerChanged;
            }
        }
        Outcome::None
    }
}

fn tab_button<'a>(tab: Tab, active: Tab, glyph: Icon, name: &'a str) -> Element<'a, Message> {
    let selected = tab == active;
    button(
        row![
            container(widgets::icon(glyph, 17.0, if selected { theme::TEXT_1 } else { theme::TEXT_3 })).center(30).style(move |_| container::Style {
                background: Some((if selected { theme::SURFACE_3 } else { theme::SURFACE_1 }).into()),
                border: iced::Border { radius: 9.0.into(), ..Default::default() },
                ..Default::default()
            }),
            label(name, theme::TEXT_MD, if selected { theme::TEXT_1 } else { theme::TEXT_2 }).font(if selected { theme::MEDIUM } else { theme::REGULAR }),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([6, 8])
    .width(Length::Fill)
    .style(theme::tab_button(selected))
    .on_press(wrap(Msg::Tab(tab)))
    .into()
}

fn page_header<'a>(title: &'a str, subtitle: &'a str) -> Element<'a, Message> {
    column![strong(title, theme::TEXT_XL, theme::TEXT_1), label(subtitle, theme::TEXT_SM, theme::TEXT_2)].spacing(4).into()
}

fn messages<'a>(state: &'a State) -> Element<'a, Message> {
    let mut items = column![].spacing(8);
    if let Some(error) = &state.error {
        items = items.push(widgets::error_text(error.as_str()));
    }
    if let Some(notice) = &state.notice {
        items = items.push(widgets::success_text(notice.as_str()));
    }
    items.into()
}

pub fn view<'a>(state: &'a State, main: &'a Main, ctx: Ctx<'a>, app: &'a crate::app::App) -> Element<'a, Message> {
    let sidebar = column![
        tab_button(Tab::Profile, state.tab, Icon::User, "Profile"),
        tab_button(Tab::Security, state.tab, Icon::Lock, "Security"),
        tab_button(Tab::Sessions, state.tab, Icon::Device, "Active sessions"),
        tab_button(Tab::Privacy, state.tab, Icon::Shield, "Privacy"),
        tab_button(Tab::Application, state.tab, Icon::Settings, "Application"),
        widgets::vspace(),
        button(row![widgets::icon(Icon::Logout, 17.0, theme::DANGER), label("Log out", theme::TEXT_MD, theme::DANGER)].spacing(10).align_y(Alignment::Center))
            .padding([8, 10])
            .width(Length::Fill)
            .style(theme::danger_text_button)
            .on_press(Message::Logout),
    ]
    .spacing(4)
    .width(210);

    let page: Element<'a, Message> = match state.tab {
        Tab::Profile => profile(state, main, ctx),
        Tab::Security => security(state),
        Tab::Sessions => sessions(state),
        Tab::Privacy => privacy(state, main, ctx),
        Tab::Application => application(state, app),
    };

    let header = row![
        column![strong("Settings", theme::TEXT_XL, theme::TEXT_1), label(format!("@{}", main.me.username), theme::TEXT_SM, theme::TEXT_3)].spacing(2),
        hspace(),
        widgets::icon_button(Icon::Close, Some(Message::CloseDialog), 36.0),
    ]
    .align_y(Alignment::Center);

    let body = row![sidebar, widgets::vdivider(), scrollable(container(page).padding(widgets::padding(2.0, 14.0, 8.0, 4.0))).height(Length::Fill).style(theme::scroll)]
        .spacing(20)
        .height(Length::Fill);
    modal(container(column![header, widgets::divider(), body].spacing(16)).height(640).into(), 860.0, Some(Message::CloseDialog))
}

fn profile<'a>(state: &'a State, main: &'a Main, ctx: Ctx<'a>) -> Element<'a, Message> {
    let me = &main.me;
    let busy = state.uploading;
    let mut photo_actions = row![widgets::small_button(if busy { "Uploading..." } else { "Change photo" }, (!busy).then_some(wrap(Msg::PickAvatar)))].spacing(8);
    if me.avatar_url.is_some() {
        photo_actions = photo_actions.push(
            button(label("Remove photo", theme::TEXT_SM, theme::DANGER)).padding([8, 12]).style(theme::danger_text_button).on_press_maybe((!busy).then_some(wrap(Msg::RemoveAvatar))),
        );
    }
    let bio_len = state.bio.text().trim_end_matches('\n').chars().count();
    column![
        page_header("Profile", "Manage how your profile appears to other users."),
        gap(6.0),
        row![widgets::avatar(ctx, me, 84.0, false), photo_actions].spacing(18).align_y(Alignment::Center),
        gap(6.0),
        field_label("Display name"),
        text_input("Your name", &state.display_name).on_input(|v| wrap(Msg::DisplayName(v))).padding(11).style(theme::field),
        field_label("Username"),
        text_input("username", &state.username).on_input(|v| wrap(Msg::Username(v))).padding(11).style(theme::field),
        field_label("Bio"),
        text_editor(&state.bio).placeholder("Tell something about yourself").on_action(|action| wrap(Msg::Bio(action))).height(96).padding(11).style(theme::editor),
        container(label(format!("{bio_len}/160"), theme::TEXT_XS, theme::TEXT_3)).align_right(Length::Fill),
        messages(state),
        widgets::primary_button(if state.saving { "Saving..." } else { "Save changes" }, (!state.saving && !state.username.trim().is_empty()).then_some(wrap(Msg::SaveProfile))),
    ]
    .spacing(8)
    .into()
}

fn security<'a>(state: &'a State) -> Element<'a, Message> {
    let ready = !state.changing_password && !state.current_password.is_empty() && !state.new_password.is_empty() && !state.confirm_password.is_empty();
    column![
        page_header("Security", "Manage your password and account security."),
        gap(6.0),
        container(
            column![
                strong("Change password", theme::TEXT_LG, theme::TEXT_1),
                label("Changing your password will sign you out on every device.", theme::TEXT_SM, theme::TEXT_2),
                gap(4.0),
                field_label("Current password"),
                text_input("", &state.current_password).secure(true).on_input(|v| wrap(Msg::CurrentPassword(v))).padding(11).style(theme::field),
                field_label("New password"),
                text_input("", &state.new_password).secure(true).on_input(|v| wrap(Msg::NewPassword(v))).padding(11).style(theme::field),
                field_label("Confirm new password"),
                text_input("", &state.confirm_password)
                    .secure(true)
                    .on_input(|v| wrap(Msg::ConfirmPassword(v)))
                    .on_submit_maybe(ready.then_some(wrap(Msg::ChangePassword)))
                    .padding(11)
                    .style(theme::field),
                messages(state),
                widgets::primary_button(if state.changing_password { "Changing password..." } else { "Change password" }, ready.then_some(wrap(Msg::ChangePassword))),
            ]
            .spacing(8)
        )
        .padding(18)
        .style(theme::card),
        container(
            row![
                column![strong("Two-factor authentication", theme::TEXT_MD, theme::TEXT_1), label("Add another layer of protection to your account.", theme::TEXT_SM, theme::TEXT_2)].spacing(3),
                hspace(),
                coming_later(),
            ]
            .align_y(Alignment::Center)
        )
        .padding(18)
        .style(theme::card),
    ]
    .spacing(12)
    .into()
}

fn coming_later<'a>() -> Element<'a, Message> {
    container(label("Coming later", theme::TEXT_XS, theme::TEXT_3).font(theme::MEDIUM)).padding([3, 10]).style(theme::pill(theme::TEXT_3, theme::SURFACE_2)).into()
}

fn sessions<'a>(state: &'a State) -> Element<'a, Message> {
    let list: Element<'a, Message> = if state.sessions_loading && state.sessions.is_empty() {
        label("Loading sessions...", theme::TEXT_SM, theme::TEXT_3).into()
    } else if state.sessions.is_empty() {
        label("No active sessions", theme::TEXT_SM, theme::TEXT_3).into()
    } else {
        column(state.sessions.iter().map(|session| {
            let mut title = row![strong(format::device_name(session.user_agent.as_deref()), theme::TEXT_MD, theme::TEXT_1)].spacing(8).align_y(Alignment::Center);
            if session.current {
                title = title.push(container(label("This device", theme::TEXT_XS, theme::SUCCESS).font(theme::SEMIBOLD)).padding([1, 8]).style(theme::pill(theme::SUCCESS, theme::SUCCESS_SOFT)));
            }
            let mut info = column![
                title,
                label(format!("Last active: {}", if session.current { "Now".to_owned() } else { format::session_date(session.last_used_at.or(Some(session.created_at))) }), theme::TEXT_SM, theme::TEXT_2),
                label(format!("Signed in: {}", format::session_date(Some(session.created_at))), theme::TEXT_SM, theme::TEXT_2),
            ]
            .spacing(3);
            if session.remember_me {
                info = info.push(label("Remember me enabled", theme::TEXT_XS, theme::TEXT_3));
            }
            let busy = state.session_action.as_deref() == Some(session.id.as_str());
            let action: Element<'a, Message> = if session.current {
                Space::new().into()
            } else {
                widgets::small_button(if busy { "Logging out..." } else { "Log out" }, (!busy).then(|| wrap(Msg::Revoke(session.id.clone()))))
            };
            container(
                row![container(widgets::icon(Icon::Device, 20.0, theme::TEXT_2)).center(40).style(theme::pill(theme::TEXT_2, theme::SURFACE_2)), info, hspace(), action]
                    .spacing(14)
                    .align_y(Alignment::Center),
            )
            .padding(14)
            .width(Length::Fill)
            .style(theme::card)
            .into()
        }))
        .spacing(8)
        .into()
    };
    let mut page = column![
        row![
            page_header("Active sessions", "Manage devices currently signed in to your account."),
            hspace(),
            button(row![widgets::icon(Icon::Refresh, 15.0, theme::TEXT_1), label(if state.sessions_loading { "Loading…" } else { "Refresh" }, theme::TEXT_SM, theme::TEXT_1)].spacing(6).align_y(Alignment::Center))
                .padding([7, 12])
                .style(theme::secondary_button)
                .on_press_maybe((!state.sessions_loading).then_some(wrap(Msg::LoadSessions))),
        ]
        .align_y(Alignment::Center),
        messages(state),
        list,
    ]
    .spacing(12);
    if state.sessions.iter().any(|session| !session.current) {
        page = page.push(
            button(label(if state.logging_out_others { "Logging out..." } else { "Log out all other sessions" }, theme::TEXT_MD, theme::DANGER))
                .padding([10, 14])
                .style(theme::danger_text_button)
                .on_press_maybe((!state.logging_out_others).then_some(wrap(Msg::LogoutOthers))),
        );
    }
    page.into()
}

fn privacy<'a>(state: &'a State, main: &'a Main, ctx: Ctx<'a>) -> Element<'a, Message> {
    let option = |title: &'a str, detail: &'a str, control: Element<'a, Message>, enabled: bool| -> Element<'a, Message> {
        container(
            row![
                column![strong(title, theme::TEXT_MD, if enabled { theme::TEXT_1 } else { theme::TEXT_2 }).font(theme::MEDIUM), label(detail, 12.5, theme::TEXT_3)].spacing(2).width(Length::Fill),
                control
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding([14, 16])
        .width(Length::Fill)
        .into()
    };
    let choice = |value: &'static str, name: &'a str| -> Element<'a, Message> {
        let active = state.last_seen.as_deref() == Some(value);
        let enabled = state.last_seen.is_some() && !state.privacy_saving;
        let color = if active { theme::ACCENT } else if enabled { theme::TEXT_2 } else { iced::Color { a: 0.6, ..theme::TEXT_2 } };
        button(container(label(name, theme::TEXT_SM, color).font(theme::MEDIUM)).center_y(30))
            .padding([0, 14])
            .style(theme::segment(active))
            .on_press_maybe(enabled.then(|| wrap(Msg::SetLastSeen(value.to_owned()))))
            .into()
    };
    let last_seen_detail = if state.last_seen.as_deref() == Some("nobody") {
        "Hidden. You also won't see when others were last online."
    } else {
        "People you chat with can see when you were last online."
    };
    let options = column![
        option("Last seen", last_seen_detail, container(row![choice("everyone", "Everyone"), choice("nobody", "Nobody")].spacing(2)).padding(3).style(theme::segmented).into(), true),
        widgets::divider(),
        option("Online status", "Control who can see when you are online.", coming_later(), false),
        widgets::divider(),
        option("Read receipts", "Choose whether other users can see when you read their messages.", coming_later(), false),
        widgets::divider(),
        option("Profile photo", "Choose who can see your profile picture.", coming_later(), false),
    ];

    let blocked: Element<'a, Message> = match &main.blocked {
        None => label("Loading blocked users...", theme::TEXT_SM, theme::TEXT_3).into(),
        Some(users) if users.is_empty() => label("You haven't blocked anyone.", theme::TEXT_SM, theme::TEXT_3).into(),
        Some(users) => {
            let mut list = column![];
            for (index, user) in users.iter().enumerate() {
                if index > 0 {
                    list = list.push(widgets::divider());
                }
                let busy = main.block_busy.is_some();
                let unblock = button(label(if main.block_busy.as_deref() == Some(user.id.as_str()) { "Unblocking..." } else { "Unblock" }, theme::TEXT_SM, theme::ACCENT).font(theme::MEDIUM))
                    .padding([8, 14])
                    .style(theme::accent_outline_button)
                    .on_press_maybe((!busy).then(|| Message::SetBlocked(user.clone(), false)));
                list = list.push(
                    container(
                        row![
                            widgets::avatar(ctx, user, 36.0, false),
                            column![strong(user.name(), theme::TEXT_MD, theme::TEXT_1).font(theme::MEDIUM), label(format!("@{}", user.username), theme::TEXT_SM, theme::TEXT_3)].spacing(1).width(Length::Fill),
                            unblock,
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .padding([10, 16]),
                );
            }
            container(list).width(Length::Fill).style(theme::panel).into()
        }
    };

    column![
        page_header("Privacy", "Control what other users can see and how they can interact with you."),
        gap(6.0),
        messages(state),
        container(options).width(Length::Fill).style(theme::panel),
        gap(12.0),
        column![
            strong("Blocked users", theme::TEXT_MD, theme::TEXT_1).font(theme::MEDIUM),
            label(
                "Blocked users can't message or call you, and you won't see each other's online status or last seen. To block someone, open their profile in a chat.",
                12.5,
                theme::TEXT_3
            )
            .width(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill),
        blocked,
    ]
    .spacing(10)
    .into()
}

fn switch_row<'a>(title: &'a str, detail: &'a str, value: bool, on_toggle: fn(bool) -> Msg) -> Element<'a, Message> {
    container(
        row![
            column![strong(title, theme::TEXT_MD, theme::TEXT_1), label(detail, theme::TEXT_SM, theme::TEXT_2)].spacing(3).width(Length::Fill),
            toggler(value).on_toggle(move |value| wrap(on_toggle(value))).size(22),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding(16)
    .width(Length::Fill)
    .style(theme::card)
    .into()
}

fn application<'a>(state: &'a State, app: &'a crate::app::App) -> Element<'a, Message> {
    let settings = &app.settings;
    let server_changed = crate::api::client::normalize_server(&state.server) != app.client.server();
    column![
        page_header("Application", "Desktop notifications, updates and connection."),
        gap(6.0),
        switch_row("Desktop notifications", "Show a Windows notification for new messages when the app is in the background.", settings.notifications, Msg::Notifications),
        switch_row("Message previews", "Include the message text in notifications.", settings.notification_previews, Msg::Previews),
        switch_row("Automatic updates", "Check for new versions and download them in the background.", settings.auto_update, Msg::AutoUpdate),
        crate::update::settings_card(&app.updater),
        container(
            column![
                strong("Server", theme::TEXT_MD, theme::TEXT_1),
                label("Address of your Messenger server. Changing it signs you out.", theme::TEXT_SM, theme::TEXT_2),
                row![
                    text_input("https://messenger.example", &state.server).on_input(|v| wrap(Msg::Server(v))).on_submit(wrap(Msg::ApplyServer)).padding(10).style(theme::field),
                    widgets::small_button("Apply", server_changed.then_some(wrap(Msg::ApplyServer))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            ]
            .spacing(8)
        )
        .padding(16)
        .style(theme::card),
        label(format!("Messenger Desktop {} · {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS), theme::TEXT_XS, theme::TEXT_3),
    ]
    .spacing(10)
    .into()
}
