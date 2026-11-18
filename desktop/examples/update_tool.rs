//! Release helper for signed in-app updates.
//!
//!   cargo run --example update_tool -- keygen <private-key-file>
//!       Creates a new Ed25519 key. Writes the base64 seed to the file (mode 0600) and
//!       prints the public key for `update-public-key.txt`.
//!   cargo run --example update_tool -- manifest <installer.exe> <version> <download-url> [notes-file]
//!       Signs the installer with the key in $UPDATE_SIGNING_KEY (base64 seed) and prints
//!       latest.json to stdout. Refuses to run without the key.
use std::io::Read;

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(args.get(1).expect("usage: keygen <private-key-file>")),
        Some("manifest") if args.len() >= 4 => manifest(&args[1], &args[2], &args[3], args.get(4)),
        _ => {
            eprintln!("usage: update_tool keygen <file> | manifest <installer> <version> <url> [notes-file]");
            std::process::exit(2);
        }
    }
}

fn keygen(path: &str) {
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut random| random.read_exact(&mut seed)).expect("read /dev/urandom");
    let key = SigningKey::from_bytes(&seed);
    let engine = base64::engine::general_purpose::STANDARD;
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(path).expect("private key file must not exist yet");
        writeln!(file, "{}", engine.encode(seed)).expect("write key");
    }
    println!("{}", engine.encode(key.verifying_key().to_bytes()));
}

fn manifest(installer: &str, version: &str, url: &str, notes: Option<&String>) {
    let engine = base64::engine::general_purpose::STANDARD;
    let encoded = std::env::var("UPDATE_SIGNING_KEY").expect("UPDATE_SIGNING_KEY is not set");
    let seed: [u8; 32] = engine.decode(encoded.trim()).expect("key is base64").try_into().expect("key is 32 bytes");
    let key = SigningKey::from_bytes(&seed);
    let version = semver::Version::parse(version.trim_start_matches('v')).expect("version is semver");
    let bytes = std::fs::read(installer).expect("installer readable");
    let sha256: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    let message = format!("messenger-update-v1\n{version}\n{sha256}\n");
    let signature = engine.encode(key.sign(message.as_bytes()).to_bytes());
    let notes = notes.map(|path| std::fs::read_to_string(path).expect("notes readable")).unwrap_or_default();
    let manifest = serde_json::json!({
        "version": version.to_string(),
        "notes": notes.trim(),
        "pub_date": chrono::Utc::now().format("%Y-%m-%d").to_string(),
        "platforms": { "windows-x86_64": { "url": url, "sha256": sha256, "size": bytes.len(), "signature": signature } },
    });
    println!("{}", serde_json::to_string_pretty(&manifest).expect("json"));
}
