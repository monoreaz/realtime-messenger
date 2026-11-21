//! End-to-end tests against a real backend. They run only when
//! `MESSENGER_IT_SERVER` points at a throwaway stack (see `scripts/it-stack.sh`);
//! otherwise they are skipped. Never point this at production: it creates data.

use std::time::Duration;

use futures::{Stream, StreamExt};

use crate::api::Client;
use crate::api::models::{GroupDetails, Privacy, ProfileUpdate};
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
    // The server allows 30 messages per 10 s per user (429 beyond that): back off and retry.
    for index in 0..55 {
        loop {
            match alice.send_message(&chat.id, &format!("bulk {index}"), None).await {
                Ok(_) => break,
                Err(error) if error.status == Some(429) => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(error) => panic!("{}", error.message),
            }
        }
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
    // Invited people join only by accepting; until then the creator is the only member.
    assert_eq!(group.members.len(), 1);
    assert!(group.is_admin(&me.id));
    assert_eq!(carol.group_invitations().await.unwrap(), std::slice::from_ref(&group.id));
    assert_eq!(carol.send_message(&group.id, "not yet", None).await.unwrap_err().status, Some(404));
    for invitee in [&bob, &carol] {
        assert!(invitee.accept_group_invitation(&group.id).await.unwrap().members.len() >= 2);
    }
    wait_for(&mut carol_events, "chat.created", |event| matches!(event, Event::Server(ServerEvent::ChatCreated { chat_id }) if *chat_id == group.id).then_some(())).await;
    let renamed = alice.update_group(&group.id, &GroupDetails { name: "Desktop crew".into(), username: Some("desktop_crew".into()), description: None }).await.unwrap();
    assert_eq!(renamed.name.as_deref(), Some("Desktop crew"));
    let not_admin = bob.update_group(&group.id, &GroupDetails { name: "Hijack".into(), username: None, description: None }).await.unwrap_err();
    assert_eq!(not_admin.status, Some(403));
    bob.send_message(&group.id, "hello group", None).await.unwrap();
    wait_for(&mut carol_events, "group message", |event| matches!(event, Event::Server(ServerEvent::MessageNew { message }) if message.chat_id == group.id).then_some(())).await;

    // --- Last seen privacy: hiding it is announced to peers as presence.last_seen.
    assert_eq!(alice.privacy().await.unwrap().last_seen, "everyone");
    let hidden = bob.update_privacy(&Privacy { last_seen: "nobody".into() }).await.unwrap();
    assert_eq!(hidden.last_seen, "nobody");
    wait_for(&mut alice_events, "presence.last_seen", |event| {
        matches!(event, Event::Server(ServerEvent::LastSeen { user_id, last_seen_at: None }) if *user_id == bob_user.id).then_some(())
    })
    .await;
    bob.update_privacy(&Privacy { last_seen: "everyone".into() }).await.unwrap();
    assert!(bob.update_privacy(&Privacy { last_seen: "friends".into() }).await.is_err());

    // --- Blocking: both directions are cut off in the private chat until unblocked.
    assert!(alice.blocked_users().await.unwrap().is_empty());
    let blocked = alice.block_user(&bob_user.id).await.unwrap();
    assert_eq!(blocked.id, bob_user.id);
    wait_for(&mut alice_events, "blocks.updated", |event| matches!(event, Event::Server(ServerEvent::BlocksUpdated)).then_some(())).await;
    assert_eq!(alice.blocked_users().await.unwrap().iter().map(|user| user.id.as_str()).collect::<Vec<_>>(), [bob_user.id.as_str()]);
    let own = alice.send_message(&chat.id, "blocked?", None).await.unwrap_err();
    assert_eq!(own.status, Some(403));
    assert!(own.message.contains("Unblock"), "{}", own.message);
    assert_eq!(bob.send_message(&chat.id, "hi?", None).await.unwrap_err().status, Some(403));
    assert!(bob.blocked_users().await.unwrap().is_empty(), "only the blocker sees the block");
    alice.unblock_user(&bob_user.id).await.unwrap();
    assert!(alice.blocked_users().await.unwrap().is_empty());
    alice.send_message(&chat.id, "unblocked", None).await.unwrap();

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
    // Own media: a photo without caption, a tall photo with caption, and a video.
    let square = dir.path().join("square.png");
    image::RgbaImage::from_fn(300, 300, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 160, 255])).save(&square).unwrap();
    alice.send_media(&chat.id, &square, "", None).await.unwrap();
    let tall = dir.path().join("tall.png");
    image::RgbaImage::from_fn(360, 640, |_, y| image::Rgba([40, (y / 3) as u8, 120, 255])).save(&tall).unwrap();
    alice.send_media(&chat.id, &tall, "Our new office from the street", None).await.unwrap();
    let clip = dir.path().join("clip.mp4");
    // The server checks file signatures: an MP4 header followed by filler bytes.
    std::fs::write(&clip, [b"\x00\x00\x00\x18ftypmp42".as_slice(), b"filler bytes for layout tests"].concat()).unwrap();
    alice.send_media(&chat.id, &clip, "", None).await.unwrap();
    carol.mark_read(&chat.id).await.unwrap();
    // Connecting once records carol's last seen, so alice sees "Last seen …" for her.
    let mut carol_events = events(&carol);
    connect(&mut carol_events).await;
    let _ = carol_user;
}

