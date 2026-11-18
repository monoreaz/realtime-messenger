//! One WebRTC audio call: str0m (sans-IO) driven by a tokio loop.
//!
//! The loop follows str0m's contract: perform one mutation (network input, timeout,
//! signalling command or an outgoing audio frame), then drain `poll_output` until it
//! returns a timeout. Host candidates are gathered from every non-loopback IPv4
//! interface (Tailscale included), each on its own UDP socket. Remote relay (TURN)
//! candidates from browsers work as ordinary remote candidates.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures::SinkExt;
use str0m::change::{SdpAnswer, SdpOffer};
use str0m::format::Codec;
use str0m::media::{Direction, MediaKind, MediaTime, Mid};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::audio::{self, Audio};
use super::codec::{Decoder, Encoder, FRAME_SAMPLES};

/// Give up if ICE has not connected within this time after negotiation started.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Role {
    Caller,
    Callee { offer: String },
}

#[derive(Debug, Clone)]
pub enum Command {
    RemoteAnswer(String),
    /// Browser `RTCIceCandidateInit` JSON.
    RemoteCandidate(serde_json::Value),
    Hangup,
}

#[derive(Debug, Clone)]
pub enum Update {
    Ready(Handle),
    /// Local SDP to send (`"offer"` or `"answer"`).
    Description { kind: &'static str, sdp: String },
    /// A local ICE candidate in browser `RTCIceCandidateInit` form.
    Candidate(serde_json::Value),
    Connected,
    Failed(String),
    /// After a hangup: audio frames sent/received (diagnostics and tests).
    #[allow(dead_code)]
    Ended { sent: u64, received: u64 },
}

/// Sends commands to a running call; also controls mute.
#[derive(Debug, Clone)]
pub struct Handle {
    commands: mpsc::UnboundedSender<Command>,
    muted: Arc<AtomicBool>,
}

impl Handle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub call_id: String,
    pub role: Role,
    /// Tests: replace devices with a tone of this frequency and allow loopback interfaces.
    pub test_tone: Option<u32>,
}

impl std::hash::Hash for Config {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.call_id.hash(state);
        self.role.hash(state);
    }
}

/// Runs a call until it fails, is hung up, or the stream is dropped.
pub fn run(config: Config) -> impl futures::Stream<Item = Update> {
    iced::stream::channel(64, async move |mut output: futures::channel::mpsc::Sender<Update>| {
        if let Err(error) = call(config, &mut output).await {
            log::warn!("call failed: {error}");
            let _ = output.send(Update::Failed(error)).await;
        }
        futures::future::pending::<()>().await;
    })
}

struct Socket {
    socket: Arc<UdpSocket>,
    local: SocketAddr,
}

fn interfaces(include_loopback: bool) -> Vec<IpAddr> {
    let mut addresses: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| interface.ip().is_ipv4())
        .filter(|interface| include_loopback || !interface.is_loopback())
        .map(|interface| interface.ip())
        .collect();
    addresses.sort();
    addresses.dedup();
    addresses
}

