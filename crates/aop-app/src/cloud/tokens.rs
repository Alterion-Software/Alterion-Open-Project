//! Where the tokens live between one run of the application and the next.
//!
//! A refresh token is a standing permission to act as the person who signed in.
//! It is not a preference, so it never goes near `config.cfg`: that file is
//! plain text, it is meant to be edited by hand, and it travels in backups, in
//! screenshots and in support bundles. `config.cfg` keeps the settings that say
//! which server to talk to. It keeps nothing that would let anybody talk to it.
//!
//! Each platform has somewhere of its own for this, so each platform's own
//! thing is used:
//!
//! ```text
//!   Linux    a file of its own beside the settings, 0600 in a 0700 directory
//!   macOS    the login Keychain, through the `security` tool
//!   Windows  a value under HKCU, wrapped in DPAPI at user scope
//! ```
//!
//! On top of that, and before anything reaches a store, the session is **sealed
//! to the machine**. The key is not kept anywhere. It is derived from the
//! hardware identity in [`crate::cloud::device`] and exists only for the length
//! of the calculation that produces it.
//!
//! ```text
//!   device exact tier  ──┐
//!   (anchor, board,      ├── HKDF-SHA256 ──┬── encryption key ──┐
//!    platform, cpu)      │                 └── signing key ───┐ │
//!   per install salt ────┘                                    v v
//!   (public, stored beside the blob)                      sealed bytes
//!                                                               │
//!                                          file / Keychain / registry
//! ```
//!
//! That matters most on Linux, where a file in a home directory has no
//! protection beyond its mode: a copied file is useless on another machine
//! because the key never existed on disk to be copied with it. It is worth
//! having on the other two as well, for a smaller reason: it means the only
//! thing ever handed to an external tool is ciphertext.
//!
//! The salt is public on purpose. It is not what protects the blob; it is what
//! stops two machines with identical hardware deriving the same key.
//!
//! A blob that will not open is not an error to show anybody. It means the
//! machine changed underneath it, and the honest reading of that is that nobody
//! is signed in. The user signs in again; they do not get a dialog about a MAC
//! tag.
//!
//! ## What the sealing is, exactly
//!
//! HKDF-SHA256 for the keys, SHA-256 in counter mode for the keystream, and
//! HMAC-SHA256 over the ciphertext: encrypt, then authenticate, with
//! independent keys. Built from the one hash this crate already depends on. The
//! moment a dependency can be added it should become ChaCha20-Poly1305 or
//! whatever `alterion-encrypt` offers; [`seal`] and [`unseal`] are the only two
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

/// What the session is filed under in the Keychain.
///
/// Named for the product rather than the crate, because this is what a person
/// sees in Keychain Access when they go looking. macOS only, because it is the
/// only store that files things under a name: the others are a path and a
/// registry value.
#[cfg(target_os = "macos")]
pub const SERVICE_NAME: &str = "Alterion Open Project";

/// The one entry within it: one signed in account, which is what the
/// application supports.
#[cfg(target_os = "macos")]
pub const ACCOUNT_NAME: &str = "cloud-session";

/// Everything needed to pick a session back up, and nothing else.
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
    /// The account picture, when the provider serves one. Defaulted so a
    /// record written by a build that knew nothing about it still reads.
    #[serde(default)]
    pub picture: Option<String>,
}

/// Written out by hand so a token cannot reach a log through a stray `{:?}`.
/// Everything here except the tokens is safe to print, and the tokens are the
/// entire reason the record is sealed.
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

