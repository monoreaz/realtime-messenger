//! Sign-in / registration / password reset screen (`auth-page` in the web app).
//! Email verification and password-reset links open in the browser, exactly as
//! they do for web users; the desktop app sends the requests.

use iced::widget::{Space, button, checkbox, column, container, image, row, text, text_input};
use iced::{Alignment, Element, Length};

use super::icons::Icon;
use super::theme;
use super::widgets::{self, field_label, gap, hspace, label};
use crate::api::{ApiError, Client};
use crate::app::Message;
use crate::storage::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Login,
    Register,
    Forgot,
}

pub struct State {
    pub mode: Mode,
    pub username: String,
    pub email: String,
    pub password: String,
    pub confirm: String,
    pub remember: bool,
    pub show_password: bool,
    pub server: String,
    pub show_server: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub unverified: bool,
    pub can_retry_restore: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Mode(Mode),
    Username(String),
    Email(String),
    Password(String),
    Confirm(String),
    Remember(bool),
    TogglePassword,
    Server(String),
    ToggleServer,
    Submit,
    Resend,
    RetryRestore,
    Done(Result<String, ApiError>),
}

#[derive(Debug, Clone)]
pub enum Request {
    Register { username: String, email: String, password: String },
    Forgot { email: String },
    Resend { email: String },
}

impl Request {
    pub async fn run(self, client: &Client) -> Result<String, ApiError> {
        match self {
            Request::Register { username, email, password } => {
                client.register(&username, &email, &password).await?;
                Ok("Account created. Check your email for a verification link, then sign in.".into())
            }
            Request::Forgot { email } => {
                client.forgot_password(&email).await?;
                Ok("If an account with this email exists, a password reset link has been sent. Open it to choose a new password.".into())
            }
            Request::Resend { email } => {
                client.resend_verification(&email).await?;
                Ok("Verification email sent. Open the link in it, then sign in.".into())
            }
        }
    }
}

pub enum Action {
    None,
    Login { server: String, username: String, password: String, remember: bool },
    Request { server: String, request: Request },
    RetryRestore,
    OpenUrl(String),
}

impl State {
    pub fn new(settings: &Settings) -> Self {
        Self {
            mode: Mode::Login,
            username: settings.last_username.clone(),
            email: String::new(),
            password: String::new(),
            confirm: String::new(),
            remember: true,
            show_password: false,
            server: settings.server.clone(),
            show_server: false,
            busy: false,
            error: None,
            notice: None,
            unverified: false,
            can_retry_restore: false,
        }
    }

    pub fn update(&mut self, msg: Msg) -> Action {
        match msg {
            Msg::Mode(mode) => {
                self.mode = mode;
                self.error = None;
                self.notice = None;
                self.unverified = false;
                self.password.clear();
                self.confirm.clear();
            }
            Msg::Username(value) => self.username = value.trim().to_owned(),
            Msg::Email(value) => self.email = value.trim().to_owned(),
            Msg::Password(value) => self.password = value,
            Msg::Confirm(value) => self.confirm = value,
            Msg::Remember(value) => self.remember = value,
            Msg::TogglePassword => self.show_password = !self.show_password,
            Msg::Server(value) => self.server = value,
            Msg::ToggleServer => self.show_server = !self.show_server,
            Msg::RetryRestore => return Action::RetryRestore,
            Msg::Resend => {
                let email = if self.email.contains('@') { self.email.clone() } else if self.username.contains('@') { self.username.clone() } else { String::new() };
                if email.is_empty() {
                    self.mode = Mode::Register;
                    self.error = Some("Enter the email address you registered with.".into());
                    return Action::None;
                }
                self.busy = true;
                return Action::Request { server: self.server.clone(), request: Request::Resend { email } };
            }
            Msg::Submit => {
                if self.busy {
                    return Action::None;
                }
                self.error = None;
                self.notice = None;
                match self.mode {
                    Mode::Login => {
                        if self.username.is_empty() || self.password.is_empty() {
                            return Action::None;
                        }
                        self.busy = true;
                        self.unverified = false;
                        return Action::Login { server: self.server.clone(), username: self.username.clone(), password: self.password.clone(), remember: self.remember };
                    }
                    Mode::Register => {
                        if let Some(error) = validate_registration(&self.username, &self.email, &self.password, &self.confirm) {
                            self.error = Some(error);
                            return Action::None;
                        }
                        self.busy = true;
                        return Action::Request {
                            server: self.server.clone(),
                            request: Request::Register { username: self.username.to_lowercase(), email: self.email.clone(), password: self.password.clone() },
                        };
                    }
                    Mode::Forgot => {
                        if !self.email.contains('@') {
                            self.error = Some("Enter a valid email address.".into());
                            return Action::None;
                        }
                        self.busy = true;
                        return Action::Request { server: self.server.clone(), request: Request::Forgot { email: self.email.clone() } };
                    }
                }
            }
            Msg::Done(result) => {
                self.busy = false;
                match result {
                    Ok(notice) => {
                        if self.mode == Mode::Register {
                            self.mode = Mode::Login;
                            self.password.clear();
                            self.confirm.clear();
                        }
                        self.notice = Some(notice);
                        self.unverified = false;
                    }
                    Err(error) => self.error = Some(error.message),
                }
            }
        }
        Action::None
    }

