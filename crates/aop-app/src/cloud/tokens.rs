//! Where the tokens live between one run of the application and the next.
//!
//! A refresh token is a standing permission to act as the person who signed in.
//! It is not a preference, so it does not sit in `config.cfg` beside the theme
//! in plain text where a backup, a screenshot or a support bundle would carry
//! it away.
//!
//! The house answer to this is not to look for somewhere to hide a key. There
//! is nowhere good: a key file beside the ciphertext it unlocks is the
//! ciphertext in cleartext with extra steps, and a desktop secret service is
//! not there on every machine this has to run on. So the key is not stored at
//! all. It is **derived from the machine**, out of the same hardware identity
//! the sign in server binds the session to, and it exists only for as long as
//! the calculation that produced it.
//!
//! ```text
//!   device exact tier  ──┐
//!   (anchor, board,      ├── HKDF-SHA256 ──┬── encryption key ──┐
//!    platform, cpu)      │                 └── signing key ───┐ │
//!   per install salt ────┘                                    │ │
//!   (public, kept beside the blob)                            v v
//!                                                        sealed blob
//!                                                     in cloud.cfg as
//!                                                     enc_session = ...
//! ```
//!
//! The salt is public on purpose. It is not what protects anything; it is there
//! so two machines with identical hardware do not end up with the same key. The
//! protection is that the exact tier of the device identity is not in the file
//! and cannot be reconstructed anywhere else. Copy `cloud.cfg` to another
//! machine and it is a paragraph of hex.
//!
//! A blob that will not open is not an error. It means the machine changed
//! underneath it, and the honest reading of that is that nobody is signed in.
//! The user signs in again; they do not get a stack trace about a MAC tag.
//!
//! ## What this is not
//!
//! The sealing below is HKDF-SHA256 for the keys, SHA-256 in counter mode for
//! the keystream, and HMAC-SHA256 over the ciphertext, in that order: encrypt
//! then authenticate, with independent keys. It is built from the one hash this
//! crate already has rather than from a purpose built construction, and the
//! moment a dependency can be added it should become ChaCha20-Poly1305 or
//! whatever `alterion-encrypt` offers. [`seal`] and [`unseal`] are the only two
//! functions that would change.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::cloud::device;

/// How long before an access token expires it is renewed.
///
/// Renewing after the fact means the first request that notices is the one that
/// fails, and that request is usually a sync somebody is waiting on. Two
/// minutes is comfortably longer than a slow upload and far shorter than the
/// hour these tokens last, so in practice nothing ever meets an expired one.
pub const EARLY_REFRESH_SECONDS: i64 = 120;

/// Everything needed to pick a session back up, and nothing else.
///
/// Serialised as JSON, then sealed, so a store only ever handles one opaque
/// string.
#[derive(Clone, Serialize, Deserialize)]
pub struct Stored {
    /// Which identity provider issued it, so a session is never replayed
    /// against a server it did not come from.
    pub issuer: String,
    pub client_id: String,
    pub access_token: String,
    /// Empty when the server issued none, in which case the session ends when
    /// the access token does.
    #[serde(default)]
    pub refresh_token: String,
    /// When the access token stops being accepted, as a Unix timestamp.
    pub expires_at: i64,
    pub subject: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

/// Written out by hand so that a token cannot reach a log through a stray
/// `{:?}`. Everything in this record except the tokens is safe to print, and
/// the tokens are the entire reason the record is sealed.
impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("access_token", &"(withheld)")
            .field("refresh_token", &"(withheld)")
            .field("expires_at", &self.expires_at)
            .field("subject", &self.subject)
            .field("email", &self.email)
            .finish()
    }
}

// ------------------------------------------------------------- randomness

/// Random bytes from the operating system.
///
/// There is no fallback to a weaker source anywhere in this module. A salt or a
/// nonce that can be guessed is not a salt or a nonce, and a sign in that
/// cannot be made unguessable does not happen at all.
#[cfg(unix)]
pub(crate) fn random_bytes(count: usize) -> Option<Vec<u8>> {
    use std::io::Read;

    let mut bytes = vec![0u8; count];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .ok()?;
    Some(bytes)
}

