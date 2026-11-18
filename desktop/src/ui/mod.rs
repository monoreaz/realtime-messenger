//! Views. Layout mirrors the web app: rail · chat list · conversation, with dialogs,
//! menus and toasts layered on top.

pub mod auth;
pub mod backdrop;
pub mod call;
pub mod chat;
pub mod icons;
pub mod settings;
pub mod sidebar;
pub mod smooth;
pub mod theme;
pub mod widgets;

use std::sync::LazyLock;

use iced::widget::{button, container, image, row, stack};
use iced::{Alignment, ContentFit, Element, Length};

use crate::app::{App, Message, Screen, Toast};
use widgets::{Ctx, label};

/// Below this window width only one of chat list / conversation is shown.
const NARROW: f32 = 760.0;

static BRAND: LazyLock<image::Handle> = LazyLock::new(|| image::Handle::from_bytes(include_bytes!("../../assets/brand-mark.webp").as_slice()));

pub fn brand_handle() -> image::Handle {
    BRAND.clone()
}

fn background<'a>(handle: image::Handle) -> Element<'a, Message> {
    image(handle).width(Length::Fill).height(Length::Fill).content_fit(ContentFit::Fill).into()
}

pub fn view(app: &App) -> Element<'_, Message> {
    let content: Element<'_, Message> = match &app.screen {
        Screen::Loading => auth::loading(),
        Screen::Auth(state) => auth::view(state),
        Screen::Main(main) => {
            let ctx = Ctx { client: &app.client, media: &main.media };
            let window = app.window_size;
            let frame = backdrop::Frame::for_window(window);
            let width = frame.rect.width;
            let narrow = width < NARROW;
            // Web --sidebar-width: clamp(290px, 25vw, 360px); 300px up to 1100px, 270px up to 900px.
            let sidebar_width = if narrow {
                (width - theme::RAIL_WIDTH).max(240.0)
            } else if window.width > 1100.0 {
                (window.width * 0.25).clamp(290.0, 360.0)
            } else if window.width > 900.0 {
                300.0
            } else {
                270.0
            };
            let chat_width = if narrow { width } else { width - theme::RAIL_WIDTH - sidebar_width - 2.0 };
            let layout: Element<'_, Message> = if narrow && main.active.is_some() {
                chat::view(app, main, ctx, chat_width, true, 0.0)
            } else if narrow {
                row![sidebar::rail(main, ctx, 0.0), widgets::vdivider(), sidebar::view(app, main, ctx, sidebar_width)].into()
            } else {
                row![
                    sidebar::rail(main, ctx, frame.radius),
                    widgets::vdivider(),
                    sidebar::view(app, main, ctx, sidebar_width),
                    widgets::vdivider(),
                    chat::view(app, main, ctx, chat_width, false, frame.radius)
                ]
                .into()
            };
            // The composed backdrop (scene + glass window) lags a resize by a moment; until it
            // arrives the previous one is stretched.
            let glass = app.backdrop.as_ref().map(|(_, handle)| handle.clone()).unwrap_or_else(|| backdrop::SCENE.clone());
            let window_layer = container(layout).padding(frame.padding(window));
            let mut layers = stack![background(glass), window_layer].width(Length::Fill).height(Length::Fill);
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
    let content = match &app.screen {
        Screen::Main(_) => content,
        _ => stack![background(backdrop::SCENE.clone()), content].into(),
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