/// Why a session could not be kept.
///
/// There is no variant for "it would not open". That is not a failure, it is
/// nobody being signed in, and it comes back as `Ok(None)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// This machine offers nowhere to put it.
    NoPlace(String),
    /// There is somewhere, and writing to it did not work.
    NotWritten(String),
    /// Signing out locally did not fully take.
    NotRemoved(String),
    /// The machine could not be identified, so nothing can be sealed to it.
    NoDeviceIdentity(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::NoPlace(why) => write!(
                f,
                "There is nowhere on this machine to keep the sign in: {why}. \
                 You will be asked to sign in again each time the application starts."
            ),
            StoreError::NotWritten(why) => write!(
                f,
                "The sign in could not be saved: {why}. \
                 You will be asked to sign in again next time."
            ),
            StoreError::NotRemoved(why) => write!(
                f,
                "Signed out, but the saved sign in could not be removed: {why}. \
                 Remove it by hand if this machine is shared."
            ),
            StoreError::NoDeviceIdentity(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for StoreError {}

// ------------------------------------------------------------- randomness

/// Random bytes from the operating system.
///
/// There is no fallback to a weaker source anywhere in this module. A salt or a
/// nonce that can be guessed is not a salt or a nonce.
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
/// encrypts and authenticates is one key doing two jobs it was not asked to do
/// at once.
fn derive(salt: &[u8], material: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let prk = hmac_sha256(salt, material);

    let mut out = Vec::with_capacity(length);
    let mut previous: Vec<u8> = Vec::new();
    let mut counter: u8 = 1;
    while out.len() < length {
        let mut block = Vec::with_capacity(previous.len() + info.len() + 1);
        block.extend_from_slice(&previous);
        block.extend_from_slice(info);
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

/// The two purposes the device identity is expanded into. Separate strings, so
/// the encryption key and the signing key are independent of each other.
const ENCRYPT_INFO: &[u8] = b"alterion-open-project/seal/v1/encrypt";
const SIGN_INFO: &[u8] = b"alterion-open-project/seal/v1/sign";

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

/// Seal a plaintext to a machine.
///
/// The salt travels inside the blob rather than beside it, so a store only ever
/// has one opaque run of bytes to keep and there is nothing to get out of step.
pub fn seal(material: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    let salt = random_bytes(SALT_BYTES)?;
    let nonce = random_bytes(NONCE_BYTES)?;

    let encryption_key = derive(&salt, material, ENCRYPT_INFO, 32);
    let signing_key = derive(&salt, material, SIGN_INFO, 32);

    let stream = keystream(&encryption_key, &nonce, plaintext.len());
    let ciphertext: Vec<u8> = plaintext
        .iter()
        .zip(stream.iter())
        .map(|(byte, mask)| byte ^ mask)
        .collect();

    // Encrypt then authenticate, over the version, the salt and the nonce as
    // well as the ciphertext, so none of them can be swapped for another
    // blob's.
    let mut header = Vec::with_capacity(1 + SALT_BYTES + NONCE_BYTES);
    header.push(SEAL_VERSION);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&nonce);

    let mut signed = header.clone();
    signed.extend_from_slice(&ciphertext);
    let tag = hmac_sha256(&signing_key, &signed);

    let mut blob = header;
    blob.extend_from_slice(&tag);
    blob.extend_from_slice(&ciphertext);
    Some(blob)
}

/// Open a sealed blob, or decide it does not belong to this machine.
///
/// `None` covers every way this can go: a different machine, a truncated file,
/// a hand edit, a blob from a future version. They all mean the same thing to
/// the caller, which is that there is no session here.
pub fn unseal(material: &[u8], blob: &[u8]) -> Option<Vec<u8>> {
    const HEADER: usize = 1 + SALT_BYTES + NONCE_BYTES;
    if blob.len() < HEADER + TAG_BYTES || blob[0] != SEAL_VERSION {
        return None;
    }

    let salt = &blob[1..1 + SALT_BYTES];
    let nonce = &blob[1 + SALT_BYTES..HEADER];
    let tag = &blob[HEADER..HEADER + TAG_BYTES];
    let ciphertext = &blob[HEADER + TAG_BYTES..];

    let signing_key = derive(salt, material, SIGN_INFO, 32);
    let mut signed = Vec::with_capacity(HEADER + ciphertext.len());
    signed.extend_from_slice(&blob[..HEADER]);
    signed.extend_from_slice(ciphertext);

    // Checked before anything is decrypted: a blob that was not written by this
    // machine is not something to start unpicking.
    if !same_bytes(&hmac_sha256(&signing_key, &signed), tag) {
        return None;
    }

    let encryption_key = derive(salt, material, ENCRYPT_INFO, 32);
    let stream = keystream(&encryption_key, nonce, ciphertext.len());
    Some(
        ciphertext
            .iter()
            .zip(stream.iter())
            .map(|(byte, mask)| byte ^ mask)
            .collect(),
    )
}

/// Bytes as lower case hex, which is the shape the sign in server wants a
/// fingerprint in and the shape the Keychain and the registry can hold without
/// worrying about a null byte cutting a value short.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The other direction. Only the stores that keep hex need it.
#[cfg(any(target_os = "macos", windows, test))]
fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

// ------------------------------------------------------------- the stores

/// Somewhere to keep an opaque run of bytes.
///
/// Bytes, not a session: what is stored is already sealed, and a store's only
/// business is putting it somewhere and getting it back. That is what lets the
/// three platform implementations be as different from each other as they need
/// to be.
pub trait TokenStore: Send + Sync {
    fn save(&self, blob: &[u8]) -> Result<(), StoreError>;

    /// The bytes from last time. `Ok(None)` when there are none, which includes
    /// the ordinary case of nobody having signed in yet.
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError>;

    fn clear(&self) -> Result<(), StoreError>;

    /// Where this store puts things, for saying so in the interface. A person
    /// deciding whether to stay signed in on a shared machine deserves to know.
    fn describe(&self) -> &'static str;
}

/// The directory the application keeps its own things in.
fn config_directory() -> Option<PathBuf> {
    crate::settings::config_root()
}

/// Where the session file lives on the platforms that use one.
pub fn session_path() -> Option<PathBuf> {
    config_directory().map(|directory| directory.join("cloud.session"))
}

// -------------------------------------------------------------- Linux file

/// A file of its own, beside the settings but not in them.
///
/// Its own file rather than a section of `config.cfg` for two reasons. The
/// obvious one is that `config.cfg` is meant to be read and edited by a person.
/// The mundane one is that [`crate::settings`] rewrites that file from the
/// settings it knows about, so a key it has never heard of would be dropped the
/// next time any preference changed, taking the sign in with it.
#[cfg(unix)]
pub struct FileStore {
    path: PathBuf,
}

#[cfg(unix)]
impl FileStore {
    pub fn at(path: PathBuf) -> FileStore {
        FileStore { path }
    }
}

#[cfg(unix)]
impl TokenStore for FileStore {
    fn save(&self, blob: &[u8]) -> Result<(), StoreError> {
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

        let Some(parent) = self.path.parent() else {
            return Err(StoreError::NoPlace("it has no directory".into()));
        };
        // 0700 rather than the default: nobody else on the machine has any
        // business listing what is in here, let alone reading it.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|error| StoreError::NotWritten(format!("{} ({error})", parent.display())))?;

        // The mode goes on at creation, not afterwards, so there is no instant
        // where the file exists and is readable by anyone else.
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.path)
            .map_err(|error| {
                StoreError::NotWritten(format!("{} ({error})", self.path.display()))
            })?;
        file.write_all(blob).map_err(|error| {
            StoreError::NotWritten(format!("{} ({error})", self.path.display()))
        })?;

        // A file left over from an older build could have a wider mode already,
        // and truncating it does not narrow it.
        let _ = std::fs::set_permissions(
            &self.path,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        );
        Ok(())
    }

    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        match std::fs::read(&self.path) {
            Ok(blob) if blob.is_empty() => Ok(None),
            Ok(blob) => Ok(Some(blob)),
            // Not there, or not readable: either way nobody is signed in.
            Err(_) => Ok(None),
        }
    }

    fn clear(&self) -> Result<(), StoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::NotRemoved(format!(
                "{} ({error})",
                self.path.display()
            ))),
        }
    }

    fn describe(&self) -> &'static str {
        "In a file only your account can read, sealed to this machine. \
         Copying it to another machine achieves nothing: the key is worked out \
         from the hardware and is never written down."
    }
}

