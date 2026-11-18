//! Small reusable building blocks shared by all screens.

use iced::widget::{Space, button, column, container, mouse_area, opaque, row, sensor, stack, svg, text};
use iced::{Alignment, Color, Element, Length, Padding, mouse};

use super::icons::Icon;
use super::theme;
use crate::api::Client;
use crate::api::models::User;
use crate::app::Message;
use crate::format;
use crate::media::{Cache, Key, Size};

/// What views need besides their own state.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub client: &'a Client,
    pub media: &'a Cache,
}

pub fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    svg(icon.handle())
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// Text in the default UI font.
pub fn label<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> text::Text<'a> {
    text(content).size(size).color(color).font(theme::REGULAR)
}

pub fn strong<'a>(content: impl text::IntoFragment<'a>, size: f32, color: Color) -> text::Text<'a> {
    text(content).size(size).color(color).font(theme::SEMIBOLD)
}

pub fn icon_button<'a>(glyph: Icon, on_press: Option<Message>, tooltip_size: f32) -> Element<'a, Message> {
    let size = tooltip_size;
    button(container(icon(glyph, 20.0, theme::TEXT_2)).center(size))
        .padding(0)
        .width(size)
        .height(size)
        .style(theme::icon_button)
        .on_press_maybe(on_press)
        .into()
}

/// Icon button whose glyph follows the button's text colour (hover brightens it).
pub fn rail_button<'a>(glyph: Icon, on_press: Message) -> Element<'a, Message> {
    button(
        container(
            svg(glyph.handle()).width(21).height(21).style(|_, status| svg::Style {
                color: Some(if status == svg::Status::Hovered { theme::TEXT_1 } else { theme::TEXT_3 }),
            }),
        )
        .center(42),
    )
    .padding(0)
    .width(42)
    .height(42)
    .style(theme::icon_button)
    .on_press(on_press)
    .into()
}

pub fn primary_button<'a>(content: impl text::IntoFragment<'a>, on_press: Option<Message>) -> Element<'a, Message> {
    button(container(strong(content, theme::TEXT_MD, theme::PRIMARY_TEXT).center()).center_x(Length::Fill))
        .padding([11, 18])
        .width(Length::Fill)
        .style(theme::primary_button)
        .on_press_maybe(on_press)
        .into()
}

pub fn small_button<'a>(content: impl text::IntoFragment<'a>, on_press: Option<Message>) -> Element<'a, Message> {
    button(label(content, theme::TEXT_SM, theme::TEXT_1)).padding([8, 14]).style(theme::secondary_button).on_press_maybe(on_press).into()
}

pub fn small_primary<'a>(content: impl text::IntoFragment<'a>, on_press: Option<Message>) -> Element<'a, Message> {
    button(strong(content, theme::TEXT_SM, theme::PRIMARY_TEXT)).padding([8, 14]).style(theme::primary_button).on_press_maybe(on_press).into()
}

pub fn small_danger<'a>(content: impl text::IntoFragment<'a>, on_press: Option<Message>) -> Element<'a, Message> {
    button(strong(content, theme::TEXT_SM, Color::WHITE)).padding([8, 14]).style(theme::danger_button).on_press_maybe(on_press).into()
}

pub fn link<'a>(content: impl text::IntoFragment<'a>, on_press: Option<Message>) -> Element<'a, Message> {
    button(text(content).size(theme::TEXT_SM).font(theme::MEDIUM)).padding([4, 6]).style(theme::link_button).on_press_maybe(on_press).into()
}

pub fn field_label<'a>(content: &'a str) -> Element<'a, Message> {
    label(content, theme::TEXT_SM, theme::TEXT_2).font(theme::MEDIUM).into()
}

pub fn error_text<'a>(content: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(label(content, theme::TEXT_SM, theme::DANGER))
        .padding([8, 12])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::DANGER_SOFT.into()),
            border: iced::Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

pub fn success_text<'a>(content: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(label(content, theme::TEXT_SM, theme::SUCCESS))
        .padding([8, 12])
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(theme::SUCCESS_SOFT.into()),
            border: iced::Border { radius: 10.0.into(), ..Default::default() },
            ..Default::default()
        })
        .into()
}