    pub fn set_login_error(&mut self, message: String) {
        self.unverified = message.contains("not verified");
        self.error = Some(message);
    }
}

pub fn validate_registration(username: &str, email: &str, password: &str, confirm: &str) -> Option<String> {
    if !(3..=32).contains(&username.len()) || !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some("Username must be 3–32 letters, numbers or underscores.".into());
    }
    if !email.contains('@') || email.len() < 3 {
        return Some("Enter a valid email address.".into());
    }
    if password.chars().count() < 8 {
        return Some("Password must be at least 8 characters.".into());
    }
    if password != confirm {
        return Some("Passwords do not match.".into());
    }
    None
}

fn wrap(msg: Msg) -> Message {
    Message::Auth(msg)
}

pub fn brand_mark<'a>(size: f32) -> Element<'a, Message> {
    image(super::brand_handle()).width(size).height(size).border_radius(size * 0.24).into()
}

fn password_field<'a>(placeholder: &'a str, value: &'a str, visible: bool, on_input: fn(String) -> Msg, submit: bool) -> Element<'a, Message> {
    let input = text_input(placeholder, value)
        .secure(!visible)
        .on_input(move |v| wrap(on_input(v)))
        .on_submit_maybe(submit.then_some(wrap(Msg::Submit)))
        .padding(12)
        .style(theme::bare_field);
    let toggle = button(widgets::icon(if visible { Icon::EyeOff } else { Icon::Eye }, 18.0, theme::TEXT_3))
        .padding(8)
        .style(theme::icon_button)
        .on_press(wrap(Msg::TogglePassword));
    container(row![input, toggle].align_y(Alignment::Center).padding(widgets::padding(0.0, 4.0, 0.0, 0.0)))
        .style(theme::sunken_field(false))
        .into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let busy = state.busy;
    let subtitle = match state.mode {
        Mode::Login => "Sign in to continue",
        Mode::Register => "Create your account",
        Mode::Forgot => "Reset your password",
    };
    let mut form = column![
        container(brand_mark(64.0)).center_x(Length::Fill),
        gap(6.0),
        container(text("Messenger").size(theme::TEXT_2XL).font(theme::SEMIBOLD).color(theme::TEXT_1)).center_x(Length::Fill),
        container(label(subtitle, theme::TEXT_MD, theme::TEXT_2)).center_x(Length::Fill),
        gap(14.0),
    ]
    .spacing(4);

    match state.mode {
        Mode::Login => {
            form = form
                .push(field_label("Username or email"))
                .push(text_input("username", &state.username).on_input(|v| wrap(Msg::Username(v))).on_submit(wrap(Msg::Submit)).padding(12).style(theme::field))
                .push(gap(6.0))
                .push(field_label("Password"))
                .push(password_field("Password", &state.password, state.show_password, Msg::Password, true))
                .push(gap(6.0))
                .push(
                    row![
                        checkbox(state.remember).label("Remember me").on_toggle(|v| wrap(Msg::Remember(v))).size(17).text_size(theme::TEXT_SM),
                        hspace(),
                        widgets::link("Forgot password?", Some(wrap(Msg::Mode(Mode::Forgot))))
                    ]
                    .align_y(Alignment::Center),
                );
        }
        Mode::Register => {
            form = form
                .push(field_label("Username"))
                .push(text_input("username", &state.username).on_input(|v| wrap(Msg::Username(v))).padding(12).style(theme::field))
                .push(gap(6.0))
                .push(field_label("Email"))
                .push(text_input("you@example.com", &state.email).on_input(|v| wrap(Msg::Email(v))).padding(12).style(theme::field))
                .push(gap(6.0))
                .push(field_label("Password"))
                .push(password_field("At least 8 characters", &state.password, state.show_password, Msg::Password, false))
                .push(gap(6.0))
                .push(field_label("Confirm password"))
                .push(password_field("Repeat password", &state.confirm, state.show_password, Msg::Confirm, true));
        }
        Mode::Forgot => {
            form = form
                .push(label("Enter the email address of your account and we will send you a link to reset your password.", theme::TEXT_SM, theme::TEXT_2))
                .push(gap(6.0))
                .push(field_label("Email"))
                .push(text_input("you@example.com", &state.email).on_input(|v| wrap(Msg::Email(v))).on_submit(wrap(Msg::Submit)).padding(12).style(theme::field));
        }
    }

    form = form.push(gap(10.0));
    if let Some(error) = &state.error {
        form = form.push(widgets::error_text(error.as_str()));
        if state.unverified {
            form = form.push(widgets::link("Resend verification email", (!busy).then_some(wrap(Msg::Resend))));
        }
        if state.can_retry_restore {
            form = form.push(widgets::link("Retry restoring my session", (!busy).then_some(wrap(Msg::RetryRestore))));
        }
        form = form.push(gap(6.0));
    }
    if let Some(notice) = &state.notice {
        form = form.push(widgets::success_text(notice.as_str())).push(gap(6.0));
    }

    let submit = match (state.mode, busy) {
        (Mode::Login, false) => "Sign in",
        (Mode::Login, true) => "Signing in...",
        (Mode::Register, false) => "Create account",
        (Mode::Register, true) => "Creating account...",
        (Mode::Forgot, false) => "Send reset link",
        (Mode::Forgot, true) => "Sending...",
    };
    let ready = !busy
        && match state.mode {
            Mode::Login => !state.username.is_empty() && !state.password.is_empty(),
            Mode::Register => !state.username.is_empty() && !state.email.is_empty() && !state.password.is_empty(),
            Mode::Forgot => !state.email.is_empty(),
        };
    form = form.push(widgets::primary_button(submit, ready.then_some(wrap(Msg::Submit))));

    let links: Element<'_, Message> = match state.mode {
        Mode::Login => row![label("No account?", theme::TEXT_SM, theme::TEXT_3), widgets::link("Create one", Some(wrap(Msg::Mode(Mode::Register))))].align_y(Alignment::Center).into(),
        _ => row![label("Already have an account?", theme::TEXT_SM, theme::TEXT_3), widgets::link("Sign in", Some(wrap(Msg::Mode(Mode::Login))))].align_y(Alignment::Center).into(),
    };
    form = form.push(gap(6.0)).push(container(links).center_x(Length::Fill));

    let server_toggle = button(
        row![widgets::icon(Icon::Settings, 14.0, theme::TEXT_3), label(if state.show_server { "Hide server settings" } else { "Server settings" }, theme::TEXT_XS, theme::TEXT_3)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::link_button)
    .on_press(wrap(Msg::ToggleServer));
    form = form.push(gap(8.0)).push(container(server_toggle).center_x(Length::Fill));
    if state.show_server {
        form = form
            .push(field_label("Server address"))
            .push(text_input("https://messenger.example", &state.server).on_input(|v| wrap(Msg::Server(v))).padding(10).style(theme::field))
            .push(label("Requires Tailscale when the server is on a tailnet.", theme::TEXT_XS, theme::TEXT_3));
    }

    let card = container(form).width(400).padding(32).style(theme::float);
    container(iced::widget::scrollable(container(card).center_x(Length::Fill).padding(24)).style(theme::scroll))
        .center(Length::Fill)
        .into()
}