// ----------------------------------------------------------- macOS Keychain

/// The login Keychain, through the `security` tool that ships with macOS.
///
/// Through the command line tool rather than the Security framework because
/// that needs no dependency, and what is handed to it is already sealed, so the
/// value that appears briefly in an argument list is ciphertext rather than a
/// token. A native binding would still be better and is the first thing to
/// change here once a dependency can be added.
#[cfg(target_os = "macos")]
pub struct KeychainStore;

#[cfg(target_os = "macos")]
impl KeychainStore {
    fn run(arguments: &[&str]) -> Result<Option<String>, String> {
        use crate::quiet::Quiet;
        let output = std::process::Command::new("security").quiet()
            .args(arguments)
            .output()
            .map_err(|error| format!("the security tool could not be run ({error})"))?;
        if output.status.success() {
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ));
        }
        // The tool answers "item not found" with a non-zero status, which is
        // the ordinary case of nothing having been stored yet.
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
impl TokenStore for KeychainStore {
    fn save(&self, blob: &[u8]) -> Result<(), StoreError> {
        let hex = to_hex(blob);
        // -U updates the entry in place rather than refusing because one is
        // already there.
        let outcome = KeychainStore::run(&[
            "add-generic-password",
            "-a",
            ACCOUNT_NAME,
            "-s",
            SERVICE_NAME,
            "-U",
            "-w",
            &hex,
        ])
        .map_err(StoreError::NotWritten)?;
        match outcome {
            Some(_) => Ok(()),
            None => Err(StoreError::NotWritten(
                "the Keychain would not accept it".into(),
            )),
        }
    }

    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let found = KeychainStore::run(&[
            "find-generic-password",
            "-a",
            ACCOUNT_NAME,
            "-s",
            SERVICE_NAME,
            "-w",
        ])
        .map_err(StoreError::NoPlace)?;
        Ok(found.as_deref().and_then(from_hex))
    }

    fn clear(&self) -> Result<(), StoreError> {
        KeychainStore::run(&[
            "delete-generic-password",
            "-a",
            ACCOUNT_NAME,
            "-s",
            SERVICE_NAME,
        ])
        .map_err(StoreError::NotRemoved)?;
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "In your login Keychain, sealed to this machine. \
         Keychain Access lists it under Alterion Open Project."
    }
}

