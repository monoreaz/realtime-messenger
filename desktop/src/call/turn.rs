//! Minimal TURN client (RFC 8656, UDP transport) for calls.
//!
//! Sans-IO like str0m: [`Turn`] produces datagrams for the TURN server
//! ([`Turn::poll_transmit`]), consumes datagrams from it ([`Turn::handle_input`]) and is
//! driven by [`Turn::handle_timeout`]. Peers are reached with Send indications and arrive
//! as Data indications (no channels: audio packets are small, and it keeps the state
//! machine simple). Authentication is the long-term credential mechanism with the
//! time-limited username/password the server hands out at `/calls/ice`.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use hmac::{Hmac, KeyInit, Mac};
use md5::{Digest, Md5};
use sha1::Sha1;

const MAGIC_COOKIE: u32 = 0x2112_A442;

const ALLOCATE: u16 = 0x003;
const REFRESH: u16 = 0x004;
const SEND: u16 = 0x006;
const DATA: u16 = 0x007;
const CREATE_PERMISSION: u16 = 0x008;

const CLASS_REQUEST: u16 = 0x000;
const CLASS_INDICATION: u16 = 0x010;
const CLASS_SUCCESS: u16 = 0x100;
const CLASS_ERROR: u16 = 0x110;

const ATTR_USERNAME: u16 = 0x0006;
const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
const ATTR_ERROR_CODE: u16 = 0x0009;
const ATTR_LIFETIME: u16 = 0x000D;
const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
const ATTR_DATA: u16 = 0x0013;
const ATTR_REALM: u16 = 0x0014;
const ATTR_NONCE: u16 = 0x0015;
const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

const UDP: u8 = 17;
/// RFC 8489 retransmission: 500 ms RTO, doubled per attempt.
const INITIAL_RTO: Duration = Duration::from_millis(500);
const MAX_ATTEMPTS: u32 = 5;
/// Permissions last 300 s on the server; refresh well before.
const PERMISSION_REFRESH: Duration = Duration::from_secs(240);
const DEFAULT_LIFETIME: u32 = 600;
/// 401/438 answers accepted per request before giving up (wrong credentials).
const MAX_AUTH_RETRIES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The relay is ready. `mapped` is our address as the server sees it.
    Allocated { relayed: SocketAddr, mapped: Option<SocketAddr> },
    /// A datagram from `peer`, relayed by the server.
    Data { peer: SocketAddr, data: Vec<u8> },
    /// The allocation is unusable (unreachable server, rejected credentials, expired).
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Allocate,
    Refresh { lifetime: u32 },
    Permission(IpAddr),
}

#[derive(Debug)]
struct Transaction {
    kind: Kind,
    packet: Vec<u8>,
    sent: Instant,
    rto: Duration,
    attempts: u32,
    auth_retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Allocating,
    Allocated { relayed: SocketAddr },
    Done,
}

#[derive(Debug)]
struct Permission {
    /// None while the CreatePermission request is in flight.
    refresh_at: Option<Instant>,
}

#[derive(Debug)]
pub struct Turn {
    server: SocketAddr,
    credentials: Credentials,
    realm: Vec<u8>,
    nonce: Vec<u8>,
    key: Option<[u8; 16]>,
    state: State,
    transactions: HashMap<[u8; 12], Transaction>,
    permissions: HashMap<IpAddr, Permission>,
    refresh_at: Option<Instant>,
    outbox: VecDeque<Vec<u8>>,
    events: VecDeque<Event>,
}

impl Turn {
    /// Starts allocating a relay on `server`.
    pub fn new(server: SocketAddr, credentials: Credentials, now: Instant) -> Self {
        let mut turn = Self {
            server,
            credentials,
            realm: Vec::new(),
            nonce: Vec::new(),
            key: None,
            state: State::Allocating,
            transactions: HashMap::new(),
            permissions: HashMap::new(),
            refresh_at: None,
            outbox: VecDeque::new(),
            events: VecDeque::new(),
        };
        turn.request(Kind::Allocate, now, 0);
        turn
    }

    pub fn server(&self) -> SocketAddr {
        self.server
    }

