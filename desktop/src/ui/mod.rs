//! Views. Layout mirrors the web app: rail · chat list · conversation, with dialogs,
//! menus and toasts layered on top.

pub mod auth;
pub mod call;
pub mod chat;
pub mod icons;
pub mod settings;
pub mod sidebar;
pub mod theme;
pub mod widgets;

use std::sync::LazyLock;

use iced::widget::{button, container, image, row, stack};
use iced::{Alignment, Element, Length};

use crate::app::{App, Message, Screen, Toast};
use widgets::{Ctx, label};

/// Below this window width only one of chat list / conversation is shown.
const NARROW: f32 = 760.0;

static BRAND: LazyLock<image::Handle> = LazyLock::new(|| image::Handle::from_bytes(include_bytes!("../../assets/brand-mark.webp").as_slice()));

pub fn brand_handle() -> image::Handle {
    BRAND.clone()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match &app.screen {
        Screen::Loading => auth::loading(),
        Screen::Auth(state) => auth::view(state),
        Screen::Main(main) => {
            let ctx = Ctx { client: &app.client, media: &main.media };
            let width = app.window_size.width;
            let narrow = width < NARROW;
            let sidebar_width = if narrow { (width - theme::RAIL_WIDTH).max(240.0) } else { (width * 0.25).clamp(290.0, 360.0) };
            let chat_width = if narrow { width } else { width - theme::RAIL_WIDTH - sidebar_width - 2.0 };
            let layout: Element<'_, Message> = if narrow && main.active.is_some() {
                chat::view(app, main, ctx, chat_width, true)
            } else if narrow {
                row![sidebar::rail(main, ctx), widgets::vdivider(), sidebar::view(app, main, ctx, sidebar_width)].into()
            } else {
                row![
                    sidebar::rail(main, ctx),
                    widgets::vdivider(),
                    sidebar::view(app, main, ctx, sidebar_width),
                    widgets::vdivider(),
                    chat::view(app, main, ctx, chat_width, false)
                ]
                .into()
            };
            let mut layers = stack![layout].width(Length::Fill).height(Length::Fill);
            if let Some(menu) = &main.context_menu {
                layers = layers.push(chat::context_menu(main, menu, app.window_size, &app.client));
            }
            if let Some(call) = &main.call {
                layers = layers.push(call::card(main, call, ctx));
            }
            if let Some(dialog) = &main.dialog {
                layers = layers.push(crate::dialogs::view(dialog, main, ctx, app));
            }
            if let Some(toast) = &main.toast {
                layers = layers.push(toast_view(toast));
            }
            layers.into()
        }
    };
    container(content).width(Length::Fill).height(Length::Fill).style(theme::shell).into()
}

fn toast_view(toast: &Toast) -> Element<'_, Message> {
    let card = container(
        row![
            label(toast.text.as_str(), theme::TEXT_SM, if toast.error { theme::rgb(0xff, 0xd7, 0xd5) } else { theme::TEXT_1 }).width(Length::Shrink),
            button(widgets::icon(icons::Icon::Close, 14.0, theme::TEXT_2)).padding(4).style(theme::icon_button).on_press(Message::DismissToast),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .max_width(520)
    .style(theme::toast(toast.error));
    container(iced::widget::opaque(card)).align_bottom(Length::Fill).center_x(Length::Fill).padding(28).into()
}