// -------------------------------------------------------- Windows registry

/// A value under `HKCU`, wrapped in DPAPI at user scope.
///
/// The registry on its own is only as private as the user account, which is why
/// the bytes go through DPAPI first: another account on the same machine, and
/// anything reading the hive offline, gets ciphertext.
#[cfg(windows)]
pub struct RegistryStore;

#[cfg(windows)]
mod dpapi {
    //! The two DPAPI calls, declared rather than pulled in.

    use std::ffi::c_void;

    #[repr(C)]
    pub struct DataBlob {
        pub length: u32,
        pub data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        pub fn CryptProtectData(
            input: *const DataBlob,
            description: *const u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;

        pub fn CryptUnprotectData(
            input: *const DataBlob,
            description: *mut *mut u16,
            entropy: *const DataBlob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn LocalFree(handle: *mut c_void) -> *mut c_void;
    }

    /// Wrap bytes so only this user account on this machine can unwrap them.
    pub fn protect(plaintext: &[u8]) -> Option<Vec<u8>> {
        let mut input = DataBlob {
            length: u32::try_from(plaintext.len()).ok()?,
            data: plaintext.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            length: 0,
            data: std::ptr::null_mut(),
        };

        // Safe: both blobs describe memory that outlives the call, and the
        // buffer the call allocates is copied out and freed before returning.
        let ok = unsafe {
            CryptProtectData(
                &mut input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut output,
            )
        };
        if ok == 0 || output.data.is_null() {
            return None;
        }

        let bytes =
            unsafe { std::slice::from_raw_parts(output.data, output.length as usize) }.to_vec();
        unsafe { LocalFree(output.data as *mut c_void) };
        Some(bytes)
    }

    /// Unwrap them again, or decide they were not this account's.
    pub fn unprotect(ciphertext: &[u8]) -> Option<Vec<u8>> {
        let mut input = DataBlob {
            length: u32::try_from(ciphertext.len()).ok()?,
            data: ciphertext.as_ptr() as *mut u8,
        };
        let mut output = DataBlob {
            length: 0,
            data: std::ptr::null_mut(),
        };

        // Safe: as above.
        let ok = unsafe {
            CryptUnprotectData(
                &mut input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut output,
            )
        };
        if ok == 0 || output.data.is_null() {
            return None;
        }

        let bytes =
            unsafe { std::slice::from_raw_parts(output.data, output.length as usize) }.to_vec();
        unsafe { LocalFree(output.data as *mut c_void) };
        Some(bytes)
    }
}

#[cfg(windows)]
impl RegistryStore {
    const KEY: &'static str = r"HKCU\Software\Alterion\Open Project";
    const VALUE: &'static str = "CloudSession";