pub fn hspace() -> Space {
    iced::widget::space::horizontal()
}

pub fn vspace() -> Space {
    iced::widget::space::vertical()
}

pub fn gap(height: f32) -> Space {
    iced::widget::space().height(height)
}

pub fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(1)).width(Length::Fill).height(1).style(theme::divider).into()
}

pub fn vdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(1)).width(1).height(Length::Fill).style(theme::divider).into()
}

/// Round avatar: photo if available (loaded lazily), otherwise initials on a hue gradient.
pub fn avatar<'a>(ctx: Ctx<'a>, user: &User, size: f32, online: bool) -> Element<'a, Message> {
    let base = avatar_inner(ctx, &user.id, user.name(), user.avatar_url.as_deref(), size);
    with_presence(base, size, online)
}

pub fn avatar_inner<'a>(ctx: Ctx<'a>, seed: &str, name: &str, avatar_url: Option<&str>, size: f32) -> Element<'a, Message> {
    let initials = container(text(format::initials(name)).size((size * 0.34).round()).font(theme::SEMIBOLD))
        .center(size)
        .style(theme::avatar(theme::hue_for(seed), false));
    let Some(path) = avatar_url else { return initials.into() };
    let key = Key::new(ctx.client.media_url(path), Size::Avatar);
    match ctx.media.get(&key) {
        Some(image) => iced::widget::image(image.handle.clone())
            .width(size)
            .height(size)
            .content_fit(iced::ContentFit::Cover)
            .border_radius(size / 2.0)
            .into(),
        None if ctx.media.failed(&key) => initials.into(),
        None => sensor(initials).on_show(move |_| Message::MediaVisible(key.clone())).into(),
    }
}

pub fn with_presence<'a>(base: Element<'a, Message>, size: f32, online: bool) -> Element<'a, Message> {
    if !online {
        return base;
    }
    let dot = (size * 0.24).max(9.0);
    stack![
        base,
        container(container(Space::new()).width(dot).height(dot).style(theme::presence_dot))
            .width(size)
            .height(size)
            .align_right(size)
            .align_bottom(size)
    ]
    .width(size)
    .height(size)
    .into()
}

pub fn group_avatar<'a>(seed: &str, size: f32) -> Element<'a, Message> {
    let hue = theme::hue_for(seed);
    container(icon(Icon::Group, size * 0.46, theme::hsla(hue, 0.22, 0.88, 1.0)))
        .center(size)
        .style(theme::avatar(hue, true))
        .into()
}

/// Centered modal card over a dimmed backdrop; clicking the backdrop sends `on_close`.
pub fn modal<'a>(content: Element<'a, Message>, width: f32, on_close: Option<Message>) -> Element<'a, Message> {
    let card = container(content).width(width).max_height(820).padding(22).style(theme::float);
    let backdrop = mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill).style(theme::backdrop));
    let backdrop = match on_close {
        Some(message) => backdrop.on_press(message),
        None => backdrop,
    };
    stack![backdrop, container(opaque(card)).center(Length::Fill).padding(24)].into()
}

pub fn dialog_header<'a>(title: impl text::IntoFragment<'a>, on_close: Option<Message>) -> Element<'a, Message> {
    row![
        text(title).size(theme::TEXT_XL).font(theme::SEMIBOLD).color(theme::TEXT_1),
        hspace(),
        icon_button(Icon::Close, on_close, 36.0),
    ]
    .align_y(Alignment::Center)
    .into()
}

pub fn pointer<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    mouse_area(content).interaction(mouse::Interaction::Pointer).into()
}

pub fn padding(top: f32, right: f32, bottom: f32, left: f32) -> Padding {
    Padding { top, right, bottom, left }
}

pub fn info_row<'a>(name: &'a str, value: Element<'a, Message>) -> Element<'a, Message> {
    column![label(name, theme::TEXT_XS, theme::TEXT_3).font(theme::MEDIUM), value].spacing(3).into()
}

/// Stable `Copy` key for keyed columns.
pub fn key(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}
