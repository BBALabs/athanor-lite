//! Athanor license minting tool — BBA-internal, never shipped in the app.
//!
//! The private signing key lives ONLY at `~/.athanor-release/license-signing.key`
//! and is NEVER committed. Public verification key is embedded in the app
//! (`licensing::LICENSE_PUBKEY_B64`).
//!
//!   cargo run --example athanor_license -- keygen
//!       Generate the ed25519 signing key (once) and print the public key to
//!       paste into src/licensing/mod.rs.
//!
//!   cargo run --example athanor_license -- mint <free|pro|teams|enterprise> <email> [days] [plan] [seats]
//!       Print a signed license.key JSON envelope to stdout. `days` omitted or 0
//!       = perpetual. Redirect to a file to deliver to a customer.
//!
//!   cargo run --example athanor_license -- verify <path-to-license.key>
//!       Verify a license against the local signing key and print its payload.

use std::path::PathBuf;

use base64::Engine as _;
use ring::signature::{Ed25519KeyPair, KeyPair};

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .expect("no USERPROFILE/HOME in environment")
}

fn key_path() -> PathBuf {
    home().join(".athanor-release").join("license-signing.key")
}

fn load_keypair() -> Ed25519KeyPair {
    let path = key_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read signing key at {path:?}: {e}\nrun `keygen` first"));
    let pkcs8 = b64().decode(raw.trim()).expect("signing key is not valid base64");
    Ed25519KeyPair::from_pkcs8(&pkcs8).expect("signing key is not a valid ed25519 pkcs8 key")
}

fn keygen() {
    let path = key_path();
    if path.exists() {
        eprintln!("refusing to overwrite existing signing key at {path:?}");
        eprintln!("delete it deliberately if you truly mean to rotate the key.");
        std::process::exit(1);
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("keygen failed");
    let kp = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
    std::fs::write(&path, b64().encode(pkcs8.as_ref())).expect("failed to write signing key");
    println!("signing key written to {path:?}  (NEVER commit this file)");
    println!();
    println!("paste this into src/licensing/mod.rs as LICENSE_PUBKEY_B64:");
    println!("{}", b64().encode(kp.public_key().as_ref()));
}

fn mint(args: &[String]) {
    let tier = args.first().map(String::as_str).unwrap_or("");
    if !matches!(tier, "free" | "pro" | "teams" | "enterprise") {
        eprintln!("tier must be one of: free pro teams enterprise");
        std::process::exit(1);
    }
    let email = args.get(1).cloned().unwrap_or_default();
    let days: i64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let plan = args.get(3).cloned().unwrap_or_else(|| "yearly".into());
    let seats: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);

    let now = chrono::Utc::now();
    let expires = if days > 0 {
        serde_json::Value::String((now + chrono::Duration::days(days)).to_rfc3339())
    } else {
        serde_json::Value::Null
    };

    // Field names must match licensing::LicensePayload (camelCase serde).
    let payload = serde_json::json!({
        "schema": 1,
        "tier": tier,
        "licenseId": uuid::Uuid::new_v4().to_string(),
        "email": email,
        "plan": plan,
        "seats": seats,
        "issuedAt": now.to_rfc3339(),
        "expires": expires,
    });
    let payload_bytes = serde_json::to_vec(&payload).unwrap();

    let kp = load_keypair();
    let sig = kp.sign(&payload_bytes);
    let envelope = serde_json::json!({
        "payloadB64": b64().encode(&payload_bytes),
        "sigB64": b64().encode(sig.as_ref()),
    });
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
}

fn verify(args: &[String]) {
    let path = args.first().expect("usage: verify <path-to-license.key>");
    let raw = std::fs::read_to_string(path).expect("cannot read license file");
    let file: serde_json::Value = serde_json::from_str(&raw).expect("license file is not JSON");
    let payload_b64 = file["payloadB64"].as_str().expect("missing payloadB64");
    let sig_b64 = file["sigB64"].as_str().expect("missing sigB64");
    let payload_bytes = b64().decode(payload_b64).expect("payloadB64 not base64");
    let sig = b64().decode(sig_b64).expect("sigB64 not base64");

    let kp = load_keypair();
    let pk = ring::signature::UnparsedPublicKey::new(
        &ring::signature::ED25519,
        kp.public_key().as_ref(),
    );
    match pk.verify(&payload_bytes, &sig) {
        Ok(()) => {
            let payload: serde_json::Value = serde_json::from_slice(&payload_bytes).unwrap();
            println!("VALID — payload:");
            println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        }
        Err(_) => {
            eprintln!("INVALID — signature does not verify against the local signing key");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => keygen(),
        Some("mint") => mint(&args[1..]),
        Some("verify") => verify(&args[1..]),
        _ => {
            eprintln!("usage:");
            eprintln!("  athanor_license keygen");
            eprintln!("  athanor_license mint <free|pro|teams|enterprise> <email> [days] [plan] [seats]");
            eprintln!("  athanor_license verify <path-to-license.key>");
            std::process::exit(1);
        }
    }
}