    fn run(arguments: &[&str]) -> Option<String> {
        use crate::quiet::Quiet;
        let output = std::process::Command::new("reg.exe").quiet()
            .args(arguments)
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(windows)]
impl TokenStore for RegistryStore {
    fn save(&self, blob: &[u8]) -> Result<(), StoreError> {
        let wrapped = dpapi::protect(blob).ok_or_else(|| {
            StoreError::NotWritten("Windows would not protect the value".into())
        })?;
        // Hex in a string value rather than raw binary, so what goes across the
        // command line is printable and cannot be cut short by a null byte.
        let hex = to_hex(&wrapped);
        RegistryStore::run(&[
            "add",
            RegistryStore::KEY,
            "/v",
            RegistryStore::VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &hex,
            "/f",
        ])
        .map(|_| ())
        .ok_or_else(|| StoreError::NotWritten("the registry would not accept it".into()))
    }

    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let Some(text) = RegistryStore::run(&[
            "query",
            RegistryStore::KEY,
            "/v",
            RegistryStore::VALUE,
        ]) else {
            return Ok(None);
        };
        let Some(hex) = text
            .lines()
            .find(|line| line.contains(RegistryStore::VALUE))
            .and_then(|line| line.split_whitespace().last())
        else {
            return Ok(None);
        };
        // Anything unreadable here is nobody being signed in, not a failure.
        Ok(from_hex(hex).and_then(|wrapped| dpapi::unprotect(&wrapped)))
    }

    fn clear(&self) -> Result<(), StoreError> {
        RegistryStore::run(&[
            "delete",
            RegistryStore::KEY,
            "/v",
            RegistryStore::VALUE,
            "/f",
        ]);
        // Nothing there to delete is the outcome that was wanted, and reg.exe
        // reports it the same way as a real failure, so it is not treated as
        // one.
        Ok(())
    }

