//! End-to-end tests against a real backend. They run only when
//! `MESSENGER_IT_SERVER` points at a throwaway stack (see `scripts/it-stack.sh`);
//! otherwise they are skipped. Never point this at production: it creates data.

use std::time::Duration;

use futures::{Stream, StreamExt};

use crate::api::Client;
use crate::api::models::{GroupDetails, ProfileUpdate};
use crate::api::ws::{self, Connection, Event, ServerEvent};
use crate::storage::{Dirs, SessionStore};

const PASSWORD: &str = "password123";

fn server() -> Option<String> {
    std::env::var("MESSENGER_IT_SERVER").ok().filter(|value| !value.is_empty())
}

type Events = std::pin::Pin<Box<dyn Stream<Item = Event> + Send>>;

/// Drives the connection in a background task (as iced does for subscriptions), so
/// outgoing frames are written even while the test awaits other things.
fn events(client: &Client) -> Events {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut stream = Box::pin(ws::run(ws::Config { client: client.clone(), generation: 0 }));
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            if sender.send(event).is_err() {
                break;
            }
        }
    });
    Box::pin(futures::stream::unfold(receiver, |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) }))
}

/// Polls the event stream until `matches` returns a value (10 s timeout).
async fn wait_for<T>(events: &mut Events, what: &str, mut matches: impl FnMut(&Event) -> Option<T>) -> T {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let next = tokio::time::timeout_at(deadline, events.next()).await;
        match next {
            Ok(Some(event)) => {
                if let Some(value) = matches(&event) {
                    return value;
                }
            }
            Ok(None) => panic!("event stream ended while waiting for {what}"),
            Err(_) => panic!("timed out waiting for {what}"),
        }
    }
}