    pub fn relayed(&self) -> Option<SocketAddr> {
        match self.state {
            State::Allocated { relayed } => Some(relayed),
            _ => None,
        }
    }

    /// Next datagram to send to the server.
    pub fn poll_transmit(&mut self) -> Option<Vec<u8>> {
        self.outbox.pop_front()
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Relays `data` to `peer` (installing a permission for its IP first if needed).
    pub fn send(&mut self, peer: SocketAddr, data: &[u8], now: Instant) {
        if self.relayed().is_none() {
            return;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.permissions.entry(peer.ip()) {
            entry.insert(Permission { refresh_at: None });
            self.request(Kind::Permission(peer.ip()), now, 0);
        }
        // Sent right away: until the permission is installed the server drops it, and ICE
        // retransmits its checks anyway.
        let packet = Builder::new(SEND | CLASS_INDICATION, transaction_id())
            .attribute(ATTR_XOR_PEER_ADDRESS, &xor_address(peer, &[0; 12]))
            .attribute(ATTR_DATA, data)
            .finish();
        self.outbox.push_back(packet);
    }

    /// A datagram that came from the server.
    pub fn handle_input(&mut self, packet: &[u8], now: Instant) {
        let Some(message) = Message::parse(packet) else { return };
        match message.class() {
            CLASS_INDICATION if message.method() == DATA => {
                if self.relayed().is_none() {
                    return;
                }
                if let (Some(peer), Some(data)) = (
                    message.get(ATTR_XOR_PEER_ADDRESS).and_then(|value| parse_xor_address(value, &message.transaction_id)),
                    message.get(ATTR_DATA),
                ) {
                    self.events.push_back(Event::Data { peer, data: data.to_vec() });
                }
            }
            CLASS_SUCCESS | CLASS_ERROR => self.handle_response(&message, now),
            _ => {}
        }
    }

    fn handle_response(&mut self, message: &Message, now: Instant) {
        let Some(transaction) = self.transactions.get(&message.transaction_id) else { return };
        let kind = transaction.kind;
        let auth_retries = transaction.auth_retries;
        if message.class() == CLASS_SUCCESS {
            // Responses to authenticated requests must carry a valid MESSAGE-INTEGRITY;
            // anything else is not from the server we share the key with.
            if let Some(key) = self.key
                && !message.verify_integrity(&key)
            {
                log::debug!("TURN {}: dropping response with bad integrity", self.server);
                return;
            }
            self.transactions.remove(&message.transaction_id);
            self.on_success(kind, message, now);
            return;
        }

        self.transactions.remove(&message.transaction_id);
        let code = message.get(ATTR_ERROR_CODE).map(error_code).unwrap_or(0);
        if matches!(code, 401 | 438) && auth_retries < MAX_AUTH_RETRIES {
            if let Some(realm) = message.get(ATTR_REALM) {
                self.realm = realm.to_vec();
            }
            if let Some(nonce) = message.get(ATTR_NONCE) {
                self.nonce = nonce.to_vec();
            }
            self.key = Some(long_term_key(&self.credentials, &self.realm));
            self.request(kind, now, auth_retries + 1);
            return;
        }
        match kind {
            Kind::Allocate => self.fail(format!("TURN server {} refused the allocation (error {code})", self.server)),
            Kind::Refresh { lifetime: 0 } => self.state = State::Done,
            Kind::Refresh { .. } => self.fail(format!("TURN allocation on {} was lost (error {code})", self.server)),
            Kind::Permission(ip) => {
                log::debug!("TURN {}: permission for {ip} refused (error {code})", self.server);
                self.permissions.remove(&ip);
            }
        }
    }

    fn on_success(&mut self, kind: Kind, message: &Message, now: Instant) {
        let tid = &message.transaction_id;
        let lifetime = message
            .get(ATTR_LIFETIME)
            .and_then(|value| value.try_into().ok())
            .map(u32::from_be_bytes)
            .unwrap_or(DEFAULT_LIFETIME);
        match kind {
            Kind::Allocate => {
                let Some(relayed) = message.get(ATTR_XOR_RELAYED_ADDRESS).and_then(|value| parse_xor_address(value, tid)) else {
                    self.fail(format!("TURN server {} sent no relay address", self.server));
                    return;
                };
                let mapped = message.get(ATTR_XOR_MAPPED_ADDRESS).and_then(|value| parse_xor_address(value, tid));
                self.state = State::Allocated { relayed };
                self.refresh_at = Some(now + refresh_interval(lifetime));
                self.events.push_back(Event::Allocated { relayed, mapped });
            }
            Kind::Refresh { lifetime: 0 } => self.state = State::Done,
            Kind::Refresh { .. } => self.refresh_at = Some(now + refresh_interval(lifetime)),
            Kind::Permission(ip) => {
                if let Some(permission) = self.permissions.get_mut(&ip) {
                    permission.refresh_at = Some(now + PERMISSION_REFRESH);
                }
            }
        }
    }

    /// Retransmissions and refreshes; call regularly (every ~100 ms is plenty).
    pub fn handle_timeout(&mut self, now: Instant) {
        let mut expired = Vec::new();
        for (tid, transaction) in &mut self.transactions {
            if now < transaction.sent + transaction.rto {
                continue;
            }
            if transaction.attempts >= MAX_ATTEMPTS {
                expired.push(*tid);
                continue;
            }
            transaction.attempts += 1;
            transaction.sent = now;
            transaction.rto *= 2;
            self.outbox.push_back(transaction.packet.clone());
        }
        for tid in expired {
            let Some(transaction) = self.transactions.remove(&tid) else { continue };
            match transaction.kind {
                Kind::Allocate => self.fail(format!("TURN server {} did not answer", self.server)),
                Kind::Refresh { lifetime: 0 } => self.state = State::Done,
                Kind::Refresh { .. } => self.fail(format!("TURN server {} stopped answering", self.server)),
                Kind::Permission(ip) => {
                    self.permissions.remove(&ip);
                }
            }
        }
        if self.relayed().is_none() {
            return;
        }
        if self.refresh_at.is_some_and(|at| now >= at) {
            // Retry after a minute if this refresh gets lost; a failure ends it.
            self.refresh_at = Some(now + Duration::from_secs(60));
            self.request(Kind::Refresh { lifetime: DEFAULT_LIFETIME }, now, 0);
        }
        let due: Vec<IpAddr> = self
            .permissions
            .iter()
            .filter(|(_, permission)| permission.refresh_at.is_some_and(|at| now >= at))
            .map(|(ip, _)| *ip)
            .collect();
        for ip in due {
            self.permissions.insert(ip, Permission { refresh_at: None });
            self.request(Kind::Permission(ip), now, 0);
        }
    }

    /// Releases the allocation: the returned datagram (Refresh, lifetime 0) is sent once,
    /// without waiting for the answer. The server limits allocations per user.
    pub fn close(&mut self) -> Option<Vec<u8>> {
        self.relayed()?;
        self.state = State::Done;
        let key = self.key?;
        Some(self.build(Kind::Refresh { lifetime: 0 }, transaction_id(), Some(key)))
    }

    fn fail(&mut self, reason: String) {
        if self.state != State::Done {
            self.state = State::Done;
            self.transactions.clear();
            self.events.push_back(Event::Failed(reason));
        }
    }

    fn request(&mut self, kind: Kind, now: Instant, auth_retries: u32) {
        let tid = transaction_id();
        let packet = self.build(kind, tid, self.key);
        self.outbox.push_back(packet.clone());
        self.transactions.insert(tid, Transaction { kind, packet, sent: now, rto: INITIAL_RTO, attempts: 1, auth_retries });
    }

    fn build(&self, kind: Kind, tid: [u8; 12], key: Option<[u8; 16]>) -> Vec<u8> {
        let mut builder = match kind {
            Kind::Allocate => Builder::new(ALLOCATE | CLASS_REQUEST, tid).attribute(ATTR_REQUESTED_TRANSPORT, &[UDP, 0, 0, 0]),
            Kind::Refresh { lifetime } => Builder::new(REFRESH | CLASS_REQUEST, tid).attribute(ATTR_LIFETIME, &lifetime.to_be_bytes()),
            Kind::Permission(ip) => {
                Builder::new(CREATE_PERMISSION | CLASS_REQUEST, tid).attribute(ATTR_XOR_PEER_ADDRESS, &xor_address(SocketAddr::new(ip, 0), &tid))
            }
        };
        if let Some(key) = key {
            builder = builder
                .attribute(ATTR_USERNAME, self.credentials.username.as_bytes())
                .attribute(ATTR_REALM, &self.realm)
                .attribute(ATTR_NONCE, &self.nonce)
                .integrity(&key);
        }
        builder.finish()
    }
}

fn refresh_interval(lifetime: u32) -> Duration {
    Duration::from_secs(u64::from(lifetime.saturating_sub(60).max(30)))
}

fn transaction_id() -> [u8; 12] {
    let mut tid = [0; 12];
    tid.copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..12]);
    tid
}