/// Desktop call engine ⇄ real browser (the web app in Chromium, see scripts/call-interop.mjs).
/// Bob (this test) first receives a call from alice's browser, then calls her back.
/// Runs only with MESSENGER_IT_CALL=1 (plus MESSENGER_IT_SERVER).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_interop_with_browser() {
    use crate::call::engine::{self, Command, Role, Update};

    let (Some(server), Ok("1")) = (server(), std::env::var("MESSENGER_IT_CALL").as_deref()) else { return };
    std::fs::create_dir_all("/src/shots").unwrap();
    let marker = |name: &str| std::fs::write(format!("/src/shots/{name}"), b"1").unwrap();
    let bob = Client::new(&server, None);
    bob.login("bob", PASSWORD, false).await.unwrap();
    let alice = bob.search_users("alice").await.unwrap().into_iter().find(|user| user.username == "alice").unwrap();
    let chat = bob.create_private_chat(&alice.id).await.unwrap();
    bob.send_message(&chat.id, "Call me from the browser", None).await.unwrap();
    let mut events = events(&bob);
    let ws = connect(&mut events).await;
    marker("interop-ready");

    async fn relay(
        mut engine: std::pin::Pin<Box<dyn Stream<Item = Update> + Send>>,
        events: &mut Events,
        ws: &Connection,
        call_id: &str,
        peer_id: &str,
    ) -> (u64, u64) {
        let mut handle: Option<engine::Handle> = None;
        let mut pending: Vec<Command> = Vec::new();
        let mut connected_at = None;
        loop {
            tokio::select! {
                update = engine.next() => match update.expect("engine running") {
                    Update::Ready(h) => {
                        for command in pending.drain(..) {
                            h.send(command);
                        }
                        handle = Some(h);
                    }
                    Update::Description { kind, sdp } => {
                        let event = if kind == "offer" { "call.offer" } else { "call.answer" };
                        ws.send(serde_json::json!({ "type": event, "call_id": call_id, "peer_id": peer_id, "sdp": { "type": kind, "sdp": sdp } }));
                    }
                    Update::Candidate(candidate) => {
                        ws.send(serde_json::json!({ "type": "call.ice", "call_id": call_id, "peer_id": peer_id, "candidate": candidate }));
                    }
                    Update::Connected => {
                        ws.send(serde_json::json!({ "type": "call.connected", "call_id": call_id, "peer_id": peer_id }));
                        connected_at = Some(tokio::time::Instant::now());
                    }
                    Update::Failed(error) => panic!("call failed: {error}"),
                    Update::Ended { sent, received } => return (sent, received),
                },
                event = events.next() => {
                    if let Some(Event::Server(ServerEvent::Call(value))) = event {
                        let kind = value["type"].as_str().unwrap_or_default();
                        // Ignore signals for other (earlier) calls, like the app does.
                        if value["call_id"].as_str() != Some(call_id) {
                            continue;
                        }
                        let command = match kind {
                            "call.answer" => Command::RemoteAnswer(value["sdp"]["sdp"].as_str().unwrap().to_owned()),
                            "call.ice" => Command::RemoteCandidate(value["candidate"].clone()),
                            "call.end" => panic!("browser ended the call early: {value}"),
                            _ => continue,
                        };
                        // Buffer until the engine is ready, like the app does.
                        match &handle {
                            Some(h) => h.send(command),
                            None => pending.push(command),
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if connected_at.is_some_and(|at| at.elapsed() > Duration::from_secs(5)) {
                        handle.as_ref().unwrap().send(Command::Hangup);
                        ws.send(serde_json::json!({ "type": "call.end", "call_id": call_id, "peer_id": peer_id }));
                        connected_at = None;
                    }
                }
            }
        }
    }

    // A: browser → desktop.
    let offer = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            if let Some(Event::Server(ServerEvent::Call(value))) = events.next().await
                && value["type"] == "call.offer"
            {
                return value;
            }
        }
    })
    .await
    .expect("browser calls within 2 minutes");
    let call_id = offer["call_id"].as_str().unwrap().to_owned();
    let engine = Box::pin(engine::run(engine::Config {
        call_id: call_id.clone(),
        role: Role::Callee { offer: offer["sdp"]["sdp"].as_str().unwrap().to_owned() },
        test_tone: Some(523),
    }));
    let (sent, received) = relay(engine, &mut events, &ws, &call_id, &alice.id).await;
    eprintln!("INTEROP A (browser→desktop): sent {sent}, received {received}");
    assert!(sent > 100 && received > 100, "audio both ways: sent {sent} received {received}");

    // B: desktop → browser.
    tokio::time::sleep(Duration::from_secs(3)).await;
    marker("interop-phase-b");
    let call_id = uuid::Uuid::new_v4().to_string();
    let engine = Box::pin(engine::run(engine::Config { call_id: call_id.clone(), role: Role::Caller, test_tone: Some(784) }));
    let (sent, received) = relay(engine, &mut events, &ws, &call_id, &alice.id).await;
    eprintln!("INTEROP B (desktop→browser): sent {sent}, received {received}");
    assert!(sent > 100 && received > 100, "audio both ways: sent {sent} received {received}");
    marker("interop-done");
}

