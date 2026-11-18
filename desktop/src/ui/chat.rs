//! Conversation panel: header, pinned bar, message list and composer.

use chrono::Utc;
use iced::advanced::widget::operation::scrollable::AbsoluteOffset;
use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::widget::{Id, Space, button, column, container, mouse_area, row, scrollable, sensor, stack, text, text_input};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Task, Vector};

use super::icons::Icon;
use super::theme;
use super::widgets::{self, Ctx, hspace, label, strong};
use crate::api::client::MediaKind;
use crate::api::models::{Chat, Message as ChatMessage, User};
use crate::app::{App, ContextMenu, Main, Message, Thread, WsStatus, composer_input, messages_scroll};
use crate::dialogs::Open;
use crate::format;
use crate::media::{Key, Size};

const RUN_GAP_SECONDS: i64 = 5 * 60;

fn message_id(id: &str) -> Id {
    Id::from(format!("msg-{id}"))
}

pub fn view<'a>(app: &'a App, main: &'a Main, ctx: Ctx<'a>, width: f32, narrow: bool) -> Element<'a, Message> {
    let Some(chat) = main.active.as_ref().and_then(|id| main.chats.iter().find(|chat| &chat.id == id)) else {
        return empty_state();
    };
    let thread = main.threads.get(&chat.id);
    let mut panel = column![header(main, chat, ctx, narrow), widgets::divider()];
    if let Some(thread) = thread
        && !thread.pins.is_empty()
    {
        panel = panel.push(pinned_bar(main, chat, thread)).push(widgets::divider());
    }
    panel = panel.push(messages(app, main, chat, thread, ctx, width)).push(composer(main, chat, width));
    container(panel).width(Length::Fill).height(Length::Fill).into()
}

fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            super::auth::brand_mark(76.0),
            text("Your conversations").size(theme::TEXT_XL).font(theme::SEMIBOLD).color(theme::TEXT_1),
            label("Choose a chat from the list, or search for someone by username to start a new one.", theme::TEXT_MD, theme::TEXT_2).center().width(340),
        ]
        .spacing(12)
        .align_x(Alignment::Center),
    )
    .center(Length::Fill)
    .into()
}

fn typing_names(main: &Main, chat: &Chat) -> Option<String> {
    let users = main.typing.get(&chat.id)?;
    if users.is_empty() {
        return None;
    }
    if !chat.is_group() {
        return Some("typing…".into());
    }
    let names: Vec<&str> = users.keys().filter_map(|id| chat.member(id).map(|member| member.user.name())).collect();
    Some(match names.as_slice() {
        [] => "Someone is typing…".into(),
        [one] => format!("{one} is typing…"),
        [a, b] => format!("{a} and {b} are typing…"),
        _ => "Several people are typing…".into(),
    })
}

fn header<'a>(main: &'a Main, chat: &'a Chat, ctx: Ctx<'a>, narrow: bool) -> Element<'a, Message> {
    let typing = typing_names(main, chat);
    let mut bar = row![].spacing(12).align_y(Alignment::Center);
    if narrow {
        bar = bar.push(widgets::icon_button(Icon::Back, Some(Message::CloseChat), 36.0));
    }
    let (avatar, title, status, status_color, open): (Element<'a, Message>, &str, String, Color, Message) = match &chat.peer {
        Some(peer) if !chat.is_group() => {
            let online = main.online.contains(&peer.id);
            let (status, color) = match &typing {
                Some(text) => (text.clone(), theme::ACCENT),
                None if online => ("Online".to_owned(), theme::SUCCESS),
                None => ("Offline".to_owned(), theme::TEXT_3),
            };
            (widgets::avatar(ctx, peer, 42.0, online), peer.name(), status, color, Message::OpenDialog(Open::PeerProfile(peer.clone())))
        }
        _ => {
            let (status, color) = match &typing {
                Some(text) => (text.clone(), theme::ACCENT),
                None => (format!("{} participants", chat.members.len()), theme::TEXT_3),
            };
            (widgets::group_avatar(&chat.id, 42.0), chat.title(), status, color, Message::OpenDialog(Open::GroupProfile(chat.id.clone())))
        }
    };
    let identity = button(row![avatar, column![strong(format::ellipsize(title, 48), theme::TEXT_LG, theme::TEXT_1), label(status, theme::TEXT_SM, status_color)].spacing(1)].spacing(12).align_y(Alignment::Center))
        .padding([4, 6])
        .style(theme::bare_button)
        .on_press(open);
    bar = bar.push(identity).push(hspace());
    if !chat.is_group() {
        let call = iced::widget::tooltip(
            widgets::icon_button(Icon::Phone, None, 40.0),
            container(label("Audio calls are coming to the desktop app. Use the web or Android app for now.", theme::TEXT_XS, theme::TEXT_1).width(220)).padding([6, 10]).style(theme::menu),
            iced::widget::tooltip::Position::Bottom,
        );
        bar = bar.push(call);
    }
    let _ = main.ws_status == WsStatus::Connected;
    container(bar).height(68).padding(widgets::padding(10.0, 14.0, 10.0, if narrow { 10.0 } else { 22.0 })).align_y(Alignment::Center).style(theme::chat_header).into()
}