async fn call(config: Config, output: &mut futures::channel::mpsc::Sender<Update>) -> Result<(), String> {
    let muted = Arc::new(AtomicBool::new(false));
    let (commands_tx, mut commands) = mpsc::unbounded_channel();
    output
        .send(Update::Ready(Handle { commands: commands_tx, muted: muted.clone() }))
        .await
        .map_err(|_| "call cancelled")?;

    // Sockets, one per interface; incoming datagrams are funnelled into one channel.
    let mut sockets = Vec::new();
    for ip in interfaces(config.test_tone.is_some()) {
        match UdpSocket::bind(SocketAddr::new(ip, 0)).await {
            Ok(socket) => {
                let local = socket.local_addr().map_err(|error| error.to_string())?;
                sockets.push(Socket { socket: Arc::new(socket), local });
            }
            Err(error) => log::info!("cannot bind call socket on {ip}: {error}"),
        }
    }
    if sockets.is_empty() {
        return Err("No network interface is available for the call.".into());
    }
    let (packets_tx, mut packets) = mpsc::channel::<(SocketAddr, SocketAddr, Vec<u8>)>(256);
    let mut readers = Vec::new();
    for socket in &sockets {
        let (udp, local, sender) = (socket.socket.clone(), socket.local, packets_tx.clone());
        readers.push(tokio::spawn(async move {
            let mut buffer = vec![0u8; 2000];
            while let Ok((size, source)) = udp.recv_from(&mut buffer).await {
                if sender.send((local, source, buffer[..size].to_vec())).await.is_err() {
                    break;
                }
            }
        }));
    }
    let _readers = AbortOnDrop(readers);
    let by_local: HashMap<SocketAddr, Arc<UdpSocket>> = sockets.iter().map(|socket| (socket.local, socket.socket.clone())).collect();

    let mut rtc = Rtc::new(Instant::now());
    let mut local_candidates = Vec::new();
    for socket in &sockets {
        let candidate = Candidate::host(socket.local, "udp").map_err(|error| error.to_string())?;
        local_candidates.push(candidate.to_sdp_string());
        rtc.add_local_candidate(candidate);
    }

    // Negotiation.
    let mut pending_offer = None;
    let mut audio_mid: Option<Mid> = None;
    match &config.role {
        Role::Caller => {
            let mut change = rtc.sdp_api();
            let mid = change.add_media(MediaKind::Audio, Direction::SendRecv, None, None, None);
            audio_mid = Some(mid);
            let (offer, pending) = change.apply().ok_or("Could not create the call offer.")?;
            pending_offer = Some(pending);
            emit(output, Update::Description { kind: "offer", sdp: offer.to_sdp_string() }).await?;
        }
        Role::Callee { offer } => {
            let offer = SdpOffer::from_sdp_string(offer).map_err(|error| format!("Invalid call offer: {error}"))?;
            let answer = rtc.sdp_api().accept_offer(offer).map_err(|error| format!("Could not answer the call: {error}"))?;
            emit(output, Update::Description { kind: "answer", sdp: answer.to_sdp_string() }).await?;
        }
    }
    let mid_for_candidates = audio_mid.map(|mid| mid.to_string()).unwrap_or_else(|| "0".to_owned());
    for candidate in &local_candidates {
        emit(output, Update::Candidate(browser_candidate(candidate, &mid_for_candidates))).await?;
    }

    let mut audio: Audio = match config.test_tone {
        Some(frequency) => audio::synthetic(frequency as f32, muted.clone()),
        None => audio::open(muted.clone())?,
    };
    let mut encoder = Encoder::new()?;
    let mut decoder = Decoder::new()?;
    let mut rtp_time: u64 = 0;
    let mut connected = false;
    let deadline = tokio::time::Instant::now() + CONNECT_TIMEOUT;
    let mut stats = Stats::default();

    loop {
        // Drain: transmit packets and handle events until str0m wants a timeout.
        let timeout = loop {
            match rtc.poll_output().map_err(|error| error.to_string())? {
                Output::Timeout(at) => break at,
                Output::Transmit(transmit) => {
                    if let Some(socket) = by_local.get(&transmit.source) {
                        let _ = socket.send_to(&transmit.contents, transmit.destination).await;
                    }
                }
                Output::Event(event) => match event {
                    Event::Connected => {
                        if !connected {
                            connected = true;
                            emit(output, Update::Connected).await?;
                        }
                    }
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) if connected => {
                        return Err("Connection lost. Please try calling again.".into());
                    }
                    Event::MediaAdded(added) if added.kind == MediaKind::Audio => audio_mid = Some(added.mid),
                    Event::MediaData(data) => {
                        stats.received += 1;
                        match decoder.decode(&data.data) {
                            Ok(pcm) => audio.playback.push(&pcm),
                            Err(error) => log::debug!("{error}"),
                        }
                    }
                    _ => {}
                },
            }
        };

        if !connected && tokio::time::Instant::now() > deadline {
            return Err("Could not connect the call. Check that both devices are online.".into());
        }
        let wait = timeout.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));

        // Exactly one mutation per iteration.
        tokio::select! {
            _ = tokio::time::sleep(wait) => {
                rtc.handle_input(Input::Timeout(Instant::now())).map_err(|error| error.to_string())?;
            }
            Some((local, source, contents)) = packets.recv() => {
                let Ok(contents) = contents.as_slice().try_into() else { continue };
                let input = Input::Receive(Instant::now(), Receive { proto: Protocol::Udp, source, destination: local, contents });
                if rtc.accepts(&input) {
                    rtc.handle_input(input).map_err(|error| error.to_string())?;
                }
            }
            command = commands.recv() => match command {
                Some(Command::RemoteAnswer(sdp)) => {
                    let Some(pending) = pending_offer.take() else { continue };
                    let answer = SdpAnswer::from_sdp_string(&sdp).map_err(|error| format!("Invalid call answer: {error}"))?;
                    rtc.sdp_api().accept_answer(pending, answer).map_err(|error| format!("Could not apply the answer: {error}"))?;
                }
                Some(Command::RemoteCandidate(init)) => {
                    if let Some(candidate) = parse_browser_candidate(&init) {
                        rtc.add_remote_candidate(candidate);
                    }
                }
                Some(Command::Hangup) | None => {
                    rtc.disconnect();
                    log::info!("call {} ended: sent {} / received {} audio frames", config.call_id, stats.sent, stats.received);
                    let _ = output.send(Update::Ended { sent: stats.sent, received: stats.received }).await;
                    return Ok(());
                }
            },
            Some(frame) = audio.capture.recv() => {
                debug_assert_eq!(frame.len(), FRAME_SAMPLES);
                if !connected {
                    continue;
                }
                let Some(mid) = audio_mid else { continue };
                let packet = encoder.encode(&frame)?;
                let time = MediaTime::new(rtp_time, str0m::media::Frequency::FORTY_EIGHT_KHZ);
                rtp_time += FRAME_SAMPLES as u64;
                if let Some(writer) = rtc.writer(mid) {
                    let opus = writer.payload_params().find(|params| params.spec().codec == Codec::Opus).map(|params| params.pt());
                    if let Some(pt) = opus {
                        writer.write(pt, Instant::now(), time, packet).map_err(|error| error.to_string())?;
                        stats.sent += 1;
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct Stats {
    sent: u64,
    received: u64,
}

struct AbortOnDrop(Vec<tokio::task::JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

async fn emit(output: &mut futures::channel::mpsc::Sender<Update>, update: Update) -> Result<(), String> {
    output.send(update).await.map_err(|_| "call cancelled".to_owned())
}

/// `candidate:…` line → browser `RTCIceCandidateInit`.
fn browser_candidate(sdp: &str, mid: &str) -> serde_json::Value {
    let line = sdp.trim().trim_start_matches("a=");
    serde_json::json!({ "candidate": line, "sdpMid": mid, "sdpMLineIndex": 0 })
}

/// Browser `RTCIceCandidateInit` → str0m candidate. Unusable candidates (end-of-candidates
/// markers, mDNS `.local` hosts, TCP) are ignored.
pub fn parse_browser_candidate(init: &serde_json::Value) -> Option<Candidate> {
    let line = init.get("candidate")?.as_str()?.trim();
    let line = line.trim_start_matches("a=");
    if line.is_empty() || line.contains(".local ") || line.split_whitespace().nth(2).is_some_and(|proto| !proto.eq_ignore_ascii_case("udp")) {
        return None;
    }
    match Candidate::from_sdp_string(line) {
        Ok(candidate) => Some(candidate),
        Err(error) => {
            log::debug!("ignoring remote candidate {line:?}: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn parses_browser_candidates() {
        let host = serde_json::json!({"candidate": "candidate:842163049 1 udp 1677729535 100.64.1.2 51734 typ host generation 0 ufrag abcd network-cost 999", "sdpMid": "0", "sdpMLineIndex": 0});
        let parsed = parse_browser_candidate(&host).expect("host candidate parses");
        assert_eq!(parsed.addr(), "100.64.1.2:51734".parse().unwrap());
        let relay = serde_json::json!({"candidate": "candidate:1 1 udp 41885439 100.91.201.40 49160 typ relay raddr 0.0.0.0 rport 0 generation 0"});
        assert!(parse_browser_candidate(&relay).is_some());
        assert!(parse_browser_candidate(&serde_json::json!({"candidate": ""})).is_none());
        assert!(parse_browser_candidate(&serde_json::json!({"candidate": "candidate:1 1 udp 2122260223 a1b2c3.local 54321 typ host"})).is_none());
        assert!(parse_browser_candidate(&serde_json::json!({"candidate": "candidate:2 1 tcp 1518280447 10.0.0.2 9 typ host tcptype active"})).is_none());
        let ours = browser_candidate(&Candidate::host("10.1.2.3:4000".parse().unwrap(), "udp").unwrap().to_sdp_string(), "0");
        assert!(parse_browser_candidate(&ours).is_some(), "{ours}");
    }

    /// Two engines negotiate over in-memory signalling and exchange real Opus audio
    /// through DTLS/SRTP on local UDP sockets.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn two_engines_exchange_audio() {
        let mut caller = Box::pin(run(Config { call_id: "c1".into(), role: Role::Caller, test_tone: Some(440) }));
        let Some(Update::Ready(caller_handle)) = caller.next().await else { panic!("caller not ready") };
        let mut offer = None;
        let mut caller_candidates = Vec::new();
        while offer.is_none() || caller_candidates.is_empty() {
            match tokio::time::timeout(Duration::from_secs(5), caller.next()).await.unwrap().unwrap() {
                Update::Description { kind: "offer", sdp } => offer = Some(sdp),
                Update::Candidate(candidate) => caller_candidates.push(candidate),
                other => panic!("unexpected {other:?}"),
            }
        }
        let mut callee = Box::pin(run(Config { call_id: "c1".into(), role: Role::Callee { offer: offer.unwrap() }, test_tone: Some(660) }));
        let Some(Update::Ready(callee_handle)) = callee.next().await else { panic!("callee not ready") };
        for candidate in caller_candidates {
            callee_handle.send(Command::RemoteCandidate(candidate));
        }

        let (mut caller_connected, mut callee_connected) = (false, false);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !(caller_connected && callee_connected) {
            tokio::select! {
                update = caller.next() => match update.unwrap() {
                    Update::Connected => caller_connected = true,
                    Update::Failed(error) => panic!("caller failed: {error}"),
                    _ => {}
                },
                update = callee.next() => match update.unwrap() {
                    Update::Description { kind: "answer", sdp } => caller_handle.send(Command::RemoteAnswer(sdp)),
                    Update::Candidate(candidate) => caller_handle.send(Command::RemoteCandidate(candidate)),
                    Update::Connected => callee_connected = true,
                    Update::Failed(error) => panic!("callee failed: {error}"),
                    _ => {}
                },
                _ = tokio::time::sleep_until(deadline) => panic!("engines did not connect"),
            }
        }
        // Let audio flow for a second while polling both streams.
        let until = tokio::time::Instant::now() + Duration::from_millis(1500);
        loop {
            tokio::select! {
                _ = caller.next() => {}
                _ = callee.next() => {}
                _ = tokio::time::sleep_until(until) => break,
            }
        }
        caller_handle.send(Command::Hangup);
        callee_handle.send(Command::Hangup);
        for (name, stream) in [("caller", &mut caller), ("callee", &mut callee)] {
            loop {
                match tokio::time::timeout(Duration::from_secs(2), stream.next()).await.expect("hangup processed").unwrap() {
                    Update::Ended { sent, received } => {
                        // ~1.5 s of 20 ms frames each way.
                        assert!(sent > 40, "{name} sent only {sent} frames");
                        assert!(received > 40, "{name} received only {received} frames");
                        break;
                    }
                    _ => continue,
                }
            }
        }
    }
}