/// Media after the 2026-11 security fixes: the server serves uploads only to signed-in
/// members of the chat, and the client sends credentials only to its own server.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_needs_a_member_session() {
    let Some(server) = server() else { return };
    let login = |name: &'static str| {
        let server = server.clone();
        async move {
            let client = Client::new(&server, None);
            client.login(name, PASSWORD, false).await.unwrap();
            client
        }
    };
    let alice = login("alice").await;
    let bob = login("bob").await;
    let carol = login("carol").await;
    let chat = alice.create_private_chat(&bob.me().await.unwrap().id).await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let picture = dir.path().join("pic.png");
    let png_bytes = png();
    std::fs::write(&picture, &png_bytes).unwrap();
    let clip = dir.path().join("clip.mp4");
    let mut video = b"\x00\x00\x00\x18ftypmp42".to_vec();
    video.extend((0..4096u32).map(|i| (i % 251) as u8));
    std::fs::write(&clip, &video).unwrap();
    let image = alice.send_media(&chat.id, &picture, "", None).await.unwrap();
    let movie = alice.send_media(&chat.id, &clip, "", None).await.unwrap();
    let image_url = alice.media_url(image.image_url.as_deref().unwrap());
    let video_url = alice.media_url(movie.video_url.as_deref().unwrap());

    // Members download with their session; an unrelated signed-in user gets 404.
    assert_eq!(alice.download(&image_url).await.unwrap(), png_bytes);
    assert_eq!(bob.download(&image_url).await.unwrap(), png_bytes);
    assert_eq!(carol.download(&image_url).await.unwrap_err().status, Some(404));

    // Video: whole-file download (how the app opens videos) and byte ranges (seeking).
    let target = dir.path().join("downloaded.mp4");
    bob.download_to(&video_url, &target, |_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), video);
    assert_eq!(bob.download_range(&video_url, 4, 11).await.unwrap(), (206, b"ftypmp42".to_vec()));
    assert_eq!(carol.download_range(&video_url, 4, 11).await.unwrap_err().status, Some(404));

    // Without a session there is nothing to send, and the server refuses.
    assert!(Client::new(&server, None).download(&image_url).await.is_err());

    // Logout ends media access for that client.
    let bob_laptop = login("bob").await;
    assert!(bob_laptop.download(&image_url).await.is_ok());
    bob_laptop.logout().await.unwrap();
    assert!(bob_laptop.download(&image_url).await.is_err());

    // Revoking a session elsewhere ("log out other sessions") ends it too.
    let bob_phone = login("bob").await;
    assert!(bob_phone.download(&image_url).await.is_ok());
    bob.logout_other_sessions().await.unwrap();
    assert!(bob_phone.download(&image_url).await.is_err());
    assert!(bob.download(&image_url).await.is_ok());

    // A URL on any other origin is fetched without the session's credentials.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = socket.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        socket.write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
        String::from_utf8_lossy(&request).to_lowercase()
    });
    let foreign = format!("http://127.0.0.1:{port}/api/uploads/messages/x.png");
    assert!(!alice.is_server_media(&foreign));
    assert!(alice.download(&foreign).await.is_err());
    let request = seen.await.unwrap();
    assert!(request.starts_with("get /api/uploads/messages/x.png"), "{request}");
    assert!(!request.contains("authorization:") && !request.contains("cookie:"), "credentials leaked: {request}");
}

