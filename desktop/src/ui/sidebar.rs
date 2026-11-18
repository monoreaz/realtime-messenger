//! Navigation rail + chat list (`aside.sidebar` in the web app).

use chrono::Utc;
use iced::widget::{Space, button, column, container, row, scrollable, stack, text, text_input};
use iced::{Alignment, Element, Length};

use super::icons::Icon;
use super::theme;
use super::widgets::{self, Ctx, hspace, label, strong};
use crate::app::{App, Main, Message, WsStatus, search_input};
use crate::dialogs::Open;
use crate::format;

pub fn rail<'a>(main: &'a Main, ctx: Ctx<'a>) -> Element<'a, Message> {
    let status_color = match main.ws_status {
        WsStatus::Connected => theme::SUCCESS,
        WsStatus::Connecting => theme::WARNING,
        WsStatus::Offline => theme::DANGER,
    };
    let profile = button(stack![
        widgets::avatar(ctx, &main.me, 36.0, false),
        container(container(Space::new()).width(10).height(10).style(move |_| container::Style {
            background: Some(status_color.into()),
            border: iced::Border { color: theme::rgb(0x1b, 0x1b, 0x1e), width: 2.0, radius: 999.0.into() },
            ..Default::default()
        }))
        .width(36)
        .height(36)
        .align_right(36)
        .align_bottom(36),
    ])
    .padding(3)
    .style(|theme, status| iced::widget::button::Style { border: iced::Border { radius: 999.0.into(), ..Default::default() }, ..theme::icon_button(theme, status) })
    .on_press(Message::OpenDialog(Open::Settings));

    let mut top = column![super::auth::brand_mark(40.0), widgets::gap(20.0), tip(widgets::rail_button(Icon::Plus, Message::OpenDialog(Open::CreateGroup)), "New group")].spacing(6).align_x(Alignment::Center);
    if main.me.is_admin {
        top = top.push(tip(widgets::rail_button(Icon::Shield, Message::OpenDialog(Open::Admin)), "Admin panel"));
    }
    let bottom = column![
        tip(widgets::rail_button(Icon::Settings, Message::OpenDialog(Open::Settings)), "Settings"),
        tip(widgets::rail_button(Icon::Logout, Message::Logout), "Log out"),
        tip(profile.into(), "Profile"),
    ]
    .spacing(6)
    .align_x(Alignment::Center);

    // The column must span the rail, otherwise its centred children hug the left edge.
    container(column![top, widgets::vspace(), bottom].width(Length::Fill).align_x(Alignment::Center))
        .width(theme::RAIL_WIDTH)
        .height(Length::Fill)
        .padding([18, 0])
        .style(theme::rail)
        .into()
}

fn tip<'a>(content: Element<'a, Message>, name: &'a str) -> Element<'a, Message> {
    iced::widget::tooltip(content, container(label(name, theme::TEXT_XS, theme::TEXT_1)).padding([5, 9]).style(theme::menu), iced::widget::tooltip::Position::Right)
        .gap(6)
        .into()
}

