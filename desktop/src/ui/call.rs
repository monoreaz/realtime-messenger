//! Floating call card (`AudioCallDialog.tsx`): incoming, calling, connecting, in call.

use iced::widget::{button, column, container, row};
use iced::{Alignment, Color, Element, Length};

use super::icons::Icon;
use super::theme;
use super::widgets::{self, Ctx, label, strong};
use crate::app::{Main, Message};
use crate::call::{ActiveCall, Phase};

fn round(glyph: Icon, background: Color, on_press: Message, size: f32) -> Element<'static, Message> {
    button(container(widgets::icon(glyph, size * 0.42, Color::WHITE)).center(size))
        .padding(0)
        .style(move |_, status| {
            let hover = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Color { a: if hover { 0.85 } else { 1.0 }, ..background }.into()),
                text_color: Color::WHITE,
                border: iced::Border { radius: 999.0.into(), ..Default::default() },
                ..Default::default()
            }
        })
        .on_press(on_press)
        .into()
}

pub fn card<'a>(main: &'a Main, call: &'a ActiveCall, ctx: Ctx<'a>) -> Element<'a, Message> {
    let peer = main.chats.iter().filter_map(|chat| chat.peer.as_ref()).find(|peer| peer.id == call.peer_id);
    let avatar = match peer {
        Some(user) => widgets::avatar(ctx, user, 52.0, false),
        None => widgets::avatar_inner(ctx, &call.peer_id, &call.name, None, 52.0),
    };
    let status = match call.phase {
        Phase::Incoming => "Incoming audio call".to_owned(),
        Phase::Dialing => "Calling…".to_owned(),
        Phase::Connecting => "Connecting…".to_owned(),
        Phase::Connected => call.elapsed().unwrap_or_else(|| "Connected".into()),
    };
    let green = theme::rgb(0x2f, 0xa8, 0x6a);
    let red = theme::DANGER_STRONG;
    let actions = match call.phase {
        Phase::Incoming => row![round(Icon::Close, red, Message::EndCall, 46.0), round(Icon::Phone, green, Message::AcceptCall, 46.0)],
        _ => row![
            round(if call.muted { Icon::MicOff } else { Icon::Mic }, if call.muted { theme::rgb(0x55, 0x55, 0x5c) } else { theme::rgb(0x3a, 0x3a, 0x40) }, Message::ToggleMute, 44.0),
            round(Icon::Phone, red, Message::EndCall, 44.0),
        ],
    }
    .spacing(12)
    .align_y(Alignment::Center);
    let status_color = if call.phase == Phase::Connected { theme::SUCCESS } else { theme::TEXT_2 };
    let mut info = column![strong(crate::format::ellipsize(&call.name, 28), theme::TEXT_LG, theme::TEXT_1), label(status, theme::TEXT_SM, status_color)].spacing(2);
    if call.muted {
        info = info.push(label("Microphone muted", theme::TEXT_XS, theme::WARNING));
    }
    let content = row![avatar, info.width(Length::Fill), actions].spacing(14).align_y(Alignment::Center);
    let card = container(content).padding([12, 16]).width(440).style(theme::menu);
    container(iced::widget::opaque(card)).center_x(Length::Fill).padding(widgets::padding(14.0, 0.0, 0.0, 0.0)).into()
}