    fn describe(&self) -> &'static str {
        "In the registry under your own user account, protected by Windows so \
         no other account can read it, and sealed to this machine."
    }
}

// ---------------------------------------------------------------- fallback

/// A store that lasts as long as the process does.
///
/// What the application falls back to when there is nowhere else. Not a
/// placeholder that loses data quietly: signing in again after a restart is a
/// nuisance, whereas a token written somewhere it cannot be protected is a
/// problem that outlives the machine.
#[derive(Default)]
pub struct MemoryStore {
    held: Mutex<Option<Vec<u8>>>,
}

impl MemoryStore {
    pub fn new() -> MemoryStore {
        MemoryStore::default()
    }
}

impl TokenStore for MemoryStore {
    fn save(&self, blob: &[u8]) -> Result<(), StoreError> {
        let mut held = self.held.lock().map_err(|_| {
            StoreError::NotWritten("the application is in an inconsistent state".into())
        })?;
        *held = Some(blob.to_vec());
        Ok(())
    }

    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.held.lock().ok().and_then(|held| held.clone()))
    }

    fn clear(&self) -> Result<(), StoreError> {
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

// ------------------------------------------------------- choosing the store

/// The store this platform uses.
#[cfg(target_os = "linux")]
fn platform_store() -> Box<dyn TokenStore> {
    match session_path() {
        Some(path) => Box::new(FileStore::at(path)),
        None => Box::new(MemoryStore::new()),
    }
}

#[cfg(target_os = "macos")]
fn platform_store() -> Box<dyn TokenStore> {
    Box::new(KeychainStore)
}

#[cfg(windows)]
fn platform_store() -> Box<dyn TokenStore> {
    Box::new(RegistryStore)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn platform_store() -> Box<dyn TokenStore> {
    // The other unixes have the same home directory arrangement as Linux, so
    // the same file store is right for them.
    match session_path() {
        Some(path) => Box::new(FileStore::at(path)),
        None => Box::new(MemoryStore::new()),
    }
}

#[cfg(not(any(unix, windows)))]
fn platform_store() -> Box<dyn TokenStore> {
    Box::new(MemoryStore::new())
}

/// The store the application is using.
static STORE: OnceLock<Box<dyn TokenStore>> = OnceLock::new();

/// The store this platform uses, made once and kept.
pub fn store() -> &'static dyn TokenStore {
    STORE.get_or_init(platform_store).as_ref()
}

// ------------------------------------------------------ sessions in and out

/// The material a session is sealed with: this machine's exact tier.
fn key_material() -> Result<Vec<u8>, StoreError> {
    device::components()
        .map(device::DeviceComponents::key_material)
        .map_err(|absent| StoreError::NoDeviceIdentity(absent.to_string()))
}

/// Keep a session, sealing it to this machine on the way in.
pub fn save_into(store: &dyn TokenStore, session: &Stored) -> Result<(), StoreError> {
    let material = key_material()?;
    let plaintext = serde_json::to_vec(session)
        .map_err(|_| StoreError::NotWritten("it could not be prepared".into()))?;
    let sealed = seal(&material, &plaintext).ok_or_else(|| {
        StoreError::NotWritten("the system random number generator could not be read".into())
    })?;
    store.save(&sealed)
}

/// The session from a store, if this machine can still open it.
///
/// Every way this can come to nothing means the same thing: nobody is signed
/// in. A machine whose hardware changed, a store that has nothing in it, a blob
/// somebody edited. None of them is worth a dialog.
pub fn load_from(store: &dyn TokenStore) -> Option<Stored> {
    let material = key_material().ok()?;
    let sealed = store.load().ok().flatten()?;
    let plaintext = unseal(&material, &sealed)?;
    serde_json::from_slice(&plaintext).ok()
}

/// Keep a session, in the store this platform uses.
pub fn save_session(session: &Stored) -> Result<(), StoreError> {
    save_into(store(), session)
}

/// The session from last time.
pub fn load_session() -> Option<Stored> {
    load_from(store())
}

/// Forget it.
pub fn clear_session() -> Result<(), StoreError> {
    store().clear()
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
            issuer: "https://auth.example.org".into(),
            client_id: "alterion-open-project".into(),
            access_token: "an-access-token".into(),
            refresh_token: "a-refresh-token".into(),
            expires_at: 1_800_000_000,
            subject: "0198f0c2-0000-7000-8000-000000000000".into(),
            name: "Ada Lovelace".into(),
            email: "ada@example.org".into(),
            picture: None,
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

    #[cfg(unix)]
    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("aop-cloud-{}", std::process::id()))
            .join(name)
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
        // Test case 1: the salt, the input and the info are all fixed by the
        // specification, so this checks the derivation against something other
        // than itself.
        let salt: Vec<u8> = (0u8..=0x0c).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9).collect();
        let derived = derive(&salt, &[0x0b; 22], &info, 42);
        assert_eq!(
            to_hex(&derived),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn different_purposes_derive_different_keys() {
        // A key that both encrypts and authenticates is one key doing two jobs.
        let encrypting = derive(b"salt", b"material", ENCRYPT_INFO, 32);
        let signing = derive(b"salt", b"material", SIGN_INFO, 32);
        assert_ne!(encrypting, signing);
    }

    #[test]
    fn hex_survives_a_round_trip_and_refuses_nonsense() {
        assert_eq!(from_hex(&to_hex(&[0u8, 15, 16, 255])), Some(vec![0, 15, 16, 255]));
        assert_eq!(from_hex("abc"), None, "an odd number of digits");
        assert_eq!(from_hex("zz"), None, "not hex at all");
        assert_eq!(from_hex(""), None, "nothing at all");
    }

    // --------------------------------------------------------- the sealing

    #[test]
    fn a_sealed_blob_opens_back_into_what_went_in() {
        let material = machine().key_material();
        let blob = seal(&material, b"the tokens").expect("seal");
        assert_eq!(unseal(&material, &blob).as_deref(), Some(&b"the tokens"[..]));
    }

    #[test]
    fn a_sealed_blob_does_not_contain_what_went_in() {
        let blob = seal(&machine().key_material(), b"a-refresh-token").expect("seal");
        assert!(
            !blob.windows(15).any(|window| window == b"a-refresh-token"),
            "the plaintext is still in there"
        );
    }

    #[test]
    fn sealing_the_same_thing_twice_does_not_produce_the_same_blob() {
        // A fresh salt and nonce each time, so two saves of an unchanged
        // session do not announce that it was unchanged.
        let material = machine().key_material();
        assert_ne!(
            seal(&material, b"the tokens").expect("seal"),
            seal(&material, b"the tokens").expect("seal")
        );
    }

    #[test]
    fn only_the_exact_tier_decides_whether_a_blob_opens() {
        // A renamed laptop, or an identifier that became readable, must not
        // lock a person out of their own tokens.
        let blob = seal(&machine().key_material(), b"the tokens").expect("seal");

        let mut drifted = machine();
        drifted.screen = "renamed-laptop".into();
        drifted.pixel_ratio = "a value that became readable".into();
        assert_eq!(
            unseal(&drifted.key_material(), &blob).as_deref(),
            Some(&b"the tokens"[..]),
            "the drifting tier must not be part of the key"
        );
    }

    #[test]
    fn a_blob_sealed_under_one_anchor_does_not_open_under_another() {
        // The whole property: copy the file elsewhere and it is a run of bytes.
        // The key never existed on disk to be copied with it.
        let blob = seal(&machine().key_material(), b"the tokens").expect("seal");

        let mut elsewhere = machine();
        elsewhere.anchor = "a different install".into();
        assert_eq!(unseal(&elsewhere.key_material(), &blob), None);

        let mut new_board = machine();
        new_board.webgl = "a different board".into();
        assert_eq!(unseal(&new_board.key_material(), &blob), None);
    }

    #[test]
    fn a_tampered_blob_is_refused_rather_than_decrypted() {
        let material = machine().key_material();
        let blob = seal(&material, b"the tokens").expect("seal");

        for position in [0, 5, 20, 40, 60] {
            if position >= blob.len() {
                continue;
            }
            let mut broken = blob.clone();
            broken[position] ^= 0x01;
            assert_eq!(
                unseal(&material, &broken),
                None,
                "a change at {position} went unnoticed"
            );
        }
    }

    #[test]
    fn a_truncated_or_hand_edited_blob_is_refused() {
        let material = machine().key_material();
        for blob in [vec![], vec![1u8], vec![0xffu8; 40]] {
            assert_eq!(unseal(&material, &blob), None, "blob was {} bytes", blob.len());
        }
    }

    #[test]
    fn a_blob_from_a_future_version_is_not_guessed_at() {
        let material = machine().key_material();
        let mut future = seal(&material, b"the tokens").expect("seal");
        future[0] = SEAL_VERSION + 1;
        assert_eq!(unseal(&material, &future), None);
    }

    // ------------------------------------------------------- the Linux file

    #[cfg(unix)]
    #[test]
    fn the_file_store_round_trips() {
        let store = FileStore::at(scratch("round-trip"));
        let _ = store.clear();

        assert_eq!(store.load(), Ok(None), "nothing there to begin with");
        store.save(b"sealed bytes").expect("save");
        assert_eq!(store.load(), Ok(Some(b"sealed bytes".to_vec())));

        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None), "signing out forgets it");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;

        let path = scratch("modes");
        let store = FileStore::at(path.clone());
        let _ = store.clear();
        store.save(b"sealed bytes").expect("save");

        let mode = std::fs::metadata(&path).expect("the file").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);

        let directory = std::fs::metadata(path.parent().expect("a parent"))
            .expect("the directory")
            .permissions()
            .mode();
        assert_eq!(directory & 0o777, 0o700, "got {:o}", directory & 0o777);
        let _ = store.clear();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_was_already_there_has_its_mode_narrowed() {
        // A file left by an older build could be world readable, and truncating
        // it would not have changed that.
        use std::os::unix::fs::PermissionsExt;

        let path = scratch("widened");
        let store = FileStore::at(path.clone());
        let _ = store.clear();
        store.save(b"first").expect("save");
        std::fs::set_permissions(&path, PermissionsExt::from_mode(0o644)).expect("widen");

        store.save(b"second").expect("save again");
        let mode = std::fs::metadata(&path).expect("the file").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "got {:o}", mode & 0o777);
        let _ = store.clear();
    }

    #[cfg(unix)]
    #[test]
    fn clearing_a_store_with_nothing_in_it_is_not_a_failure() {
        let store = FileStore::at(scratch("never-written"));
        assert_eq!(store.clear(), Ok(()));
        assert_eq!(store.load(), Ok(None), "nothing, rather than an error");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_keychain_store_round_trips() {
        let store = KeychainStore;
        let _ = store.clear();
        assert_eq!(store.load(), Ok(None));
        store.save(b"sealed bytes").expect("save");
        assert_eq!(store.load(), Ok(Some(b"sealed bytes".to_vec())));
        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None));
    }

    #[cfg(windows)]
    #[test]
    fn the_registry_store_round_trips() {
        let store = RegistryStore;
        let _ = store.clear();
        assert_eq!(store.load(), Ok(None));
        store.save(b"sealed bytes").expect("save");
        assert_eq!(store.load(), Ok(Some(b"sealed bytes".to_vec())));
        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None));
    }

    #[cfg(windows)]
    #[test]
    fn windows_protects_the_value_before_it_reaches_the_registry() {
        let wrapped = dpapi::protect(b"sealed bytes").expect("protect");
        assert_ne!(wrapped, b"sealed bytes".to_vec());
        assert_eq!(dpapi::unprotect(&wrapped).as_deref(), Some(&b"sealed bytes"[..]));
    }

    #[test]
    fn a_memory_store_gives_back_what_was_put_in_it() {
        let store = MemoryStore::new();
        assert_eq!(store.load(), Ok(None));
        store.save(b"sealed bytes").expect("save");
        assert_eq!(store.load(), Ok(Some(b"sealed bytes".to_vec())));
        store.clear().expect("clear");
        assert_eq!(store.load(), Ok(None));
    }

    #[test]
    fn a_store_says_where_it_keeps_things() {
        // Shown to the user, so it has to be a sentence rather than a name.
        let described = store().describe();
        assert!(described.ends_with('.'), "got {described:?}");
        assert!(described.len() > 40);
    }

    // ------------------------------------------------------ whole sessions

    #[test]
    fn a_session_survives_being_written_and_read_back() {
        // Against a store of its own rather than the process wide one: these
        // run in parallel, and two of them sharing the real store would be two
        // of them overwriting each other's session.
        let store = MemoryStore::new();
        assert!(load_from(&store).is_none(), "nothing there to begin with");

        save_into(&store, &sample()).expect("save");
        let back = load_from(&store).expect("a session");
        assert_eq!(back.refresh_token, "a-refresh-token");
        assert_eq!(back.subject, sample().subject);
        assert_eq!(back.expires_at, sample().expires_at);

        store.clear().expect("clear");
        assert!(load_from(&store).is_none(), "signing out forgets it");
    }

    #[test]
    fn what_reaches_the_store_holds_no_token_in_the_clear() {
        // The point of the whole module. A backup or a support bundle carrying
        // this must not be carrying the session.
        let store = MemoryStore::new();
        save_into(&store, &sample()).expect("save");

        let kept = store.load().expect("the store").expect("something in it");
        for secret in ["a-refresh-token", "an-access-token", "ada@example.org"] {
            assert!(
                !kept
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "{secret} is in there in the clear"
            );
        }
    }

    #[test]
    fn saving_twice_keeps_the_newer_one() {
        // Which matters more than it looks: the server spends a refresh token
        // on use, so the replacement has to land over the top of the old one.
        let store = MemoryStore::new();
        save_into(&store, &sample()).expect("save");

        let mut rotated = sample();
        rotated.refresh_token = "the-next-refresh-token".into();
        save_into(&store, &rotated).expect("save again");

        assert_eq!(
            load_from(&store).expect("a session").refresh_token,
            "the-next-refresh-token"
        );
    }

    #[test]
    fn a_session_from_another_machine_reads_as_nobody_being_signed_in() {
        // Not an error to show. A person whose motherboard was replaced signs
        // in again; they do not get a stack trace about a MAC tag.
        let mut elsewhere = machine();
        elsewhere.anchor = "somebody else's machine".into();
        let foreign = seal(
            &elsewhere.key_material(),
            &serde_json::to_vec(&sample()).expect("serialise"),
        )
        .expect("seal");

        let store = MemoryStore::new();
        store.save(&foreign).expect("save");
        assert!(
            load_from(&store).is_none(),
            "it is not this machine's session"
        );
    }

    #[test]
    fn the_whole_round_trip_works_against_this_platform_s_own_store() {
        // The one that touches the real file, Keychain or registry, so the
        // platform specific half is not left to the unit tests above.
        let _ = clear_session();
        save_session(&sample()).expect("save");
        assert_eq!(
            load_session().expect("a session").subject,
            sample().subject
        );
        clear_session().expect("clear");
        assert!(load_session().is_none(), "signing out forgets it");
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
        const { assert!(EARLY_REFRESH_SECONDS < 3600) };
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
        let text = r#"{"issuer":"https://auth.example.org","client_id":"app",
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

    #[test]
    fn every_storage_message_says_what_it_means_for_the_person() {
        for error in [
            StoreError::NoPlace("no home directory".into()),
            StoreError::NotWritten("the disk is full".into()),
            StoreError::NotRemoved("permission denied".into()),
            StoreError::NoDeviceIdentity(
                "This machine could not be identified, so signing in has stopped.".into(),
            ),
        ] {
            let message = error.to_string();
            assert!(message.len() > 40, "too terse to act on: {message}");
        }
    }
}