pub fn view<'a>(app: &'a App, main: &'a Main, ctx: Ctx<'a>, width: f32) -> Element<'a, Message> {
    let (status, status_color) = match main.ws_status {
        WsStatus::Connected => ("Online", theme::SUCCESS),
        WsStatus::Connecting => ("Connecting…", theme::WARNING),
        WsStatus::Offline => ("Offline", theme::DANGER),
    };
    let header = row![
        text("Chats").size(theme::TEXT_XL).font(theme::SEMIBOLD).color(theme::TEXT_1),
        hspace(),
        container(row![container(Space::new()).width(6).height(6).style(theme::pill(status_color, status_color)), label(status, theme::TEXT_XS, status_color).font(theme::MEDIUM)].spacing(6).align_y(Alignment::Center))
            .padding([3, 9])
            .style(theme::pill(status_color, theme::SURFACE_2)),
    ]
    .align_y(Alignment::Center);

    let search_field = text_input("Search people…", &main.search.query)
        .id(search_input())
        .on_input(Message::SearchChanged)
        .on_submit(Message::SearchSubmit)
        .padding(widgets::padding(9.0, 8.0, 9.0, 4.0))
        .size(theme::TEXT_MD)
        .style(theme::bare_field);
    let mut search_row = row![container(widgets::icon(Icon::Search, 17.0, theme::TEXT_3)).padding(widgets::padding(0.0, 0.0, 0.0, 12.0)), search_field].align_y(Alignment::Center);
    if !main.search.query.is_empty() {
        search_row = search_row.push(widgets::icon_button(Icon::Close, Some(Message::SearchClear), 32.0));
    }
    let search = container(search_row).height(40).style(theme::sunken_field(false)).align_y(Alignment::Center);

    let mut list = column![].spacing(2);
    let searching = !main.search.query.trim().is_empty();
    if searching {
        if main.search.loading && main.search.results.is_empty() {
            list = list.push(container(label("Searching…", theme::TEXT_SM, theme::TEXT_3)).padding([10, 12]));
        } else if main.search.results.is_empty() && main.search.searched.is_some() {
            list = list.push(container(label("No people found", theme::TEXT_SM, theme::TEXT_3)).padding([10, 12]));
        }
        if !main.search.results.is_empty() {
            list = list.push(list_label("People"));
            for user in &main.search.results {
                let online = main.online.contains(&user.id);
                list = list.push(
                    button(
                        row![
                            widgets::avatar(ctx, user, 38.0, online),
                            column![strong(format::ellipsize(user.name(), 28), theme::TEXT_MD, theme::TEXT_1), label(format!("@{}", user.username), theme::TEXT_SM, theme::TEXT_3)].spacing(1),
                            hspace(),
                            widgets::icon(Icon::ChevronRight, 16.0, theme::TEXT_3),
                        ]
                        .spacing(12)
                        .align_y(Alignment::Center),
                    )
                    .padding([8, 10])
                    .width(Length::Fill)
                    .style(theme::list_button)
                    .on_press(Message::SelectUser(user.clone())),
                );
            }
            list = list.push(list_label("Conversations"));
        }
    }

    let visible: Vec<_> = main.chats.iter().filter(|chat| chat.is_listed() || main.active.as_ref() == Some(&chat.id)).collect();
    let now = Utc::now();
    // Rough characters that fit in a preview line at this sidebar width.
    let preview_chars = ((width - 130.0) / 7.2).max(12.0) as usize;
    let title_chars = ((width - 170.0) / 8.0).max(10.0) as usize;
    let chats: Element<'a, Message> = if visible.is_empty() && !searching {
        container(
            column![
                container(widgets::icon(Icon::Chat, 22.0, theme::TEXT_2)).center(48).style(theme::pill(theme::TEXT_2, theme::SURFACE_2)),
                strong("No conversations yet", theme::TEXT_MD, theme::TEXT_1),
                label("Search for someone by username to start a conversation.", theme::TEXT_SM, theme::TEXT_3).center(),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .padding([40, 16])
        .center_x(Length::Fill)
        .into()
    } else {
        column(visible.into_iter().map(|chat| {
            let active = main.active.as_ref() == Some(&chat.id);
            let unread = chat.unread_count > 0;
            let avatar = match &chat.peer {
                Some(peer) if !chat.is_group() => widgets::avatar(ctx, peer, 46.0, main.online.contains(&peer.id)),
                _ => widgets::group_avatar(&chat.id, 46.0),
            };
            let time = chat.last_message.as_ref().map(|message| format::chat_list_time(message.created_at, now)).unwrap_or_default();
            let typing = main.typing.get(&chat.id).is_some_and(|users| !users.is_empty());
            let preview: Element<'a, Message> = if typing {
                label("typing…", theme::TEXT_SM, theme::ACCENT).into()
            } else {
                match &chat.last_message {
                    Some(message) => {
                        let own = message.sender_id == main.me.id;
                        let body = format::ellipsize(&message.preview(), preview_chars.saturating_sub(if own { 5 } else { 0 }));
                        let mut line = row![].spacing(0);
                        if own {
                            line = line.push(label("You: ", theme::TEXT_SM, theme::TEXT_2));
                        }
                        line.push(label(body, theme::TEXT_SM, if unread { theme::TEXT_2 } else { theme::TEXT_3 }).wrapping(text::Wrapping::None)).into()
                    }
                    None if chat.is_group() => label(format!("{} participants", chat.members.len()), theme::TEXT_SM, theme::TEXT_3).into(),
                    None => label("No messages yet", theme::TEXT_SM, theme::TEXT_3).into(),
                }
            };
            let mut preview_row = row![container(preview).width(Length::Fill).clip(true)].spacing(8).align_y(Alignment::Center);
            if unread {
                let count = if chat.unread_count > 99 { "99+".to_owned() } else { chat.unread_count.to_string() };
                preview_row = preview_row.push(container(text(count).size(11).font(theme::BOLD)).padding([2, 7]).style(theme::badge));
            }
            let title_row = row![
                container(strong(format::ellipsize(chat.title(), title_chars), theme::TEXT_MD, theme::TEXT_1).wrapping(text::Wrapping::None)).width(Length::Fill).clip(true),
                label(time, theme::TEXT_XS, if unread { theme::TEXT_1 } else { theme::TEXT_3 }),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let item = button(row![avatar, column![title_row, preview_row].spacing(3).width(Length::Fill)].spacing(12).align_y(Alignment::Center))
                .padding([9, 10])
                .width(Length::Fill)
                .style(theme::chat_item(active))
                .on_press(Message::OpenChat(chat.id.clone()));
            item.into()
        }))
        .spacing(2)
        .into()
    };
    list = list.push(chats);

    let mut content = column![
        container(header).padding(widgets::padding(18.0, 18.0, 12.0, 18.0)),
        container(search).padding([0, 14]),
        container(scrollable(container(list).padding(widgets::padding(10.0, 10.0, 12.0, 10.0))).height(Length::Fill).style(theme::scroll)).height(Length::Fill),
    ];
    if let Some(banner) = crate::update::banner(&app.updater) {
        content = content.push(container(banner).padding(widgets::padding(0.0, 12.0, 12.0, 12.0)));
    }
    container(content).width(width).height(Length::Fill).style(theme::sidebar).into()
}

fn list_label<'a>(name: &'a str) -> Element<'a, Message> {
    container(label(name, theme::TEXT_XS, theme::TEXT_3).font(theme::SEMIBOLD)).padding(widgets::padding(10.0, 10.0, 4.0, 10.0)).into()
}
