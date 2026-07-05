//! Licensing — local-first, offline-forever tier resolution.
//!
//! A license is a small signed token stored at `<data_root>/license.key` (a
//! manual drop-in at `~/.athanor/license.key` is also honored and imported).
//! It is verified LOCALLY against an embedded ed25519 public key — there is no
//! phone-home of any kind, and the app works fully offline on every tier,
//! including paid ones. An absent, malformed, tampered, or expired license
//! fails closed to Free; licensing can never crash the app or block a launch.
//!
//! Threat model, stated honestly: a determined user can patch the binary to
//! bypass the check. That is the accepted tradeoff of local-only validation
//! over a server-dependent DRM system (Tony's explicit choice). What the
//! signature *does* guarantee is that no valid Pro/Teams token can be forged
//! without BBA's private key — casual key-sharing/forgery is infeasible, which
//! is the real goal.
//!
//! The private signing key lives ONLY at `~/.athanor-release/license-signing.key`
//! on BBA's machine and is NEVER committed. Mint licenses with
//! `cargo run --example athanor_license -- mint <tier> <email> [days]`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AthanorError, Result};
use crate::workspaces::{data_root, write_atomic};

/// Embedded ed25519 public key (raw 32 bytes, base64). Replace with the output
/// of `cargo run --example athanor_license -- keygen`. Until a real key is
/// embedded, no license verifies and every launch is Free (the correct, safe
/// default) — Pro activation begins working the moment the real key is pasted.
const LICENSE_PUBKEY_B64: &str = "2qhI325NEMne75he3y7SwLPVY2AZj7dO9suBHiw0/R0=";

pub const LICENSE_FILENAME: &str = "license.key";

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

// ── Tier ──────────────────────────────────────────────────────

/// The four tiers, ordered by capability. `Ord` follows declaration order, so
/// `Free < Pro < Teams < Enterprise` — every gate is a single `>=` comparison.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    #[default]
    Free = 0,
    Pro = 1,
    Teams = 2,
    Enterprise = 3,
}

impl Tier {
    fn from_u8(v: u8) -> Tier {
        match v {
            1 => Tier::Pro,
            2 => Tier::Teams,
            3 => Tier::Enterprise,
            _ => Tier::Free,
        }
    }
    pub fn as_u8(self) -> u8 {
        self as u8
    }
    /// Human label used in the UI and error copy.
    pub fn label(self) -> &'static str {
        match self {
            Tier::Free => "Free",
            Tier::Pro => "Pro",
            Tier::Teams => "Teams",
            Tier::Enterprise => "Enterprise",
        }
    }
}

// The process-wide current tier. Lock-free reads keep `features::is_available`
// a cheap one-liner on any hot path. Set once at boot and on activate/deactivate.
static CURRENT_TIER: AtomicU8 = AtomicU8::new(0);

/// The tier this launch is currently operating at. Defaults to Free until
/// [`init`] runs; never blocks, never fails.
pub fn current_tier() -> Tier {
    Tier::from_u8(CURRENT_TIER.load(Ordering::Relaxed))
}

fn set_current_tier(t: Tier) {
    CURRENT_TIER.store(t.as_u8(), Ordering::Relaxed);
}

// ── License token ─────────────────────────────────────────────

/// The signed content of a license. Every field carries `#[serde(default)]` so
/// an older/newer token (or a hand-edit that drops a field) still deserializes
/// to a usable value instead of failing — same durability contract as the rest
/// of the app's persisted state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LicensePayload {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub tier: Tier,
    #[serde(default)]
    pub license_id: String,
    #[serde(default)]
    pub email: String,
    /// "monthly" | "yearly" | "lifetime".
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub seats: u32,
    #[serde(default)]
    pub issued_at: String,
    /// RFC3339 timestamp or bare `YYYY-MM-DD`; `None` = perpetual.
    #[serde(default)]
    pub expires: Option<String>,
}

/// The on-disk envelope: the exact signed payload bytes (base64) plus its
/// detached ed25519 signature (base64). Storing the signed bytes verbatim
/// sidesteps any JSON-canonicalization ambiguity between signer and verifier.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LicenseFile {
    #[serde(default)]
    pub payload_b64: String,
    #[serde(default)]
    pub sig_b64: String,
}

// ── Resolution ────────────────────────────────────────────────

/// The resolved licensing state held in app state for status queries.
#[derive(Default, Clone)]
pub struct Resolved {
    pub tier: Tier,
    pub payload: Option<LicensePayload>,
    /// A license was present and valid-by-signature but past its expiry, so the
    /// tier was downgraded to Free. Lets the UI say "renew" vs "upgrade".
    pub expired: bool,
}

/// Managed Tauri state: the resolved license. Poison-proof, mirroring `WsLock`.
#[derive(Default)]
pub struct LicenseState(pub Mutex<Resolved>);