fn pinned_bar<'a>(main: &'a Main, chat: &'a Chat, thread: &'a Thread) -> Element<'a, Message> {
    let index = main.pin_cursor % thread.pins.len();
    let pin = &thread.pins[index];
    let title = if thread.pins.len() == 1 { "Pinned message".to_owned() } else { format!("Pinned message #{} of {}", index + 1, thread.pins.len()) };
    let strip = button(
        row![
            container(widgets::icon(Icon::Pin, 15.0, theme::ACCENT)).center(30).style(theme::pill(theme::ACCENT, theme::ACCENT_SOFT)),
            column![strong(title, theme::TEXT_SM, theme::ACCENT), label(format::ellipsize(&pin.preview(), 90), theme::TEXT_SM, theme::TEXT_2).wrapping(text::Wrapping::None)].spacing(1).width(Length::Fill),
            widgets::icon(Icon::ChevronRight, 16.0, theme::TEXT_3),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([6, 10])
    .width(Length::Fill)
    .style(theme::bare_button)
    .on_press(Message::CyclePin);
    let list = button(row![widgets::icon(Icon::List, 16.0, theme::TEXT_2), label(thread.pins.len().to_string(), theme::TEXT_SM, theme::TEXT_2)].spacing(6).align_y(Alignment::Center))
        .padding([6, 10])
        .style(theme::icon_button)
        .on_press(Message::OpenDialog(Open::Pinned(chat.id.clone())));
    container(row![strip, list].spacing(6).align_y(Alignment::Center)).padding([4, 14]).style(theme::pinned_bar).into()
}

fn same_run(a: &ChatMessage, b: &ChatMessage) -> bool {
    a.sender_id == b.sender_id
        && !a.is_deleted()
        && !b.is_deleted()
        && !a.is_call
        && !b.is_call
        && (b.created_at - a.created_at).num_seconds().abs() < RUN_GAP_SECONDS
        && format::same_day(a.created_at, b.created_at)
}

fn messages<'a>(app: &'a App, main: &'a Main, chat: &'a Chat, thread: Option<&'a Thread>, ctx: Ctx<'a>, width: f32) -> Element<'a, Message> {
    let Some(thread) = thread else { return container(Space::new()).height(Length::Fill).into() };
    if !thread.loaded {
        let body: Element<'a, Message> = match &thread.error {
            Some(error) => column![label(error.as_str(), theme::TEXT_MD, theme::DANGER), widgets::small_button("Retry", Some(Message::OpenChat(chat.id.clone())))].spacing(10).align_x(Alignment::Center).into(),
            None => label("Loading messages…", theme::TEXT_MD, theme::TEXT_3).into(),
        };
        return container(body).center(Length::Fill).into();
    }
    let visible: Vec<&ChatMessage> = thread.visible().iter().filter(|message| !message.is_deleted()).collect();
    if visible.is_empty() {
        return container(
            column![
                container(widgets::icon(Icon::Chat, 22.0, theme::TEXT_2)).center(48).style(theme::pill(theme::TEXT_2, theme::SURFACE_2)),
                strong("No messages yet", theme::TEXT_MD, theme::TEXT_1),
                label("Say hello to start the conversation.", theme::TEXT_SM, theme::TEXT_3),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .center(Length::Fill)
        .into();
    }

    let max_bubble = (width * 0.76).clamp(220.0, 620.0);
    let now = Utc::now();
    let highlight = main.highlight.as_ref().map(|(id, _)| id.as_str());
    let mut items = Vec::with_capacity(visible.len() * 2 + 1);
    if thread.loading_older || thread.hidden_older() > 0 || thread.has_older {
        let note = if thread.loading_older { "Loading earlier messages…" } else { "" };
        items.push(container(label(note, theme::TEXT_XS, theme::TEXT_3)).center_x(Length::Fill).height(28).into());
    }
    for (index, message) in visible.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| visible[i]);
        let next = visible.get(index + 1).copied();
        let new_day = previous.is_none_or(|previous| !format::same_day(previous.created_at, message.created_at));
        if new_day {
            items.push(
                container(container(label(format::message_date(message.created_at, now), theme::TEXT_XS, theme::TEXT_2).font(theme::MEDIUM)).padding([4, 12]).style(theme::date_chip))
                    .center_x(Length::Fill)
                    .padding([10, 0])
                    .into(),
            );
        }
        let joined_above = !new_day && previous.is_some_and(|previous| same_run(previous, message));
        let joined_below = next.is_some_and(|next| same_run(message, next) && format::same_day(message.created_at, next.created_at));
        let highlighted = highlight == Some(message.id.as_str());
        items.push(message_row(app, main, chat, message, ctx, max_bubble, joined_above, joined_below, highlighted));
    }

    // Plain column: iced 0.14's keyed column skips widget-type checks when diffing,
    // which corrupts state when a chat's message list is replaced.
    let list = column(items).padding(widgets::padding(12.0, (width * 0.04).clamp(12.0, 56.0), 12.0, (width * 0.04).clamp(12.0, 56.0)));
    let scroll = scrollable(list).id(messages_scroll()).anchor_bottom().on_scroll(Message::Scrolled).height(Length::Fill).width(Length::Fill).style(theme::scroll);

    let mut layers = stack![scroll].width(Length::Fill).height(Length::Fill);
    if !main.at_bottom {
        let unread = chat.unread_count;
        let mut jump = stack![button(container(widgets::icon(Icon::ArrowDown, 20.0, theme::TEXT_1)).center(44)).padding(0).style(theme::floating_round_button).on_press(Message::ScrollToLatest)];
        if unread > 0 {
            jump = jump.push(container(container(text(unread.min(99).to_string()).size(10).font(theme::BOLD)).padding([1, 5]).style(theme::badge)).align_right(44).width(44));
        }
        layers = layers.push(container(jump).align_right(Length::Fill).align_bottom(Length::Fill).padding(18));
    }
    layers.into()
}