#[cfg(windows)]
pub(crate) fn random_bytes(count: usize) -> Option<Vec<u8>> {
    // RtlGenRandom, which Windows has always exported under this name. It is
    // the system generator and needs no crypto context set up first.
    #[link(name = "advapi32")]
    unsafe extern "system" {
        #[link_name = "SystemFunction036"]
        fn rtl_gen_random(buffer: *mut u8, length: u32) -> u8;
    }

    let mut bytes = vec![0u8; count];
    let length = u32::try_from(count).ok()?;
    // Safe: the pointer and the length describe the vector allocated above.
    let filled = unsafe { rtl_gen_random(bytes.as_mut_ptr(), length) };
    if filled == 0 {
        return None;
    }
    Some(bytes)
}

// ------------------------------------------------------------------ hashes

/// HMAC-SHA256, from the hash this crate already depends on.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    const BLOCK: usize = 64;

    let mut padded = [0u8; BLOCK];
    if key.len() > BLOCK {
        padded[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK];
    let mut outer_pad = [0u8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] = padded[index] ^ 0x36;
        outer_pad[index] = padded[index] ^ 0x5c;
    }

    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner);

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer.finalize());
    out
}

/// HKDF-SHA256 (RFC 5869): extract entropy from the device identity, then
/// expand it into as many independent keys as are wanted.
///
/// Two separate keys rather than one used twice, because a key that both
/// encrypts and authenticates is a key doing two jobs it was not asked to do at
/// once.
fn derive(salt: &[u8], material: &[u8], info: &str, length: usize) -> Vec<u8> {
    let prk = hmac_sha256(salt, material);

    let mut out = Vec::with_capacity(length);
    let mut previous: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while out.len() < length {
        let mut block = Vec::with_capacity(previous.len() + info.len() + 1);
        block.extend_from_slice(&previous);
        block.extend_from_slice(info.as_bytes());
        block.push(counter);

        let digest = hmac_sha256(&prk, &block);
        out.extend_from_slice(&digest);
        previous = digest.to_vec();
        counter += 1;
    }
    out.truncate(length);
    out
}

/// Compare two tags without letting how long it took say how much matched.
fn same_bytes(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

// -------------------------------------------------------------- the sealing

/// The format the blob is written in, so a later one can be told apart from
/// this one rather than mis-read as it.
const SEAL_VERSION: u8 = 1;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 32;
const SALT_BYTES: usize = 16;

/// The keystream: SHA-256 in counter mode.
fn keystream(key: &[u8], nonce: &[u8], length: usize) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut out = Vec::with_capacity(length.div_ceil(32) * 32);
    let mut counter: u32 = 0;
    while out.len() < length {
        let mut block = Sha256::new();
        block.update(key);
        block.update(nonce);
        block.update(counter.to_be_bytes());
        out.extend_from_slice(&block.finalize());
        counter += 1;
    }
    out.truncate(length);
    out
}

/// Seal a plaintext against a machine.
///
/// Returns hex, because the blob shares a line in a text file with a key name
/// and has to survive being looked at.
pub fn seal(material: &[u8], salt: &[u8], plaintext: &[u8]) -> Option<String> {
    let nonce = random_bytes(NONCE_BYTES)?;

    let encryption_key = derive(salt, material, "alterion-open-project/seal/v1/encrypt", 32);
    let signing_key = derive(salt, material, "alterion-open-project/seal/v1/sign", 32);

    let stream = keystream(&encryption_key, &nonce, plaintext.len());
    let ciphertext: Vec<u8> = plaintext
        .iter()
        .zip(stream.iter())
        .map(|(byte, mask)| byte ^ mask)
        .collect();

    // Encrypt then authenticate, over the version and the nonce as well as the
    // ciphertext, so none of the three can be swapped for another blob's.
    let mut signed = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
    signed.push(SEAL_VERSION);
    signed.extend_from_slice(&nonce);
    signed.extend_from_slice(&ciphertext);
    let tag = hmac_sha256(&signing_key, &signed);

    let mut blob = Vec::with_capacity(1 + NONCE_BYTES + TAG_BYTES + ciphertext.len());
    blob.push(SEAL_VERSION);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&tag);
    blob.extend_from_slice(&ciphertext);
    Some(to_hex(&blob))
}

