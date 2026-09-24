//! Sealing saved credentials with a key kept somewhere other than the file
//! they are saved in.
//!
//! On Windows, Mimic seals each value to the user's account with DPAPI
//! (`apps/desktop/src-tauri/src/secrets.rs`). macOS has no DPAPI; what it has
//! is the login Keychain, which the system unlocks with the user's password.
//! So on macOS one random key is kept there — one Keychain item, "Mimic",
//! made the first time a value is saved — and each value is sealed with it
//! (XChaCha20-Poly1305, a fresh random nonce each time, bound to Mimic's own
//! label). Copied off the computer, the file cannot be opened without the
//! Keychain; a value altered in the file does not open at all.
//!
//! What it does not stop is the same as on Windows: a program running as the
//! user can ask for the Keychain item, as it could for the browser's saved
//! passwords — though macOS may ask the user first.
//!
//! **Not yet run on a Mac.** The sealing below is tested wherever the tests
//! run; the Keychain calls are compiled for macOS (`cargo check --target
//! aarch64-apple-darwin`) and have never been run. `docs/SECURITY_MODEL.md`
//! says so, and so does the app, which reports this protection only on a
//! build that uses it.

use std::io;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

/// What every sealed value is bound to: a value sealed for anything else does
/// not open as one of these.
const LABEL: &[u8] = b"mimic.credentials.v2";

const NONCE: usize = 24;

/// Where the key comes from.
pub trait KeySource: Send + Sync {
    /// The key, made and kept the first time it is asked for.
    fn key(&self) -> io::Result<[u8; 32]>;
}

/// Seals and opens values with the key `K` gives. The key is asked for once
/// and kept for the life of the process — so opening every saved value does
/// not ask the Keychain, or the user, each time — unless asking fails, when
/// it is asked again next time.
pub struct KeyedSealer<K: KeySource> {
    source: K,
    key: std::sync::Mutex<Option<[u8; 32]>>,
}

impl<K: KeySource> KeyedSealer<K> {
    pub fn new(source: K) -> Self {
        Self { source, key: std::sync::Mutex::new(None) }
    }

    fn cipher(&self) -> io::Result<XChaCha20Poly1305> {
        let mut kept = self.key.lock().unwrap_or_else(|p| p.into_inner());
        let key = match *kept {
            Some(key) => key,
            None => {
                let key = self.source.key()?;
                *kept = Some(key);
                key
            }
        };
        Ok(XChaCha20Poly1305::new(Key::from_slice(&key)))
    }

    /// A fresh nonce, then the sealed value.
    pub fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
        let nonce: [u8; NONCE] = rand::random();
        let sealed = self
            .cipher()?
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: LABEL })
            .map_err(|_| io::Error::other("the value could not be sealed"))?;
        let mut out = nonce.to_vec();
        out.extend(sealed);
        Ok(out)
    }

    /// The value, or an error when it was not sealed with this key and label
    /// or has been altered since.
    pub fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
        if sealed.len() < NONCE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a sealed value"));
        }
        let (nonce, body) = sealed.split_at(NONCE);
        self.cipher()?
            .decrypt(XNonce::from_slice(nonce), Payload { msg: body, aad: LABEL })
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the value does not open with this key"))
    }
}

/// The login Keychain as a [`KeySource`]: one generic password, service
/// "Mimic", account "credentials key".
#[cfg(target_os = "macos")]
pub mod login_keychain {
    use std::io;

    use security_framework::passwords::{get_generic_password, set_generic_password};

    use super::KeySource;

    const SERVICE: &str = "Mimic";
    const ACCOUNT: &str = "credentials key";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub struct LoginKeychain;

    impl KeySource for LoginKeychain {
        fn key(&self) -> io::Result<[u8; 32]> {
            match get_generic_password(SERVICE, ACCOUNT) {
                Ok(bytes) => bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the Keychain item is not Mimic's key")),
                Err(e) if e.code() == NOT_FOUND => {
                    let key: [u8; 32] = rand::random();
                    set_generic_password(SERVICE, ACCOUNT, &key).map_err(|e| io::Error::other(e.to_string()))?;
                    // Whatever the Keychain now holds is the key: read back,
                    // not assumed to be the one just made.
                    get_generic_password(SERVICE, ACCOUNT)
                        .map_err(|e| io::Error::other(e.to_string()))?
                        .as_slice()
                        .try_into()
                        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "the Keychain item is not Mimic's key"))
                }
                Err(e) => Err(io::Error::other(e.to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixed([u8; 32], AtomicUsize);

    impl KeySource for Fixed {
        fn key(&self) -> io::Result<[u8; 32]> {
            self.1.fetch_add(1, Ordering::SeqCst);
            Ok(self.0)
        }
    }

    struct Refusing;

    impl KeySource for Refusing {
        fn key(&self) -> io::Result<[u8; 32]> {
            Err(io::Error::other("the Keychain is locked"))
        }
    }

    fn sealer(byte: u8) -> KeyedSealer<Fixed> {
        KeyedSealer::new(Fixed([byte; 32], AtomicUsize::new(0)))
    }

    #[test]
    fn a_value_opens_with_the_key_it_was_sealed_with_and_no_other() {
        let s = sealer(7);
        let sealed = s.seal(b"sk-live").unwrap();
        assert_ne!(&sealed[NONCE..], b"sk-live", "the file never holds the value");
        assert_eq!(s.unseal(&sealed).unwrap(), b"sk-live");
        assert!(sealer(8).unseal(&sealed).is_err(), "another key opens nothing");
        assert!(s.unseal(b"short").is_err());
    }

    #[test]
    fn each_seal_is_new_and_an_altered_value_does_not_open() {
        let s = sealer(7);
        let a = s.seal(b"same").unwrap();
        let b = s.seal(b"same").unwrap();
        assert_ne!(a, b, "a fresh nonce each time");
        let mut altered = a.clone();
        let last = altered.len() - 1;
        altered[last] ^= 1;
        assert!(s.unseal(&altered).is_err());
        assert_eq!(s.source.1.load(Ordering::SeqCst), 1, "the key is asked for once");
    }

    #[test]
    fn without_the_key_nothing_is_sealed_or_opened() {
        let s = KeyedSealer::new(Refusing);
        assert!(s.seal(b"x").is_err());
        assert!(s.unseal(&[0u8; 40]).is_err());
    }
}