#[allow(clippy::too_many_arguments)]
fn message_row<'a>(
    app: &'a App,
    main: &'a Main,
    chat: &'a Chat,
    message: &'a ChatMessage,
    ctx: Ctx<'a>,
    max_bubble: f32,
    joined_above: bool,
    joined_below: bool,
    highlighted: bool,
) -> Element<'a, Message> {
    let own = message.sender_id == main.me.id;
    let (text_color, meta_color) = if own { (theme::BUBBLE_OUT_TEXT, theme::BUBBLE_OUT_TEXT_2) } else { (theme::TEXT_1, theme::TEXT_3) };
    let image_url = message.image_url.as_deref().map(|path| app.client.media_url(path));
    let video_url = message.video_url.as_deref().map(|path| app.client.media_url(path));
    let has_media = image_url.is_some() || video_url.is_some();

    let mut bubble = column![].spacing(5);
    if chat.is_group() && !own && !joined_above {
        bubble = bubble.push(strong(sender_name(main, chat, &message.sender_id), theme::TEXT_SM, theme::author_color(&message.sender_id)));
    }
    if let Some(reply) = &message.reply_to_message {
        let who = if reply.sender_id == main.me.id { "You".to_owned() } else { sender_name(main, chat, &reply.sender_id) };
        let quote = row![
            container(Space::new()).width(3).height(Length::Fill).style(theme::quote_bar(own)),
            column![
                strong(who, theme::TEXT_XS, if own { theme::rgb(0x3b, 0x63, 0xc4) } else { theme::ACCENT }),
                label(format::ellipsize(&reply.preview(), 60), theme::TEXT_SM, meta_color).wrapping(text::Wrapping::None)
            ]
            .spacing(1),
        ]
        .spacing(8)
        .height(Length::Shrink);
        bubble = bubble.push(button(container(quote).padding([5, 8]).style(theme::reply_quote(own)).max_width(max_bubble - 24.0)).padding(0).style(theme::bare_button).on_press(Message::JumpTo(reply.id.clone())));
    }

    let meta = meta_row(main, chat, message, own, meta_color);
    if let Some(url) = &image_url {
        bubble = bubble.push(image_preview(ctx, url, max_bubble.min(360.0)));
    }
    if let Some(url) = &video_url {
        bubble = bubble.push(video_card(url, max_bubble.min(320.0)));
    }
    if message.is_call && !has_media {
        bubble = bubble.push(row![call_card(message, own), meta].spacing(10).align_y(Alignment::End));
    } else if !message.content.is_empty() {
        let body = text(message.content.as_str()).size(theme::TEXT_MD).color(text_color).font(theme::REGULAR);
        let short = message.content.chars().count() <= 38 && !message.content.contains('\n');
        if short {
            bubble = bubble.push(row![body, meta].spacing(10).align_y(Alignment::End));
        } else {
            bubble = bubble.push(column![body, container(meta).align_right(Length::Fill)].spacing(2));
        }
    } else {
        bubble = bubble.push(container(meta).align_right(Length::Fill));
    }

    let padding = if has_media { widgets::padding(6.0, 6.0, 6.0, 6.0) } else { widgets::padding(7.0, 12.0, 6.0, 12.0) };
    let bubble = container(bubble).padding(padding).max_width(max_bubble).style(theme::bubble(own, joined_above, joined_below, highlighted));
    let interactive = mouse_area(bubble).on_right_press(Message::ShowContextMenu(message.clone())).on_double_click(Message::Reply(message.clone()));
    let aligned = if own { container(interactive).align_right(Length::Fill) } else { container(interactive).align_left(Length::Fill) };
    let spacing = if joined_above { 2.0 } else { 8.0 };
    container(aligned).id(message_id(&message.id)).padding(widgets::padding(spacing, 0.0, 0.0, 0.0)).width(Length::Fill).into()
}