/// Open a sealed blob, or decide it does not belong to this machine.
///
/// `None` covers every way this can go: a different machine, a truncated file,
/// a hand edit, a blob from a future version. They all mean the same thing to
/// the caller, which is that there is no session here.
pub fn unseal(material: &[u8], salt: &[u8], blob: &str) -> Option<Vec<u8>> {
    let blob = from_hex(blob)?;
    if blob.len() < 1 + NONCE_BYTES + TAG_BYTES || blob[0] != SEAL_VERSION {
        return None;
    }

    let nonce = &blob[1..1 + NONCE_BYTES];
    let tag = &blob[1 + NONCE_BYTES..1 + NONCE_BYTES + TAG_BYTES];
    let ciphertext = &blob[1 + NONCE_BYTES + TAG_BYTES..];

    let signing_key = derive(salt, material, "alterion-open-project/seal/v1/sign", 32);
    let mut signed = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
    signed.push(SEAL_VERSION);
    signed.extend_from_slice(nonce);
    signed.extend_from_slice(ciphertext);

    // Checked before anything is decrypted: a blob that was not written by this
    // machine is not something to start unpicking.
    if !same_bytes(&hmac_sha256(&signing_key, &signed), tag) {
        return None;
    }

    let encryption_key = derive(salt, material, "alterion-open-project/seal/v1/encrypt", 32);
    let stream = keystream(&encryption_key, nonce, ciphertext.len());
    Some(
        ciphertext
            .iter()
            .zip(stream.iter())
            .map(|(byte, mask)| byte ^ mask)
            .collect(),
    )
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

// -------------------------------------------------------------- the store

/// Somewhere to keep a signed in session.
///
/// Three operations, none of which say anything about files, so the sealed
/// store, a memory store, or something a later build brings can each satisfy
/// it.
pub trait TokenStore: Send + Sync {
    /// The session from last time, if there was one and this machine can still
    /// open it. A blob that will not open is not a failure: it is nobody being
    /// signed in.
    fn load(&self) -> Option<Stored>;

    /// Keep a session. The error is for telling the user, so it says what went
    /// wrong in terms of their machine rather than in terms of an API.
    fn save(&self, session: &Stored) -> Result<(), String>;

    /// Forget it, on sign out.
    fn clear(&self) -> Result<(), String>;

    /// Where this store puts things, for saying so in the interface. A person
    /// deciding whether to stay signed in on a shared machine deserves to know.
    fn describe(&self) -> &'static str;
}

/// Where the sealed session is kept.
///
/// Its own file rather than a section of `config.cfg`, for a mundane reason
/// that would otherwise cost the user their sign in: [`crate::settings`] writes
/// `config.cfg` by rendering the whole file from the settings it knows about,
/// so a key it has never heard of is dropped the next time any preference
/// changes. The format is the same, and the file sits beside it.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("alterion-open-project").join("cloud.cfg"))
}

/// The sealed file store.
pub struct SealedStore {
    path: PathBuf,
}

impl SealedStore {
    pub fn at(path: PathBuf) -> SealedStore {
        SealedStore { path }
    }

    /// Read the file into its `key = value` pairs, the same shape `config.cfg`
    /// uses so anybody who opens it recognises what they are looking at.
    fn read(&self) -> Vec<(String, String)> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('['))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect()
    }

    fn value(&self, wanted: &str) -> Option<String> {
        self.read()
            .into_iter()
            .find(|(key, _)| key == wanted)
            .map(|(_, value)| value)
            .filter(|value| !value.is_empty())
    }

    /// The salt for this install, making one the first time.
    ///
    /// Public by design and kept in plain sight: it is not what protects the
    /// blob, it is what stops two machines with identical hardware from
    /// deriving the same key.
    fn salt(&self) -> Option<Vec<u8>> {
        self.value("salt").and_then(|hex| from_hex(&hex))
    }

    fn write(&self, salt: &[u8], sealed: &str) -> Result<(), String> {
        let text = format!(
            "# Alterion Open Project: the signed in cloud account.\n\
             # Not meant to be edited. The sealed value below can only be opened\n\
             # on the machine that wrote it, so copying this file elsewhere\n\
             # achieves nothing. Delete it to sign out.\n\
             \n\
             [cloud]\n\
             salt = {}\n\
             enc_session = {sealed}\n",
            to_hex(salt)
        );

        if let Some(parent) = self.path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return Err(format!(
                "The sign in could not be saved: {} could not be created. \
                 You will be asked to sign in again next time.",
                parent.display()
            ));
        }
        std::fs::write(&self.path, text).map_err(|error| {
            format!(
                "The sign in could not be saved to {}: {error}. \
                 You will be asked to sign in again next time.",
                self.path.display()
            )
        })
    }
}

