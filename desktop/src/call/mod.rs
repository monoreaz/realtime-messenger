//! Native audio calls (WebRTC, compatible with the web client's signalling).
pub mod audio;
pub mod codec;
pub mod engine;
pub mod turn;

use std::time::Instant;

use iced::Subscription;

use engine::{Handle, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Incoming,
    Dialing,
    Connecting,
    Connected,
}

/// The single active call (incoming, outgoing or in progress).
#[derive(Debug)]
pub struct ActiveCall {
    pub id: String,
    pub peer_id: String,
    pub name: String,
    pub phase: Phase,
    pub outgoing: bool,
    /// Incoming offer SDP, until the user answers.
    pub offer: Option<String>,
    pub accepted: bool,
    pub handle: Option<Handle>,
    pub pending_candidates: Vec<serde_json::Value>,
    pub pending_answer: Option<String>,
    pub started_at: Option<Instant>,
    pub muted: bool,
}

impl ActiveCall {
    pub fn outgoing(peer_id: String, name: String) -> Self {
        Self::new(uuid::Uuid::new_v4().to_string(), peer_id, name, Phase::Dialing, true, None)
    }

    pub fn incoming(id: String, peer_id: String, name: String, offer: String) -> Self {
        Self::new(id, peer_id, name, Phase::Incoming, false, Some(offer))
    }

    fn new(id: String, peer_id: String, name: String, phase: Phase, outgoing: bool, offer: Option<String>) -> Self {
        Self {
            id,
            peer_id,
            name,
            phase,
            outgoing,
            offer,
            accepted: outgoing,
            handle: None,
            pending_candidates: Vec::new(),
            pending_answer: None,
            started_at: None,
            muted: false,
        }
    }

    /// The engine runs for outgoing calls immediately and for incoming ones once answered.
    fn role(&self) -> Option<Role> {
        if self.outgoing {
            Some(Role::Caller)
        } else if self.accepted {
            self.offer.clone().map(|offer| Role::Callee { offer })
        } else {
            None
        }
    }

    /// `client` fetches the TURN credentials for the call (`/calls/ice`).
    pub fn subscription(&self, client: &crate::api::client::Client) -> Subscription<(String, engine::Update)> {
        let Some(role) = self.role() else { return Subscription::none() };
        let config = engine::Config { call_id: self.id.clone(), role, test_tone: None, ice: engine::Ice::Fetch(client.clone()), relay_only: false };
        Subscription::run_with(config, |config| {
            use futures::StreamExt;
            let id = config.call_id.clone();
            engine::run(config.clone()).map(move |update| (id.clone(), update))
        })
    }

    /// "1:05" style elapsed time.
    pub fn elapsed(&self) -> Option<String> {
        let seconds = self.started_at?.elapsed().as_secs();
        Some(if seconds >= 3600 {
            format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
        } else {
            format!("{}:{:02}", seconds / 60, seconds % 60)
        })
    }
}

/// User-facing text for a `call.end` reason.
pub fn end_reason(reason: Option<&str>) -> Option<&'static str> {
    match reason? {
        "busy" => Some("This person is busy."),
        "offline" => Some("This person is offline."),
        "declined" => Some("The call was declined."),
        "timeout" => Some("No answer."),
        "failed" => Some("The call could not be connected."),
        _ => None,
    }
}