async fn connect(events: &mut Events) -> Connection {
    wait_for(events, "connection", |event| match event {
        Event::Connected(connection) => Some(connection.clone()),
        _ => None,
    })
    .await
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_fn(64, 48, |x, y| image::Rgba([(x * 4) as u8, (y * 5) as u8, 120, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn end_to_end_against_backend() {
    let Some(server) = server() else {
        eprintln!("MESSENGER_IT_SERVER not set; skipping live integration test");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let dirs = Dirs { config: root.path().join("config"), cache: root.path().join("cache") };
    let store = SessionStore::new(&dirs);

    // --- Login with "remember me" persists the refresh cookie; a fresh client restores it.
    let first = Client::new(&server, Some(store.clone()));
    first.login("alice", PASSWORD, true).await.expect("alice logs in");
    let stored = store.load().expect("remembered session is persisted");
    let alice = Client::new(&server, Some(store.clone()));
    alice.restore(stored.clone());
    alice.refresh().await.expect("restored session refreshes");
    let rotated = store.load().expect("rotated cookie persisted");
    assert_ne!(rotated.refresh_cookie, stored.refresh_cookie, "refresh rotates the cookie");
    let me = alice.me().await.unwrap();
    assert_eq!(me.username, "alice");
    assert!(me.is_admin);

    // A wrong password gives the server's message.
    let error = Client::new(&server, None).login("alice", "wrong-password", false).await.unwrap_err();
    assert_eq!(error.status, Some(401));
    assert!(error.message.contains("Incorrect"), "{}", error.message);

    let bob = Client::new(&server, None);
    bob.login("bob", PASSWORD, false).await.unwrap();
    assert!(!bob.has_refresh_session() || bob.access_token().is_some());
    let carol = Client::new(&server, None);
    carol.login("carol", PASSWORD, false).await.unwrap();

    let mut alice_events = events(&alice);
    let mut bob_events = events(&bob);
    let mut carol_events = events(&carol);
    let alice_ws = connect(&mut alice_events).await;
    let _bob_ws = connect(&mut bob_events).await;
    let _carol_ws = connect(&mut carol_events).await;

    // --- Search and private chat.
    let found = alice.search_users("bo").await.unwrap();
    let bob_user = found.iter().find(|user| user.username == "bob").expect("search finds bob").clone();
    let chat = alice.create_private_chat(&bob_user.id).await.unwrap();
    assert!(!chat.is_group());
    assert_eq!(chat.peer.as_ref().map(|peer| peer.username.as_str()), Some("bob"));

    // --- Send → realtime delivery.
    let hello = alice.send_message(&chat.id, "Hello Bob", None).await.unwrap();
    let delivered = wait_for(&mut bob_events, "message.new", |event| match event {
        Event::Server(ServerEvent::MessageNew { message }) if message.id == hello.id => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(delivered.content, "Hello Bob");

    // --- Chat list shows unread count and preview for bob.
    let bob_chats = bob.chats().await.unwrap();
    let bob_chat = bob_chats.iter().find(|item| item.id == chat.id).unwrap();
    assert_eq!(bob_chat.unread_count, 1);
    assert_eq!(bob_chat.last_message.as_ref().unwrap().content, "Hello Bob");

    // --- Typing indicator.
    alice_ws.typing(&chat.id, true);
    wait_for(&mut bob_events, "typing.start", |event| matches!(event, Event::Server(ServerEvent::TypingStart { chat_id, .. }) if *chat_id == chat.id).then_some(())).await;

    // --- Read receipts.
    bob.mark_read(&chat.id).await.unwrap();
    wait_for(&mut alice_events, "chat.read", |event| matches!(event, Event::Server(ServerEvent::ChatRead { chat_id, .. }) if *chat_id == chat.id).then_some(())).await;
    let alice_view = alice.chats().await.unwrap().into_iter().find(|item| item.id == chat.id).unwrap();
    assert!(alice_view.peer_last_read_at.is_some_and(|read| read >= hello.created_at));

    // --- Reply, edit, pin.
    let reply = bob.send_message(&chat.id, "Hi Alice", Some(&hello.id)).await.unwrap();
    assert_eq!(reply.reply_to_message.as_ref().map(|quote| quote.id.as_str()), Some(hello.id.as_str()));
    let edited = alice.edit_message(&chat.id, &hello.id, "Hello Bob!").await.unwrap();
    assert!(edited.edited_at.is_some());
    wait_for(&mut bob_events, "message.updated", |event| match event {
        Event::Server(ServerEvent::MessageUpdated { message }) if message.id == hello.id && message.content == "Hello Bob!" => Some(()),
        _ => None,
    })
    .await;
    let pinned = bob.set_pinned(&chat.id, &hello.id, true).await.unwrap();
    assert!(pinned.pinned_at.is_some());
    assert_eq!(alice.pinned_messages(&chat.id).await.unwrap().len(), 1);
    let unpinned = alice.set_pinned(&chat.id, &hello.id, false).await.unwrap();
    assert!(unpinned.pinned_at.is_none());

    // --- Validation errors come back as readable messages.
    let empty = alice.send_message(&chat.id, "   ", None).await.unwrap_err();
    assert_eq!(empty.status, Some(422));
    assert!(empty.message.to_lowercase().contains("empty"), "{}", empty.message);

    // --- Pagination (50 per page, `before_message_id`).
    for index in 0..55 {
        alice.send_message(&chat.id, &format!("bulk {index}"), None).await.unwrap();
    }
    let latest = bob.messages(&chat.id, None).await.unwrap();
    assert_eq!(latest.len(), 50);
    assert!(latest.windows(2).all(|pair| pair[0].created_at <= pair[1].created_at), "oldest first");
    let older = bob.messages(&chat.id, Some(&latest[0].id)).await.unwrap();
    assert_eq!(older.len(), 57 - 50);
    assert_eq!(older.first().unwrap().id, hello.id);

    // --- Image upload + media download/decoding used by the UI.
    let image_path = root.path().join("photo.png");
    std::fs::write(&image_path, png()).unwrap();
    let photo = alice.send_media(&chat.id, &image_path, "caption here", Some(&reply.id)).await.unwrap();
    let image_url = alice.media_url(photo.image_url.as_deref().expect("image message has url"));
    assert_eq!(photo.content, "caption here");
    let thumb = crate::media::load(bob.clone(), crate::media::Key::new(image_url.clone(), crate::media::Size::Thumb)).await.unwrap();
    assert_eq!((thumb.width, thumb.height), (64, 48));
    let not_media = root.path().join("notes.txt");
    std::fs::write(&not_media, b"x").unwrap();
    assert!(alice.send_media(&chat.id, &not_media, "", None).await.is_err());

    // --- Delete.
    let deleted = alice.delete_message(&chat.id, &photo.id).await.unwrap();
    assert!(deleted.deleted_at.is_some());
    wait_for(&mut bob_events, "message.deleted", |event| matches!(event, Event::Server(ServerEvent::MessageDeleted { message }) if message.id == photo.id).then_some(())).await;

    // --- Profile + avatar; peers get profile.updated.
    let profile = alice
        .update_profile(&ProfileUpdate { username: "alice".into(), display_name: Some("Alice A.".into()), bio: Some("Testing the desktop app".into()) })
        .await
        .unwrap();
    assert_eq!(profile.display_name.as_deref(), Some("Alice A."));
    wait_for(&mut bob_events, "profile.updated", |event| matches!(event, Event::Server(ServerEvent::ProfileUpdated { user }) if user.username == "alice").then_some(())).await;
    let with_avatar = alice.upload_avatar(&image_path).await.unwrap();
    assert!(with_avatar.avatar_url.is_some());
    let without = alice.remove_avatar().await.unwrap();
    assert!(without.avatar_url.is_none());

    // --- Groups.
    let carol_user = alice.search_users("carol").await.unwrap().into_iter().find(|user| user.username == "carol").unwrap();
    let group = alice.create_group("Desktop team", &[bob_user.id.clone(), carol_user.id.clone()], Some("desktop_team"), Some("Testing")).await.unwrap();
    assert!(group.is_group());
    assert_eq!(group.members.len(), 3);
    assert!(group.is_admin(&me.id));
    wait_for(&mut carol_events, "chat.created", |event| matches!(event, Event::Server(ServerEvent::ChatCreated { chat_id }) if *chat_id == group.id).then_some(())).await;
    let renamed = alice.update_group(&group.id, &GroupDetails { name: "Desktop crew".into(), username: Some("desktop_crew".into()), description: None }).await.unwrap();
    assert_eq!(renamed.name.as_deref(), Some("Desktop crew"));
    let not_admin = bob.update_group(&group.id, &GroupDetails { name: "Hijack".into(), username: None, description: None }).await.unwrap_err();
    assert_eq!(not_admin.status, Some(403));
    bob.send_message(&group.id, "hello group", None).await.unwrap();
    wait_for(&mut carol_events, "group message", |event| matches!(event, Event::Server(ServerEvent::MessageNew { message }) if message.chat_id == group.id).then_some(())).await;

    // --- Sessions.
    let sessions = alice.sessions().await.unwrap();
    assert!(sessions.iter().any(|session| session.current));
    assert!(sessions.iter().any(|session| session.user_agent.as_deref().is_some_and(|agent| agent.starts_with("MessengerDesktop/"))));
    alice.logout_other_sessions().await.unwrap();
    assert_eq!(alice.sessions().await.unwrap().len(), 1);
    // The first client's session was rotated away/revoked: refreshing it fails cleanly.
    assert!(first.refresh().await.unwrap_err().is_unauthorized());

    // --- Admin: stats, search, block → account.blocked over WebSocket, unblock.
    let stats = alice.admin_stats().await.unwrap();
    assert!(stats.users >= 3);
    let page = alice.admin_users("carol", "all", 0, 25).await.unwrap();
    assert_eq!(page.total, 1);
    assert!(bob.admin_stats().await.unwrap_err().status == Some(403));
    alice.admin_set_blocked(&carol_user.id, true).await.unwrap();
    wait_for(&mut carol_events, "account.blocked", |event| matches!(event, Event::Server(ServerEvent::AccountBlocked)).then_some(())).await;
    assert!(carol.chats().await.is_err());
    alice.admin_set_blocked(&carol_user.id, false).await.unwrap();

    // --- Calls endpoint answers (ICE configuration for future native calls).
    assert!(alice.ice_servers().await.is_ok());

    // --- Logout clears the remembered session.
    alice.logout().await.unwrap();
    assert!(store.load().is_none());
    assert!(alice.me().await.unwrap_err().is_unauthorized());
}

/// Fills the throwaway stack with a realistic conversation for screenshots.
/// Runs only with MESSENGER_IT_SEED=1 (in addition to MESSENGER_IT_SERVER).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seed_demo_data() {
    let (Some(server), Ok("1")) = (server(), std::env::var("MESSENGER_IT_SEED").as_deref()) else { return };
    let alice = Client::new(&server, None);
    alice.login("alice", PASSWORD, false).await.unwrap();
    let bob = Client::new(&server, None);
    bob.login("bob", PASSWORD, false).await.unwrap();
    let carol = Client::new(&server, None);
    carol.login("carol", PASSWORD, false).await.unwrap();
    let carol_user = carol.me().await.unwrap();
    bob.update_profile(&ProfileUpdate { username: "bob".into(), display_name: Some("Bob Harper".into()), bio: Some("Coffee, code and cycling.".into()) }).await.unwrap();

    let chat = carol.create_private_chat(&alice.me().await.unwrap().id).await.unwrap();
    let first = carol.send_message(&chat.id, "Hey Alice! Did you get a chance to try the new desktop app?", None).await.unwrap();
    alice.send_message(&chat.id, "Yes — it starts instantly and the chat list scrolls really smoothly.", Some(&first.id)).await.unwrap();
    let plan = carol.send_message(&chat.id, "Great. Let's review the release plan tomorrow at 10:00.", None).await.unwrap();
    carol.set_pinned(&chat.id, &plan.id, true).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("sunset.png");
    image::RgbaImage::from_fn(640, 400, |x, y| {
        let t = y as f32 / 400.0;
        image::Rgba([(220.0 - 120.0 * t) as u8, (140.0 - 60.0 * t + (x % 7) as f32) as u8, (90.0 + 90.0 * t) as u8, 255])
    })
    .save(&picture)
    .unwrap();
    carol.send_media(&chat.id, &picture, "View from the office today", None).await.unwrap();
    let typo = alice.send_message(&chat.id, "Looks amazing! See you tomorow", None).await.unwrap();
    alice.edit_message(&chat.id, &typo.id, "Looks amazing! See you tomorrow 👋").await.unwrap();
    carol.send_message(&chat.id, "👍", None).await.unwrap();
    carol.mark_read(&chat.id).await.unwrap();
    let _ = carol_user;
}