impl TokenStore for SealedStore {
    fn load(&self) -> Option<Stored> {
        let material = device::components().ok()?.key_material();
        let salt = self.salt()?;
        let sealed = self.value("enc_session")?;

        // Nothing is deleted on a failure to open. A machine identity that is
        // briefly unreadable would otherwise take the session with it, and the
        // next successful sign in overwrites the file anyway.
        let plaintext = unseal(&material, &salt, &sealed)?;
        serde_json::from_slice(&plaintext).ok()
    }

    fn save(&self, session: &Stored) -> Result<(), String> {
        let components = device::components().map_err(ToString::to_string)?;
        let material = components.key_material();

        // The salt is kept once and reused, so a re-seal does not orphan the
        // blob that is already there.
        let salt = match self.salt() {
            Some(salt) if salt.len() == SALT_BYTES => salt,
            _ => random_bytes(SALT_BYTES).ok_or_else(|| {
                "The sign in could not be saved: the system random number generator \
                 could not be read."
                    .to_string()
            })?,
        };

        let plaintext = serde_json::to_vec(session)
            .map_err(|_| "The sign in could not be prepared for saving.".to_string())?;
        let sealed = seal(&material, &salt, &plaintext).ok_or_else(|| {
            "The sign in could not be saved: the system random number generator \
             could not be read."
                .to_string()
        })?;

        self.write(&salt, &sealed)
    }

    fn clear(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            // Already gone is the outcome that was wanted.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!(
                "Signed out, but {} could not be removed: {error}. Delete it by hand.",
                self.path.display()
            )),
        }
    }

    fn describe(&self) -> &'static str {
        "Sealed to this machine and kept in cloud.cfg beside your settings. \
         The key is worked out from the hardware and is never written down, so \
         the file is useless on any other machine."
    }
}

/// A store that lasts as long as the process does.
///
/// What the application falls back to when there is nowhere to put a file. Not
/// a placeholder that loses data quietly: signing in again after a restart is a
/// nuisance, whereas a token written somewhere it cannot be protected is a
/// problem that outlives the machine.
#[derive(Default)]
pub struct MemoryStore {
    held: Mutex<Option<Stored>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }
}

impl TokenStore for MemoryStore {
    fn load(&self) -> Option<Stored> {
        self.held.lock().ok()?.clone()
    }

    fn save(&self, session: &Stored) -> Result<(), String> {
        let mut held = self.held.lock().map_err(|_| {
            "The sign in could not be recorded. Restart the application.".to_string()
        })?;
        *held = Some(session.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        if let Ok(mut held) = self.held.lock() {
            *held = None;
        }
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "Kept in memory only, because there is nowhere on this machine to write it. \
         Signing in again will be needed after a restart."
    }
}

/// The store the application is using.
static STORE: OnceLock<Box<dyn TokenStore>> = OnceLock::new();

/// Choose where sessions are kept.
///
/// Called once at start up, before anything signs in. A second call is ignored
/// rather than swapping the store out from under a live session, and says so,
/// so a mistake in the start up order is visible rather than silent.
pub fn use_store(store: Box<dyn TokenStore>) -> bool {
    STORE.set(store).is_ok()
}

/// The store: the sealed file, or memory when there is nowhere to put a file.
pub fn store() -> &'static dyn TokenStore {
    STORE
        .get_or_init(|| match config_path() {
            Some(path) => Box::new(SealedStore::at(path)),
            None => Box::new(MemoryStore::new()),
        })
        .as_ref()
}

// ------------------------------------------------------------- expiry maths

/// When a token issued now, lasting this many seconds, stops being accepted.
///
/// A server that says nothing about the lifetime is treated as having issued
/// one that is already over, so the first use renews rather than gambling on a
/// token that may already be dead.
pub fn expires_at(now: DateTime<Utc>, expires_in: i64) -> DateTime<Utc> {
    now + TimeDelta::try_seconds(expires_in.max(0)).unwrap_or_default()
}