fn meta_row<'a>(main: &'a Main, chat: &'a Chat, message: &'a ChatMessage, own: bool, color: Color) -> Element<'a, Message> {
    let mut meta = row![].spacing(4).align_y(Alignment::Center);
    if message.pinned_at.is_some() {
        meta = meta.push(widgets::icon(Icon::Pin, 11.0, color));
    }
    if message.edited_at.is_some() {
        meta = meta.push(label("edited", theme::TEXT_XS, color));
    }
    meta = meta.push(label(format::time(message.created_at), theme::TEXT_XS, color));
    if own {
        let read = chat.peer_last_read_at.is_some_and(|read_at| message.created_at <= read_at);
        let glyph = if read { Icon::CheckDouble } else { Icon::Check };
        let tint = if read { theme::rgb(0x3b, 0x63, 0xc4) } else { color };
        meta = meta.push(widgets::icon(glyph, 15.0, tint));
    }
    let _ = main;
    meta.into()
}

fn sender_name(main: &Main, chat: &Chat, sender_id: &str) -> String {
    if sender_id == main.me.id {
        return main.me.name().to_owned();
    }
    chat.member(sender_id)
        .map(|member| member.user.name().to_owned())
        .or_else(|| chat.peer.as_ref().filter(|peer| peer.id == sender_id).map(|peer| peer.name().to_owned()))
        .unwrap_or_else(|| "Unknown".to_owned())
}