/// RFC 8489 §9.2.2: key = MD5(username ":" realm ":" password).
fn long_term_key(credentials: &Credentials, realm: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(credentials.username.as_bytes());
    hasher.update(b":");
    hasher.update(realm);
    hasher.update(b":");
    hasher.update(credentials.password.as_bytes());
    hasher.finalize().into()
}

fn hmac_sha1(key: &[u8], data: &[u8]) -> [u8; 20] {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn error_code(value: &[u8]) -> u16 {
    if value.len() < 4 {
        return 0;
    }
    u16::from(value[2] & 0x07) * 100 + u16::from(value[3])
}

/// Whether `packet` looks like a STUN message (RFC 7983 demultiplexing: first byte 0–3).
pub fn is_stun(packet: &[u8]) -> bool {
    packet.len() >= 20 && packet[0] < 4 && packet[4..8] == MAGIC_COOKIE.to_be_bytes()
}

fn xor_address(address: SocketAddr, tid: &[u8; 12]) -> Vec<u8> {
    let port = address.port() ^ (MAGIC_COOKIE >> 16) as u16;
    let mut value = Vec::with_capacity(20);
    match address.ip() {
        IpAddr::V4(ip) => {
            value.extend([0, 1]);
            value.extend(port.to_be_bytes());
            value.extend((u32::from(ip) ^ MAGIC_COOKIE).to_be_bytes());
        }
        IpAddr::V6(ip) => {
            value.extend([0, 2]);
            value.extend(port.to_be_bytes());
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            mask[4..].copy_from_slice(tid);
            value.extend(ip.octets().iter().zip(mask).map(|(byte, mask)| byte ^ mask));
        }
    }
    value
}

fn parse_xor_address(value: &[u8], tid: &[u8; 12]) -> Option<SocketAddr> {
    if value.len() < 8 {
        return None;
    }
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC_COOKIE >> 16) as u16;
    match value[1] {
        1 => {
            let ip = u32::from_be_bytes(value[4..8].try_into().ok()?) ^ MAGIC_COOKIE;
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
        }
        2 if value.len() >= 20 => {
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
            mask[4..].copy_from_slice(tid);
            let mut octets = [0u8; 16];
            for (index, octet) in octets.iter_mut().enumerate() {
                *octet = value[4 + index] ^ mask[index];
            }
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port))
        }
        _ => None,
    }
}