impl LicenseState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Resolved> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn embedded_pubkey() -> Option<Vec<u8>> {
    b64().decode(LICENSE_PUBKEY_B64).ok().filter(|k| k.len() == 32)
}

/// Verify an envelope against a specific public key and return its payload.
/// Split out so tests can exercise the full crypto path with an ephemeral key.
fn verify_with(pubkey: &[u8], file: &LicenseFile) -> Result<LicensePayload> {
    let payload_bytes = b64()
        .decode(file.payload_b64.trim())
        .map_err(|_| AthanorError::License("license payload is not valid base64".into()))?;
    let sig = b64()
        .decode(file.sig_b64.trim())
        .map_err(|_| AthanorError::License("license signature is not valid base64".into()))?;
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, pubkey);
    key.verify(&payload_bytes, &sig)
        .map_err(|_| AthanorError::License("license signature does not verify".into()))?;
    serde_json::from_slice::<LicensePayload>(&payload_bytes)
        .map_err(|_| AthanorError::License("license payload is malformed".into()))
}

/// Verify against the embedded production key.
fn verify(file: &LicenseFile) -> Result<LicensePayload> {
    let pk = embedded_pubkey().ok_or_else(|| {
        AthanorError::License("no license verification key is embedded in this build".into())
    })?;
    verify_with(&pk, file)
}

/// Whether a validated payload is past its expiry. Fails closed: an unparseable
/// expiry counts as expired.
fn is_expired(payload: &LicensePayload) -> bool {
    let Some(s) = payload.expires.as_deref() else {
        return false; // perpetual
    };
    let s = s.trim();
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return chrono::Utc::now() > dt.with_timezone(&chrono::Utc);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return chrono::Utc::now().date_naive() > d;
    }
    true
}

fn resolve(file: &LicenseFile) -> Resolved {
    match verify(file) {
        Ok(payload) => {
            if is_expired(&payload) {
                Resolved { tier: Tier::Free, payload: Some(payload), expired: true }
            } else {
                Resolved { tier: payload.tier, payload: Some(payload), expired: false }
            }
        }
        Err(e) => {
            log::warn!(target: "license", "license present but not honored: {e}");
            Resolved::default()
        }
    }
}

// ── Paths ─────────────────────────────────────────────────────

fn license_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_root(app)?.join(LICENSE_FILENAME))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// A manual drop-in location honored as an import source: `~/.athanor/license.key`.
fn dropin_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".athanor").join(LICENSE_FILENAME))
}

/// Read the license envelope from the canonical path, falling back to the
/// drop-in location (which, if present and parseable, is imported into the
/// canonical path so it survives portable moves and future reads).
fn read_file(app: &AppHandle) -> Option<LicenseFile> {
    if let Ok(path) = license_path(app) {
        if let Ok(bytes) = std::fs::read(&path) {
            match serde_json::from_slice::<LicenseFile>(&bytes) {
                Ok(f) => return Some(f),
                Err(e) => log::warn!(target: "license", "license.key unreadable ({e}); ignoring"),
            }
        }
    }
    if let Some(dp) = dropin_path() {
        if let Ok(bytes) = std::fs::read(&dp) {
            if let Ok(f) = serde_json::from_slice::<LicenseFile>(&bytes) {
                if let Ok(path) = license_path(app) {
                    let _ = write_atomic(&path, &bytes); // best-effort import
                }
                return Some(f);
            }
        }
    }
    None
}

fn read_resolved(app: &AppHandle) -> Resolved {
    match read_file(app) {
        Some(f) => resolve(&f),
        None => Resolved::default(),
    }
}

// ── Public surface ────────────────────────────────────────────

/// Resolve the license at boot and publish the tier. Called once from `setup()`.
pub fn init(app: &AppHandle) {
    let resolved = read_resolved(app);
    set_current_tier(resolved.tier);
    if let Some(state) = app.try_state::<LicenseState>() {
        *state.lock() = resolved;
    }
    log::info!(target: "license", "licensed tier: {}", current_tier().label());
}

fn status_from(r: &Resolved) -> LicenseStatus {
    let (email, plan, expires, seats) = match &r.payload {
        Some(p) => (
            (!p.email.is_empty()).then(|| p.email.clone()),
            (!p.plan.is_empty()).then(|| p.plan.clone()),
            p.expires.clone(),
            p.seats,
        ),
        None => (None, None, None, 0),
    };
    LicenseStatus {
        tier: r.tier,
        tier_label: r.tier.label().to_string(),
        licensed: r.tier != Tier::Free,
        email,
        plan,
        expires,
        expired: r.expired,
        seats,
    }
}

/// The current licensing status for the UI.
pub fn status(app: &AppHandle) -> LicenseStatus {
    status_from(&app.state::<LicenseState>().lock())
}