fn image_preview<'a>(ctx: Ctx<'a>, url: &str, max_width: f32) -> Element<'a, Message> {
    let key = Key::new(url, Size::Thumb);
    let open = Message::OpenImage(url.to_owned());
    match ctx.media.get(&key) {
        Some(image) => {
            let aspect = image.height as f32 / image.width.max(1) as f32;
            let width = max_width.min(image.width as f32).max(120.0);
            let height = (width * aspect).min(420.0);
            let width = if width * aspect > 420.0 { 420.0 / aspect } else { width };
            button(iced::widget::image(image.handle.clone()).width(width).height(height).content_fit(iced::ContentFit::Cover).border_radius(13.0))
                .padding(0)
                .style(theme::bare_button)
                .on_press(open)
                .into()
        }
        None => {
            let failed = ctx.media.failed(&key);
            let placeholder = container(if failed { widgets::icon(Icon::Image, 26.0, theme::TEXT_3) } else { label("Loading photo…", theme::TEXT_XS, theme::TEXT_3).into() })
                .width(max_width.min(280.0))
                .height(200)
                .center_x(max_width.min(280.0))
                .center_y(200)
                .style(|_| container::Style { background: Some(theme::black(0.18).into()), border: iced::Border { radius: 13.0.into(), ..Default::default() }, ..Default::default() });
            if failed {
                button(placeholder).padding(0).style(theme::bare_button).on_press(open).into()
            } else {
                sensor(placeholder).on_show(move |_| Message::MediaVisible(key.clone())).into()
            }
        }
    }
}

fn video_card<'a>(url: &str, width: f32) -> Element<'a, Message> {
    let card = container(
        column![
            container(widgets::icon(Icon::Play, 22.0, theme::TEXT_1)).center(52).style(|_| container::Style {
                background: Some(theme::black(0.45).into()),
                border: iced::Border { color: theme::white(0.25), width: 1.0, radius: 999.0.into() },
                ..Default::default()
            }),
            label("Video · opens in your player", theme::TEXT_XS, theme::TEXT_2),
        ]
        .spacing(10)
        .align_x(Alignment::Center),
    )
    .center_x(width)
    .center_y(180)
    .width(width)
    .height(180)
    .style(|_| container::Style {
        background: Some(theme::rgb(0x10, 0x11, 0x13).into()),
        border: iced::Border { radius: 13.0.into(), ..Default::default() },
        ..Default::default()
    });
    button(card).padding(0).style(theme::bare_button).on_press(Message::OpenVideo(url.to_owned())).into()
}

fn call_card<'a>(message: &'a ChatMessage, own: bool) -> Element<'a, Message> {
    let summary = format::call_summary(&message.content);
    let (tint, background) = if summary.completed {
        (theme::SUCCESS, theme::SUCCESS_SOFT)
    } else {
        (theme::DANGER, theme::DANGER_SOFT)
    };
    let tint = if own && summary.completed { theme::rgb(0x2f, 0x8a, 0x57) } else if own { theme::DANGER_STRONG } else { tint };
    let text_color = if own { theme::BUBBLE_OUT_TEXT } else { theme::TEXT_1 };
    let sub_color = if own { theme::BUBBLE_OUT_TEXT_2 } else { theme::TEXT_2 };
    row![
        container(widgets::icon(Icon::CallCard, 21.0, tint)).center(40).style(theme::pill(tint, background)),
        column![
            strong(if own { "Outgoing audio call" } else { "Incoming audio call" }, theme::TEXT_MD, text_color),
            label(format!("{} {}", if own { "↗" } else { "↙" }, summary.detail), theme::TEXT_SM, sub_color),
        ]
        .spacing(1),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .into()
}