/// JWT key rotation (run by the audit's orchestration script, which restarts the backend
/// with a new JWT_SECRET_KEY when the `ready` file appears and then creates `rotated`).
/// Checks that nobody is logged out: a running client renews its access token from its
/// refresh cookie, and a restarted app restores its remembered session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jwt_rotation_keeps_sessions() {
    let (Some(server), Ok(state)) = (server(), std::env::var("MESSENGER_IT_ROTATION_DIR")) else { return };
    let state = std::path::PathBuf::from(state);
    let dirs = Dirs { config: state.join("config"), cache: state.join("cache") };
    let store = SessionStore::new(&dirs);

    // A running app with "remember me", and one without (session cookie in memory only).
    let remembered = Client::new(&server, Some(store.clone()));
    remembered.login("alice", PASSWORD, true).await.unwrap();
    let in_memory = Client::new(&server, None);
    in_memory.login("bob", PASSWORD, false).await.unwrap();
    let old_token = remembered.access_token().unwrap();
    let old_bob_token = in_memory.access_token().unwrap();
    let mut events = events(&in_memory);
    connect(&mut events).await;

    std::fs::write(state.join("ready"), b"").unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    while !state.join("rotated").exists() {
        assert!(tokio::time::Instant::now() < deadline, "backend was not restarted");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Tokens signed with the old key are refused...
    let raw = reqwest::Client::new();
    for token in [&old_token, &old_bob_token] {
        let status = raw.get(format!("{server}/api/users/me")).bearer_auth(token).send().await.unwrap().status();
        assert_eq!(status.as_u16(), 401);
    }
    // ...but both running clients carry on: the first 401 triggers a refresh with the
    // (database-backed) refresh cookie, which the key rotation does not affect.
    assert_eq!(remembered.me().await.unwrap().username, "alice");
    assert_eq!(in_memory.me().await.unwrap().username, "bob");
    assert_ne!(remembered.access_token().unwrap(), old_token);
    // Rotating the key needs a backend restart, which drops every WebSocket. The client
    // reconnects; its old token is refused (policy close), so it refreshes and reconnects.
    let mut reconnected = false;
    for _ in 0..6 {
        let attempt = tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = events.next().await {
                if matches!(event, Event::Connected(_)) {
                    return true;
                }
            }
            false
        })
        .await;
        if matches!(attempt, Ok(true)) {
            reconnected = true;
            break;
        }
    }
    assert!(reconnected, "realtime connection did not come back after the rotation");
    let bob_id = in_memory.me().await.unwrap().id;
    let chat = remembered.create_private_chat(&bob_id).await.unwrap();
    remembered.send_message(&chat.id, "after rotation", None).await.unwrap();
    wait_for(&mut events, "message over the existing socket", |event| {
        matches!(event, Event::Server(ServerEvent::MessageNew { message }) if message.content == "after rotation").then_some(())
    })
    .await;
    // A restarted app restores the remembered session from disk.
    let restarted = Client::new(&server, Some(store.clone()));
    restarted.restore(store.load().expect("remembered session on disk"));
    assert_eq!(restarted.me().await.unwrap().username, "alice");
}