pub fn loading<'a>() -> Element<'a, Message> {
    container(column![brand_mark(72.0), label("Loading Messenger…", theme::TEXT_MD, theme::TEXT_2)].spacing(16).align_x(Alignment::Center))
        .center(Length::Fill)
        .into()
}

#[allow(dead_code)]
fn spacer<'a>() -> Element<'a, Message> {
    Space::new().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_validation() {
        assert!(validate_registration("ab", "a@b.c", "password1", "password1").is_some());
        assert!(validate_registration("anna", "nope", "password1", "password1").is_some());
        assert!(validate_registration("anna", "a@b.c", "short", "short").is_some());
        assert!(validate_registration("anna", "a@b.c", "password1", "password2").is_some());
        assert!(validate_registration("anna_1", "a@b.c", "password1", "password1").is_none());
    }

    #[test]
    fn login_submit_requires_fields() {
        let mut state = State::new(&Settings::default());
        state.username.clear();
        assert!(matches!(state.update(Msg::Submit), Action::None));
        state.update(Msg::Username("ann".into()));
        state.update(Msg::Password("secret123".into()));
        assert!(matches!(state.update(Msg::Submit), Action::Login { .. }));
        assert!(state.busy);
        assert!(matches!(state.update(Msg::Submit), Action::None));
    }
}