struct Builder {
    buffer: Vec<u8>,
}

impl Builder {
    fn new(message_type: u16, tid: [u8; 12]) -> Self {
        let mut buffer = Vec::with_capacity(128);
        buffer.extend(message_type.to_be_bytes());
        buffer.extend([0, 0]);
        buffer.extend(MAGIC_COOKIE.to_be_bytes());
        buffer.extend(tid);
        Self { buffer }
    }

    fn attribute(mut self, kind: u16, value: &[u8]) -> Self {
        self.buffer.extend(kind.to_be_bytes());
        self.buffer.extend((value.len() as u16).to_be_bytes());
        self.buffer.extend(value);
        while !self.buffer.len().is_multiple_of(4) {
            self.buffer.push(0);
        }
        self.set_length(self.buffer.len() - 20);
        self
    }

    /// MESSAGE-INTEGRITY over everything so far, with the length already counting it.
    fn integrity(mut self, key: &[u8]) -> Self {
        self.set_length(self.buffer.len() - 20 + 24);
        let mac = hmac_sha1(key, &self.buffer);
        self.buffer.extend(ATTR_MESSAGE_INTEGRITY.to_be_bytes());
        self.buffer.extend(20u16.to_be_bytes());
        self.buffer.extend(mac);
        self
    }