/// Whether it is time to renew.
///
/// True from a couple of minutes before the token expires, not from the moment
/// it has, so a request already in flight finishes against a token that is
/// still good.
pub fn needs_refresh(now: DateTime<Utc>, expires_at: DateTime<Utc>) -> bool {
    let margin = TimeDelta::try_seconds(EARLY_REFRESH_SECONDS).unwrap_or_default();
    now + margin >= expires_at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("a timestamp in range")
    }

    fn sample() -> Stored {
        Stored {
            issuer: "https://auth.coraldune.cloud".into(),
            client_id: "alterion-open-project".into(),
            access_token: "an-access-token".into(),
            refresh_token: "a-refresh-token".into(),
            expires_at: 1_800_000_000,
            subject: "0198f0c2-0000-7000-8000-000000000000".into(),
            name: "Ada Lovelace".into(),
            email: "ada@example.org".into(),
        }
    }

    fn machine() -> device::DeviceComponents {
        device::DeviceComponents {
            anchor: "9f2c1e7a4b8d4c3e9a1f0b7d6c5e4a3b".into(),
            webgl: "Acme|X570|Acme Inc|Desktop".into(),
            platform: "linux|x86_64".into(),
            cpu: "Acme Ryzen 9 5950X|32".into(),
            screen: "workstation".into(),
            pixel_ratio: String::new(),
        }
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("aop-cloud-{}-{name}", std::process::id()))
    }

    // ------------------------------------------------ the hashes themselves

    #[test]
    fn hmac_matches_the_rfc_4231_vector() {
        // Written by hand, so it is checked against the specification's own
        // worked example rather than against itself.
        let tag = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            to_hex(&tag),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_handles_a_key_longer_than_a_block() {
        // Test case 6 of the same document: a 131 byte key, which has to be
        // hashed down before it is padded.
        let tag = hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First",
        );
        assert_eq!(
            to_hex(&tag),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn the_key_derivation_matches_the_rfc_5869_vector() {
        let salt: Vec<u8> = (0u8..=0x0c).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9).collect();
        let derived = derive(
            &salt,
            &[0x0b; 22],
            &String::from_utf8_lossy(&info),
            42,
        );
        assert_eq!(
            to_hex(&derived),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn different_purposes_derive_different_keys() {
        // A key that both encrypts and authenticates is one key doing two jobs.
        let encrypting = derive(b"salt", b"material", "alterion-open-project/seal/v1/encrypt", 32);
        let signing = derive(b"salt", b"material", "alterion-open-project/seal/v1/sign", 32);
        assert_ne!(encrypting, signing);
    }

    #[test]
    fn hex_survives_a_round_trip_and_refuses_nonsense() {
        assert_eq!(from_hex(&to_hex(&[0u8, 15, 16, 255])), Some(vec![0, 15, 16, 255]));
        assert_eq!(from_hex("abc"), None, "an odd number of digits");
        assert_eq!(from_hex("zz"), None, "not hex at all");
    }

    // -------------------------------------------------------- the sealing

    #[test]
    fn a_sealed_blob_opens_back_into_what_went_in() {
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let material = machine().key_material();
        let blob = seal(&material, &salt, b"the tokens").expect("seal");
        assert_eq!(unseal(&material, &salt, &blob).as_deref(), Some(&b"the tokens"[..]));
    }

    #[test]
    fn a_sealed_blob_does_not_contain_what_went_in() {
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let blob = seal(&machine().key_material(), &salt, b"a-refresh-token").expect("seal");
        assert!(!blob.contains(&to_hex(b"a-refresh-token")), "got {blob}");
        assert!(!blob.contains("refresh"), "got {blob}");
    }

    #[test]
    fn sealing_the_same_thing_twice_does_not_produce_the_same_blob() {
        // A fresh nonce each time, so two saves of an unchanged session do not
        // announce that it was unchanged.
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let material = machine().key_material();
        let first = seal(&material, &salt, b"the tokens").expect("seal");
        let second = seal(&material, &salt, b"the tokens").expect("seal");
        assert_ne!(first, second);
    }

    #[test]
    fn only_the_exact_tier_decides_whether_a_blob_opens() {
        // A renamed laptop, or an identifier that became readable, must not
        // lock a person out of their own tokens.
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let blob = seal(&machine().key_material(), &salt, b"the tokens").expect("seal");

        let mut drifted = machine();
        drifted.screen = "renamed-laptop".into();
        drifted.pixel_ratio = "a value that became readable".into();
        assert_eq!(
            unseal(&drifted.key_material(), &salt, &blob).as_deref(),
            Some(&b"the tokens"[..]),
            "the drifting tier must not be part of the key"
        );
    }

    #[test]
    fn another_machine_cannot_open_it() {
        // The whole property: copy the file elsewhere and it is a paragraph of
        // hex. The key never existed on disk to be copied with it.
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let blob = seal(&machine().key_material(), &salt, b"the tokens").expect("seal");

        let mut elsewhere = machine();
        elsewhere.anchor = "a different install".into();
        assert_eq!(unseal(&elsewhere.key_material(), &salt, &blob), None);

        let mut new_board = machine();
        new_board.webgl = "a different board".into();
        assert_eq!(unseal(&new_board.key_material(), &salt, &blob), None);
    }

    #[test]
    fn the_salt_is_what_stops_identical_machines_sharing_a_key() {
        let material = machine().key_material();
        let blob = seal(&material, b"one install's salt", b"the tokens").expect("seal");
        assert_eq!(unseal(&material, b"another install's salt", &blob), None);
    }

    #[test]
    fn a_tampered_blob_is_refused_rather_than_decrypted() {
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let material = machine().key_material();
        let blob = seal(&material, &salt, b"the tokens").expect("seal");

        for position in [0, 5, 30, 90] {
            let mut broken: Vec<char> = blob.chars().collect();
            if position >= broken.len() {
                continue;
            }
            broken[position] = if broken[position] == 'a' { 'b' } else { 'a' };
            let broken: String = broken.into_iter().collect();
            assert_eq!(
                unseal(&material, &salt, &broken),
                None,
                "a change at {position} went unnoticed"
            );
        }
    }

    #[test]
    fn a_truncated_or_hand_edited_blob_is_refused() {
        let material = machine().key_material();
        for blob in ["", "00", "not hex at all", &"ff".repeat(20)] {
            assert_eq!(unseal(&material, b"salt", blob), None, "blob was {blob:?}");
        }
    }

    #[test]
    fn a_blob_from_a_future_version_is_not_guessed_at() {
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let material = machine().key_material();
        let blob = seal(&material, &salt, b"the tokens").expect("seal");
        let future = format!("02{}", &blob[2..]);
        assert_eq!(unseal(&material, &salt, &future), None);
    }

    // ------------------------------------------------------- sealed store

    #[test]
    fn a_session_survives_being_written_and_read_back() {
        let path = scratch("round-trip");
        let store = SealedStore::at(path.clone());
        let _ = store.clear();

        assert!(store.load().is_none(), "nothing there to begin with");
        store.save(&sample()).expect("save");

        let back = store.load().expect("a session");
        assert_eq!(back.refresh_token, "a-refresh-token");
        assert_eq!(back.subject, sample().subject);
        assert_eq!(back.expires_at, sample().expires_at);

        store.clear().expect("clear");
        assert!(store.load().is_none(), "signing out forgets it");
    }

    #[test]
    fn the_file_on_disk_holds_no_token_in_the_clear() {
        // The point of the whole module. A backup, a screenshot or a support
        // bundle carrying this file must not be carrying the session.
        let path = scratch("no-plaintext");
        let store = SealedStore::at(path.clone());
        let _ = store.clear();
        store.save(&sample()).expect("save");

        let text = std::fs::read_to_string(&path).expect("the file");
        assert!(!text.contains("a-refresh-token"), "got {text}");
        assert!(!text.contains("an-access-token"), "got {text}");
        assert!(!text.contains("ada@example.org"), "got {text}");
        assert!(text.contains("enc_session"), "the house naming");
        assert!(text.contains("salt"), "kept in plain sight beside it");
        let _ = store.clear();
    }

    #[test]
    fn a_file_from_another_machine_reads_as_nobody_being_signed_in() {
        // Not an error to show. A person whose motherboard was replaced signs
        // in again; they do not get a stack trace about a MAC tag.
        let path = scratch("foreign");
        let salt = random_bytes(SALT_BYTES).expect("the system generator");
        let mut elsewhere = machine();
        elsewhere.anchor = "somebody else's machine".into();
        let blob = seal(
            &elsewhere.key_material(),
            &salt,
            &serde_json::to_vec(&sample()).expect("serialise"),
        )
        .expect("seal");

        let store = SealedStore::at(path.clone());
        store.write(&salt, &blob).expect("write");
        assert!(store.load().is_none(), "it is not this machine's session");
        let _ = store.clear();
    }

    #[test]
    fn saving_twice_keeps_the_newer_one_and_the_same_salt() {
        // Which matters more than it looks: the server spends a refresh token
        // on use, so the replacement has to land over the top of the old one.
        let path = scratch("rotate");
        let store = SealedStore::at(path);
        let _ = store.clear();

        store.save(&sample()).expect("save");
        let salt = store.salt().expect("a salt");

        let mut rotated = sample();
        rotated.refresh_token = "the-next-refresh-token".into();
        store.save(&rotated).expect("save again");

        assert_eq!(store.salt().as_deref(), Some(salt.as_slice()), "one salt per install");
        assert_eq!(
            store.load().expect("a session").refresh_token,
            "the-next-refresh-token"
        );
        let _ = store.clear();
    }

    #[test]
    fn clearing_a_file_that_is_not_there_is_not_a_failure() {
        assert!(SealedStore::at(scratch("never-written")).clear().is_ok());
    }

    #[test]
    fn a_store_says_where_it_keeps_things() {
        // Shown to the user, so it has to be a sentence rather than a name.
        for described in [
            MemoryStore::new().describe(),
            SealedStore::at(scratch("describe")).describe(),
        ] {
            assert!(described.ends_with('.'), "got {described:?}");
            assert!(described.len() > 40);
        }
    }

    #[test]
    fn a_memory_store_gives_back_what_was_put_in_it() {
        let store = MemoryStore::new();
        assert!(store.load().is_none());
        store.save(&sample()).expect("save");
        assert_eq!(store.load().expect("a session").refresh_token, "a-refresh-token");
        store.clear().expect("clear");
        assert!(store.load().is_none());
    }

    // -------------------------------------------------------- expiry maths

    #[test]
    fn renewal_happens_before_the_token_expires_rather_than_after() {
        // The whole point: a sync in flight must not be the thing that
        // discovers the token has run out.
        let expiry = at(3600);
        assert!(
            needs_refresh(at(3600 - EARLY_REFRESH_SECONDS), expiry),
            "renew as the margin opens"
        );
        assert!(
            needs_refresh(at(3600 - EARLY_REFRESH_SECONDS + 1), expiry),
            "and anywhere inside it"
        );
        assert!(
            !needs_refresh(at(3600 - EARLY_REFRESH_SECONDS - 1), expiry),
            "but not before there is any reason to"
        );
    }

    #[test]
    fn a_token_that_has_already_expired_certainly_needs_renewing() {
        assert!(needs_refresh(at(3601), at(3600)));
        assert!(needs_refresh(at(9_999_999), at(3600)));
    }

    #[test]
    fn the_margin_is_a_margin_and_not_the_whole_lifetime() {
        // If it were, every single call would renew, and the server treats a
        // refresh token as single use.
        let issued = at(0);
        assert!(!needs_refresh(issued, expires_at(issued, 3600)));
        assert!(EARLY_REFRESH_SECONDS < 3600);
    }

    #[test]
    fn an_hour_from_now_is_an_hour_from_now() {
        assert_eq!(expires_at(at(1000), 3600), at(4600));
    }

    #[test]
    fn a_server_that_gives_no_lifetime_is_treated_as_already_expired() {
        // Better to renew once for nothing than to send a dead token and have
        // the user see a failure they cannot explain.
        let now = at(1000);
        assert_eq!(expires_at(now, 0), now);
        assert!(needs_refresh(now, expires_at(now, 0)));
        assert!(needs_refresh(now, expires_at(now, -50)), "or a nonsense one");
    }

    // -------------------------------------------------------------- record

    #[test]
    fn printing_a_session_does_not_print_its_tokens() {
        // A debug print is how a secret reaches a log without anyone meaning it
        // to, so the tokens are not in the printed form at all.
        let printed = format!("{:?}", sample());
        assert!(!printed.contains("an-access-token"), "got {printed}");
        assert!(!printed.contains("a-refresh-token"), "got {printed}");
        assert!(printed.contains("ada@example.org"), "the rest is still useful");
    }

    #[test]
    fn a_record_from_an_older_build_without_a_name_still_reads() {
        let text = r#"{"issuer":"https://auth.coraldune.cloud","client_id":"app",
            "access_token":"a","expires_at":1,"subject":"s"}"#;
        let back: Stored = serde_json::from_str(text).expect("deserialise");
        assert!(back.refresh_token.is_empty());
        assert!(back.name.is_empty());
    }

    #[test]
    fn randomness_comes_from_the_system_and_is_not_the_same_twice() {
        let first = random_bytes(32).expect("the system generator");
        let second = random_bytes(32).expect("the system generator");
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }
}