/// Activate a pasted license key: verify it, persist it atomically, and flip the
/// live tier. Returns a friendly error (rendered as a designed state, never a
/// raw string) if the key is malformed, forged, or expired.
pub fn activate(app: &AppHandle, key_text: &str) -> Result<LicenseStatus> {
    let file: LicenseFile = serde_json::from_str(key_text.trim())
        .map_err(|_| AthanorError::License("that doesn't look like an Athanor license key".into()))?;
    let payload = verify(&file)?;
    if is_expired(&payload) {
        return Err(AthanorError::License(
            "this license has expired — renew it to reactivate".into(),
        ));
    }
    write_atomic(&license_path(app)?, serde_json::to_string_pretty(&file)?.as_bytes())?;
    let resolved = Resolved { tier: payload.tier, payload: Some(payload), expired: false };
    set_current_tier(resolved.tier);
    let out = status_from(&resolved);
    *app.state::<LicenseState>().lock() = resolved;
    log::info!(target: "license", "activated tier {}", current_tier().label());
    Ok(out)
}

/// Remove the local license and return to Free.
pub fn deactivate(app: &AppHandle) -> Result<LicenseStatus> {
    if let Ok(path) = license_path(app) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(dp) = dropin_path() {
        let _ = std::fs::remove_file(dp);
    }
    let resolved = Resolved::default();
    set_current_tier(Tier::Free);
    *app.state::<LicenseState>().lock() = resolved;
    log::info!(target: "license", "license removed; tier now Free");
    Ok(status_from(&Resolved::default()))
}

/// The status view returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseStatus {
    pub tier: Tier,
    pub tier_label: String,
    /// True when a valid non-Free license is active.
    pub licensed: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub expires: Option<String>,
    /// A license exists but is past expiry (tier downgraded to Free).
    pub expired: bool,
    pub seats: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    fn keypair() -> ring::signature::Ed25519KeyPair {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    fn mint(payload: &LicensePayload, kp: &ring::signature::Ed25519KeyPair) -> LicenseFile {
        let bytes = serde_json::to_vec(payload).unwrap();
        let sig = kp.sign(&bytes);
        LicenseFile {
            payload_b64: b64().encode(&bytes),
            sig_b64: b64().encode(sig.as_ref()),
        }
    }

    #[test]
    fn valid_license_resolves_to_its_tier() {
        let kp = keypair();
        let pk = kp.public_key().as_ref().to_vec();
        let file = mint(
            &LicensePayload { tier: Tier::Pro, email: "a@b.co".into(), ..Default::default() },
            &kp,
        );
        assert_eq!(verify_with(&pk, &file).unwrap().tier, Tier::Pro);
    }

    #[test]
    fn forged_payload_is_rejected() {
        let kp = keypair();
        let pk = kp.public_key().as_ref().to_vec();
        let mut file = mint(&LicensePayload { tier: Tier::Free, ..Default::default() }, &kp);
        // Swap in a Pro payload the signature was never made over.
        let forged =
            serde_json::to_vec(&LicensePayload { tier: Tier::Enterprise, ..Default::default() })
                .unwrap();
        file.payload_b64 = b64().encode(&forged);
        assert!(verify_with(&pk, &file).is_err());
    }

    #[test]
    fn signature_from_a_different_key_is_rejected() {
        let signer = keypair();
        let stranger = keypair();
        let file = mint(&LicensePayload { tier: Tier::Pro, ..Default::default() }, &signer);
        assert!(verify_with(stranger.public_key().as_ref(), &file).is_err());
    }

    #[test]
    fn expiry_is_enforced() {
        assert!(is_expired(&LicensePayload {
            expires: Some("2000-01-01".into()),
            ..Default::default()
        }));
        assert!(!is_expired(&LicensePayload {
            expires: Some("2999-01-01".into()),
            ..Default::default()
        }));
        assert!(!is_expired(&LicensePayload { expires: None, ..Default::default() }));
        // Unparseable expiry fails closed (treated as expired).
        assert!(is_expired(&LicensePayload {
            expires: Some("whenever".into()),
            ..Default::default()
        }));
    }

    #[test]
    fn tier_is_totally_ordered() {
        assert!(Tier::Free < Tier::Pro);
        assert!(Tier::Pro < Tier::Teams);
        assert!(Tier::Teams < Tier::Enterprise);
    }

    #[test]
    fn missing_fields_deserialize_to_free() {
        // A stripped/older token must not fail to parse.
        let p: LicensePayload = serde_json::from_str("{}").unwrap();
        assert_eq!(p.tier, Tier::Free);
    }

    #[test]
    fn embedded_pubkey_is_well_formed() {
        // Guards against a broken paste of LICENSE_PUBKEY_B64: it must decode to
        // exactly 32 bytes (a valid ed25519 public key), or every real license
        // would silently fail to verify.
        assert!(
            embedded_pubkey().is_some(),
            "LICENSE_PUBKEY_B64 must be valid base64 decoding to 32 bytes"
        );
    }
}