    fn set_length(&mut self, length: usize) {
        self.buffer[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    }

    fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

struct Message<'a> {
    message_type: u16,
    transaction_id: [u8; 12],
    raw: &'a [u8],
    /// (type, offset of the attribute header, value)
    attributes: Vec<(u16, usize, &'a [u8])>,
}

impl<'a> Message<'a> {
    fn parse(raw: &'a [u8]) -> Option<Self> {
        if !is_stun(raw) {
            return None;
        }
        let length = u16::from_be_bytes([raw[2], raw[3]]) as usize;
        if 20 + length != raw.len() || !length.is_multiple_of(4) {
            return None;
        }
        let mut attributes = Vec::new();
        let mut offset = 20;
        while offset + 4 <= raw.len() {
            let kind = u16::from_be_bytes([raw[offset], raw[offset + 1]]);
            let size = u16::from_be_bytes([raw[offset + 2], raw[offset + 3]]) as usize;
            let value = raw.get(offset + 4..offset + 4 + size)?;
            attributes.push((kind, offset, value));
            offset += 4 + size.div_ceil(4) * 4;
        }
        Some(Self {
            message_type: u16::from_be_bytes([raw[0], raw[1]]),
            transaction_id: raw[8..20].try_into().ok()?,
            raw,
            attributes,
        })
    }

    fn class(&self) -> u16 {
        self.message_type & 0x0110
    }

    fn method(&self) -> u16 {
        let t = self.message_type;
        (t & 0x000F) | ((t & 0x00E0) >> 1) | ((t & 0x3E00) >> 2)
    }

    /// First occurrence; attributes after MESSAGE-INTEGRITY are ignored (except by
    /// callers that don't need integrity), as RFC 8489 §14.5 requires.
    fn get(&self, kind: u16) -> Option<&'a [u8]> {
        self.attributes
            .iter()
            .take_while(|(attribute, ..)| *attribute != ATTR_MESSAGE_INTEGRITY || kind == ATTR_MESSAGE_INTEGRITY)
            .find(|(attribute, ..)| *attribute == kind)
            .map(|(_, _, value)| *value)
    }

    fn verify_integrity(&self, key: &[u8]) -> bool {
        let Some(&(_, offset, value)) = self.attributes.iter().find(|(kind, ..)| *kind == ATTR_MESSAGE_INTEGRITY) else {
            return false;
        };
        let mut signed = self.raw[..offset].to_vec();
        signed[2..4].copy_from_slice(&((offset - 20 + 24) as u16).to_be_bytes());
        let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC takes any key length");
        mac.update(&signed);
        mac.verify_slice(value).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: String = text.split_whitespace().collect();
        (0..digits.len()).step_by(2).map(|index| u8::from_str_radix(&digits[index..index + 2], 16).unwrap()).collect()
    }

    /// RFC 5769 §2.1 sample request (short-term credential, so the key is the password).
    #[test]
    fn rfc5769_request_integrity() {
        let request = hex(
            "000100582112a442b7e7a701bc34d686fa87dfae802200105354554e207465737420636c69656e74
             002400046e0001ff80290008932ff9b151263b36000600096576746a3a68367659202020
             000800149aeaa70cbfd8cb56781ef2b5b2d3f249c1b571a280280004e57a3bcf",
        );
        let message = Message::parse(&request).expect("parses");
        assert_eq!(message.class(), CLASS_REQUEST);
        assert_eq!(message.get(ATTR_USERNAME), Some(&b"evtj:h6vY"[..]));
        assert!(message.verify_integrity(b"VOkJxbRl1RmTxUk/WvJxBt"));
        assert!(!message.verify_integrity(b"wrong"));
    }

    /// RFC 5769 §2.2: XOR-MAPPED-ADDRESS 192.0.2.1:32853.
    #[test]
    fn rfc5769_xor_mapped_address() {
        let address: SocketAddr = "192.0.2.1:32853".parse().unwrap();
        assert_eq!(xor_address(address, &[0; 12]), hex("0001a147e112a643"));
        assert_eq!(parse_xor_address(&hex("0001a147e112a643"), &[0; 12]), Some(address));
        let v6: SocketAddr = "[2001:db8:1234:5678:11:2233:4455:6677]:32853".parse().unwrap();
        let tid = [7; 12];
        assert_eq!(parse_xor_address(&xor_address(v6, &tid), &tid), Some(v6));
    }

    /// A scripted server: 401 challenge, authenticated allocation, permission, data in
    /// both directions, refresh and release.
    #[test]
    fn allocation_flow() {
        let server: SocketAddr = "198.51.100.1:3478".parse().unwrap();
        let credentials = Credentials { username: "1800000000:alice".into(), password: "secret".into() };
        let key = long_term_key(&credentials, b"slaxz.com");
        let now = Instant::now();
        let mut turn = Turn::new(server, credentials, now);

        // Unauthenticated Allocate → 401 with realm and nonce.
        let first = turn.poll_transmit().unwrap();
        let request = Message::parse(&first).unwrap();
        assert_eq!(request.message_type, ALLOCATE | CLASS_REQUEST);
        assert!(request.get(ATTR_MESSAGE_INTEGRITY).is_none());
        let challenge = Builder::new(ALLOCATE | CLASS_ERROR, request.transaction_id)
            .attribute(ATTR_ERROR_CODE, &[0, 0, 4, 1])
            .attribute(ATTR_REALM, b"slaxz.com")
            .attribute(ATTR_NONCE, b"nonce-1")
            .finish();
        turn.handle_input(&challenge, now);

        // Authenticated Allocate → success.
        let second = turn.poll_transmit().unwrap();
        let request = Message::parse(&second).unwrap();
        assert_eq!(request.get(ATTR_USERNAME), Some(&b"1800000000:alice"[..]));
        assert_eq!(request.get(ATTR_NONCE), Some(&b"nonce-1"[..]));
        assert!(request.verify_integrity(&key), "request is signed with the long-term key");
        let relayed: SocketAddr = "88.146.114.175:49170".parse().unwrap();
        let mapped: SocketAddr = "203.0.113.9:61000".parse().unwrap();
        let tid = request.transaction_id;
        let forged = Builder::new(ALLOCATE | CLASS_SUCCESS, tid).attribute(ATTR_XOR_RELAYED_ADDRESS, &xor_address(relayed, &tid)).integrity(b"other").finish();
        turn.handle_input(&forged, now);
        assert_eq!(turn.poll_event(), None, "a response with a wrong MAC is ignored");
        let success = Builder::new(ALLOCATE | CLASS_SUCCESS, tid)
            .attribute(ATTR_XOR_RELAYED_ADDRESS, &xor_address(relayed, &tid))
            .attribute(ATTR_XOR_MAPPED_ADDRESS, &xor_address(mapped, &tid))
            .attribute(ATTR_LIFETIME, &600u32.to_be_bytes())
            .integrity(&key)
            .finish();
        turn.handle_input(&success, now);
        assert_eq!(turn.poll_event(), Some(Event::Allocated { relayed, mapped: Some(mapped) }));
        assert_eq!(turn.relayed(), Some(relayed));

        // Sending to a peer: CreatePermission (signed) plus a Send indication.
        let peer: SocketAddr = "88.146.114.175:49200".parse().unwrap();
        turn.send(peer, b"hello", now);
        let permission = turn.poll_transmit().unwrap();
        let permission = Message::parse(&permission).unwrap();
        assert_eq!(permission.message_type, CREATE_PERMISSION | CLASS_REQUEST);
        assert!(permission.verify_integrity(&key));
        let indication = turn.poll_transmit().unwrap();
        let indication = Message::parse(&indication).unwrap();
        assert_eq!(indication.message_type, SEND | CLASS_INDICATION);
        assert_eq!(parse_xor_address(indication.get(ATTR_XOR_PEER_ADDRESS).unwrap(), &indication.transaction_id), Some(peer));
        assert_eq!(indication.get(ATTR_DATA), Some(&b"hello"[..]));
        let tid = permission.transaction_id;
        turn.handle_input(&Builder::new(CREATE_PERMISSION | CLASS_SUCCESS, tid).integrity(&key).finish(), now);
        turn.send(peer, b"again", now);
        let only_indication = Message::parse(&turn.poll_transmit().unwrap()).unwrap().message_type;
        assert_eq!(only_indication, SEND | CLASS_INDICATION);
        assert!(turn.poll_transmit().is_none(), "the permission is not requested twice");

        // Data from the peer.
        let tid = transaction_id();
        let data = Builder::new(DATA | CLASS_INDICATION, tid)
            .attribute(ATTR_XOR_PEER_ADDRESS, &xor_address(peer, &tid))
            .attribute(ATTR_DATA, b"world")
            .finish();
        turn.handle_input(&data, now);
        assert_eq!(turn.poll_event(), Some(Event::Data { peer, data: b"world".to_vec() }));

        // Refresh of allocation and permission before they expire.
        turn.handle_timeout(now + Duration::from_secs(541));
        let kinds: Vec<u16> = std::iter::from_fn(|| turn.poll_transmit()).map(|packet| Message::parse(&packet).unwrap().message_type).collect();
        assert!(kinds.contains(&(REFRESH | CLASS_REQUEST)), "{kinds:?}");
        assert!(kinds.contains(&(CREATE_PERMISSION | CLASS_REQUEST)), "{kinds:?}");

        // Stale nonce: the request is repeated with the new nonce.
        let refresh = turn.transactions.iter().find(|(_, t)| matches!(t.kind, Kind::Refresh { .. })).map(|(tid, _)| *tid).unwrap();
        let stale = Builder::new(REFRESH | CLASS_ERROR, refresh).attribute(ATTR_ERROR_CODE, &[0, 0, 4, 38]).attribute(ATTR_NONCE, b"nonce-2").finish();
        turn.handle_input(&stale, now);
        let retried = std::iter::from_fn(|| turn.poll_transmit()).find(|packet| Message::parse(packet).unwrap().message_type == REFRESH | CLASS_REQUEST).unwrap();
        assert_eq!(Message::parse(&retried).unwrap().get(ATTR_NONCE), Some(&b"nonce-2"[..]));

        // Release.
        let close = turn.close().unwrap();
        let close = Message::parse(&close).unwrap();
        assert_eq!(close.message_type, REFRESH | CLASS_REQUEST);
        assert_eq!(close.get(ATTR_LIFETIME), Some(&0u32.to_be_bytes()[..]));
        assert!(close.verify_integrity(&long_term_key(&Credentials { username: "1800000000:alice".into(), password: "secret".into() }, b"slaxz.com")));
        assert!(turn.relayed().is_none());
    }

    #[test]
    fn unreachable_server_fails_after_retransmissions() {
        let now = Instant::now();
        let mut turn = Turn::new("198.51.100.1:3478".parse().unwrap(), Credentials { username: "u".into(), password: "p".into() }, now);
        let mut sent = 0;
        let mut at = now;
        while turn.poll_event().is_none() {
            sent += std::iter::from_fn(|| turn.poll_transmit()).count();
            at += Duration::from_millis(100);
            turn.handle_timeout(at);
            assert!(at < now + Duration::from_secs(30), "gives up");
        }
        assert_eq!(sent, MAX_ATTEMPTS as usize);
        assert!(at < now + Duration::from_secs(16), "gives up after about 15 s, took {:?}", at - now);
    }

    #[test]
    fn rejected_credentials_fail() {
        let now = Instant::now();
        let mut turn = Turn::new("198.51.100.1:3478".parse().unwrap(), Credentials { username: "u".into(), password: "p".into() }, now);
        for _ in 0..=MAX_AUTH_RETRIES {
            let request = turn.poll_transmit().unwrap();
            let tid = Message::parse(&request).unwrap().transaction_id;
            let challenge = Builder::new(ALLOCATE | CLASS_ERROR, tid).attribute(ATTR_ERROR_CODE, &[0, 0, 4, 1]).attribute(ATTR_REALM, b"r").attribute(ATTR_NONCE, b"n").finish();
            turn.handle_input(&challenge, now);
        }
        assert!(matches!(turn.poll_event(), Some(Event::Failed(_))));
    }
}