fn composer<'a>(main: &'a Main, chat: &'a Chat, width: f32) -> Element<'a, Message> {
    let composer = &main.composer;
    let mut previews = column![].spacing(6);
    let preview = |glyph: Icon, title: String, detail: String, cancel: Message| -> Element<'a, Message> {
        container(
            row![
                widgets::icon(glyph, 18.0, theme::ACCENT),
                column![strong(title, theme::TEXT_SM, theme::ACCENT), label(format::ellipsize(&detail, 90), theme::TEXT_SM, theme::TEXT_2).wrapping(text::Wrapping::None)].spacing(1).width(Length::Fill),
                widgets::icon_button(Icon::Close, Some(cancel), 30.0),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([6, 10])
        .style(theme::composer_preview)
        .into()
    };
    if let Some(editing) = &composer.editing {
        previews = previews.push(preview(Icon::Edit, "Editing message".into(), editing.content.clone(), Message::CancelEdit));
    }
    if let Some(reply) = &composer.replying {
        let who = if reply.sender_id == main.me.id { "yourself".to_owned() } else { sender_name(main, chat, &reply.sender_id) };
        previews = previews.push(preview(Icon::Reply, format!("Replying to {who}"), reply.preview(), Message::CancelReply));
    }
    if let Some(attachment) = &composer.attachment {
        let thumb: Element<'a, Message> = match (&attachment.preview, attachment.kind) {
            (Some(image), _) => iced::widget::image(image.handle.clone()).width(44).height(44).content_fit(iced::ContentFit::Cover).border_radius(8.0).into(),
            (None, MediaKind::Video) => container(widgets::icon(Icon::Play, 18.0, theme::TEXT_1)).center(44).style(theme::pill(theme::TEXT_1, theme::SURFACE_3)).into(),
            (None, MediaKind::Image) => container(widgets::icon(Icon::Image, 18.0, theme::TEXT_1)).center(44).style(theme::pill(theme::TEXT_1, theme::SURFACE_3)).into(),
        };
        let detail = match attachment.kind {
            MediaKind::Image => format::megabytes(attachment.size),
            MediaKind::Video => format!("{} · Video", format::megabytes(attachment.size)),
        };
        previews = previews.push(
            container(
                row![
                    thumb,
                    column![strong(format::ellipsize(&attachment.name, 60), theme::TEXT_SM, theme::TEXT_1), label(detail, theme::TEXT_XS, theme::TEXT_3)].spacing(2).width(Length::Fill),
                    widgets::icon_button(Icon::Close, Some(Message::ClearAttachment), 30.0),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .style(theme::composer_preview),
        );
    }

    let placeholder = if composer.editing.is_some() {
        "Edit message..."
    } else if composer.attachment.is_some() {
        "Add a caption..."
    } else if composer.replying.is_some() {
        "Write a reply..."
    } else {
        "Write a message..."
    };
    let can_send = if composer.editing.is_some() {
        !composer.sending && !composer.text.trim().is_empty()
    } else {
        !composer.sending && (!composer.text.trim().is_empty() || composer.attachment.is_some())
    };
    let input = text_input(placeholder, &composer.text)
        .id(composer_input())
        .on_input(Message::ComposerChanged)
        .on_submit_maybe(can_send.then_some(Message::ComposerSubmit))
        .padding([10, 8])
        .size(theme::TEXT_MD)
        .style(theme::bare_field);
    let mut form = row![].spacing(4).align_y(Alignment::Center);
    if composer.editing.is_none() {
        form = form.push(widgets::icon_button(Icon::Paperclip, (!composer.sending).then_some(Message::Attach), 40.0));
    }
    form = form.push(input).push(
        button(container(widgets::icon(if composer.editing.is_some() { Icon::Check } else { Icon::Send }, 19.0, if can_send { theme::PRIMARY_TEXT } else { theme::white(0.35) })).center(40))
            .padding(0)
            .style(theme::send_button)
            .on_press_maybe(can_send.then_some(Message::ComposerSubmit)),
    );
    let side = (width * 0.04).clamp(12.0, 56.0);
    let form = container(form).padding(6).style(theme::composer(false));
    container(column![previews, form].spacing(6)).padding(widgets::padding(8.0, side, 18.0, side)).width(Length::Fill).into()
}

/// Right-click menu for a message, positioned at the cursor (clamped to the window).
pub fn context_menu<'a>(main: &'a Main, menu: &'a ContextMenu, window: iced::Size, client: &'a crate::api::Client) -> Element<'a, Message> {
    let message = &menu.message;
    let own = message.sender_id == main.me.id;
    let item = |glyph: Icon, name: &'a str, on_press: Message, danger: bool| -> Element<'a, Message> {
        button(row![widgets::icon(glyph, 16.0, if danger { theme::DANGER } else { theme::TEXT_2 }), label(name, theme::TEXT_MD, if danger { theme::DANGER } else { theme::TEXT_1 })].spacing(10).align_y(Alignment::Center))
            .padding([7, 10])
            .width(Length::Fill)
            .style(theme::menu_item(danger))
            .on_press(on_press)
            .into()
    };
    let mut count = 2.0;
    let mut items = column![item(Icon::Reply, "Reply", Message::Reply(message.clone()), false)].spacing(2);
    items = items.push(item(if message.pinned_at.is_some() { Icon::Unpin } else { Icon::Pin }, if message.pinned_at.is_some() { "Unpin" } else { "Pin message" }, Message::TogglePin(message.clone()), false));
    if !message.content.is_empty() && !message.is_call {
        count += 1.0;
        items = items.push(item(Icon::List, "Copy text", Message::CopyText(message.content.clone()), false));
    }
    if let Some(path) = message.image_url.as_deref().or(message.video_url.as_deref()) {
        count += 1.0;
        items = items.push(item(Icon::ArrowDown, "Save as…", Message::SaveMedia(client.media_url(path)), false));
    }
    if own && !message.content.is_empty() && !message.is_call {
        count += 1.0;
        items = items.push(item(Icon::Edit, "Edit", Message::Edit(message.clone()), false));
    }
    if own {
        count += 1.0;
        items = items.push(item(Icon::Trash, "Delete", Message::AskDelete(message.clone()), true));
    }
    let (menu_width, menu_height) = (200.0, count * 36.0 + 12.0);
    let x = menu.position.x.min(window.width - menu_width - 8.0).max(4.0);
    let y = menu.position.y.min(window.height - menu_height - 8.0).max(4.0);
    let card = container(items).padding(6).width(menu_width).style(theme::menu);
    let dismiss = mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill)).on_press(Message::HideContextMenu).on_right_press(Message::HideContextMenu);
    stack![dismiss, container(iced::widget::opaque(card)).padding(widgets::padding(y, 0.0, 0.0, x))].into()
}

/// Scrolls the message list so the given message is centred.
pub fn scroll_to_message<T: Send + 'static>(message_id_value: String) -> Task<T> {
    struct Find {
        target: Id,
        scroll: Id,
        viewport: Option<(Rectangle, Rectangle)>,
        found: Option<Rectangle>,
    }

    impl Operation<()> for Find {
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
            operate(self);
        }

        fn scrollable(&mut self, id: Option<&Id>, bounds: Rectangle, content_bounds: Rectangle, _translation: Vector, _state: &mut dyn Scrollable) {
            if id == Some(&self.scroll) {
                self.viewport = Some((bounds, content_bounds));
            }
        }

        fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
            if id == Some(&self.target) {
                self.found = Some(bounds);
            }
        }

        fn finish(&self) -> Outcome<()> {
            let (Some((viewport, content)), Some(target)) = (self.viewport, self.found) else { return Outcome::None };
            let max = (content.height - viewport.height).max(0.0);
            let relative = target.y - content.y;
            let translation = (relative - (viewport.height - target.height).max(0.0) / 2.0).clamp(0.0, max);
            // The list is anchored to the bottom, so offsets count from the end.
            let offset = AbsoluteOffset { x: None, y: Some(max - translation) };
            Outcome::Chain(Box::new(iced::advanced::widget::operation::scrollable::scroll_to(self.scroll.clone(), offset)))
        }
    }

    let find = Find { target: message_id(&message_id_value), scroll: messages_scroll(), viewport: None, found: None };
    iced::advanced::widget::operate(find).discard()
}

#[allow(dead_code)]
fn point(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

#[allow(dead_code)]
fn user_name(user: &User) -> &str {
    user.name()
}
