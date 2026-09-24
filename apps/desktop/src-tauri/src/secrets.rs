//! Where credentials live: provider API keys and mailbox app passwords.
//!
//! **On Windows every value is sealed to the user's Windows account** with
//! DPAPI (`CryptProtectData`: current-user scope, no prompt, and an entropy
//! string of Mimic's own) before it is written. Copied off this computer —
//! a backup, a sync, a disk read by another account or another operating
//! system — the file cannot be opened without the user's Windows password.
//!
//! **What it does not stop, stated plainly:** a program running as this same
//! user can ask Windows to unseal the values, exactly as it could the
//! browser's saved passwords, and so can an administrator of this computer.
//! `docs/SECURITY_MODEL.md` keeps that out of scope and nothing on screen
//! claims otherwise.
//!
//! Elsewhere (development builds; macOS is unbuilt) the values are written
//! unsealed to a file only this user can open, and [`CredentialState`] says
//! so, so the screen reports what the store did rather than assuming.
//!
//! Before 0.10.0-alpha.6 the values were written unsealed to
//! `credentials.json`. Its values are moved into `secrets.json`, sealed,
//! written and read back from disk before they are marked as moved, and the
//! old file is deleted only after that — so a value that opens is never lost
//! with it, and one changed or cleared since is not brought back by it or by
//! a restored copy of it. If Windows will not seal, no value is written
//! unsealed: the old file stays and is used for the session.
//!
//! What was already true and still is: credentials are never written to the
//! database, never appear in a diagnostics bundle, and never reach a log.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use mimic_core::providers::SecretStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The sealed file, format 2.
const FILE: &str = "secrets.json";
/// The unsealed file every version before 0.10.0-alpha.6 wrote.
const LEGACY: &str = "credentials.json";
const FORMAT: u64 = 2;

/// How this build protects what it saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protection {
    /// Sealed to the user's Windows account (DPAPI).
    Account,
    /// Sealed with a key kept in the user's login Keychain (macOS). Compiled,
    /// never yet run on a Mac (`mimic-keychain`).
    Keychain,
    /// In a file only this user can open, not sealed.
    File,
}

impl Protection {
    /// Whether values are sealed, not merely kept in a file.
    pub fn seals(self) -> bool {
        self != Protection::File
    }
}

/// Seals a value before it is written and unseals it when it is read.
pub trait Sealer: Send + Sync {
    fn protection(&self) -> Protection;
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>>;
    fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>>;
}

/// No sealing: what a build with neither DPAPI nor the Keychain does, and
/// says it does.
#[cfg(any(not(any(windows, target_os = "macos")), test))]
pub struct Unsealed;

#[cfg(any(not(any(windows, target_os = "macos")), test))]
impl Sealer for Unsealed {
    fn protection(&self) -> Protection {
        Protection::File
    }
    fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
        Ok(plain.to_vec())
    }
    fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
        Ok(sealed.to_vec())
    }
}

/// The sealer this build uses.
pub fn platform_sealer() -> Box<dyn Sealer> {
    #[cfg(windows)]
    {
        Box::new(dpapi::Dpapi)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(keychain::Keychain(mimic_keychain::KeyedSealer::new(mimic_keychain::login_keychain::LoginKeychain)))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Box::new(Unsealed)
    }
}

/// A [`Sealer`] over any key source: on macOS, the login Keychain. Not yet
/// run on a Mac; see `mimic-keychain`.
#[cfg(any(target_os = "macos", test))]
mod keychain {
    use std::io;

    use mimic_keychain::{KeySource, KeyedSealer};

    use super::{Protection, Sealer};

    pub struct Keychain<K: KeySource>(pub KeyedSealer<K>);

    impl<K: KeySource> Sealer for Keychain<K> {
        fn protection(&self) -> Protection {
            Protection::Keychain
        }
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            self.0.seal(plain)
        }
        fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            self.0.unseal(sealed)
        }
    }
}

#[cfg(windows)]
mod dpapi {
    use std::io;
    use std::ptr;

    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    use super::{Protection, Sealer};

    /// Mimic's own entropy. Not a secret — it is in the binary — but a blob
    /// sealed with it does not open for a caller that does not pass it.
    const ENTROPY: &[u8] = b"mimic.credentials.v2";

    pub struct Dpapi;

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr().cast_mut() }
    }

    /// Make one DPAPI call and take the buffer it returns.
    fn run(call: impl FnOnce(*mut CRYPT_INTEGER_BLOB) -> BOOL) -> io::Result<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: ptr::null_mut() };
        if call(&mut out) == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: on success Windows filled `out` with a buffer it allocated
        // with LocalAlloc and gave to the caller; it is copied once, freed
        // once, and not touched again.
        unsafe {
            let bytes = if out.pbData.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec()
            };
            LocalFree(out.pbData.cast());
            Ok(bytes)
        }
    }

    impl Sealer for Dpapi {
        fn protection(&self) -> Protection {
            Protection::Account
        }

        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            let (input, entropy) = (blob(plain), blob(ENTROPY));
            // SAFETY: the inputs point at live slices for the whole call; the
            // description, reserved and prompt pointers may be null.
            run(|out| unsafe {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    out,
                )
            })
        }

        fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            let (input, entropy) = (blob(sealed), blob(ENTROPY));
            // SAFETY: as above; the description out-pointer may be null.
            run(|out| unsafe {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    out,
                )
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dpapi_opens_what_it_sealed_and_nothing_else() {
            let sealed = Dpapi.seal(b"sk-live").unwrap();
            assert_ne!(sealed.as_slice(), b"sk-live".as_slice());
            assert_eq!(Dpapi.unseal(&sealed).unwrap(), b"sk-live");
            assert!(Dpapi.unseal(b"not a sealed value").is_err());

            // Sealed for this account but without Mimic's entropy: refused.
            let input = blob(b"sk-live");
            let foreign = run(|out| unsafe {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    out,
                )
            })
            .unwrap();
            assert!(Dpapi.unseal(&foreign).is_err());
        }
    }
}

/// One saved value as it is on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    protection: Protection,
    /// Hex of the bytes the sealer produced — for `file`, the value itself.
    sealed: String,
}

#[derive(Serialize, Deserialize)]
struct SecretsFile {
    format: u64,
    values: BTreeMap<String, Stored>,
    /// For each value of the old unsealed file already dealt with — moved
    /// in, or cleared — a SHA-256 of the key and the value it held, sealed
    /// like a value. Kept after the old file is gone, so a restored copy of
    /// it brings nothing back. On Windows a mark is unsealed only when
    /// sealing was refused, which is only while the old file, holding the
    /// value itself, is on disk, and it is sealed or dropped once the old file
    /// is gone. Builds that do not seal keep every mark unsealed, as they do
    /// the values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    taken: BTreeMap<String, Stored>,
}

#[derive(Debug, Clone, Default)]
struct State {
    /// Every saved value as it is on disk, whether it opens here or not.
    stored: BTreeMap<String, Stored>,
    /// The saved values that opened here.
    plain: BTreeMap<String, String>,
    /// See [`SecretsFile::taken`].
    taken: BTreeMap<String, Stored>,
    /// This session only: the old file's values while they could not be
    /// moved in. Never written anywhere.
    from_old: BTreeMap<String, String>,
}

/// Why the sealed file is not in use, when it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unreadable {
    /// It could not be read and was renamed aside; saves start a new one.
    SetAside,
    /// It could not be read and could not be moved aside, or another
    /// version of Mimic wrote it: left as it is, and nothing is saved over it.
    LeftAlone,
}

/// What the store is doing for the user, for the screen. Never a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialState {
    /// How this build protects what it saves.
    pub protection: Protection,
    /// On a build that seals: something unsealed is still on disk — the old
    /// file, whether or not what was in it has been moved in.
    pub unsealed_left: bool,
    /// Keys saved but not openable here — sealed to another Windows account
    /// or computer, or before the account's password was reset. Kept, not
    /// deleted.
    pub locked: Vec<String>,
    /// Said in the session it happens, and logged.
    pub unreadable: Option<Unreadable>,
}

/// What opening the store found, for the log. Counts and reasons, never a
/// value.
#[derive(Debug, Clone, Default)]
pub struct OpenReport {
    /// Values moved in, sealed, from the old unsealed file.
    pub moved: usize,
    /// The old file could not be parsed; being unusable and unsealed, it was
    /// deleted.
    pub removed_damaged_old_file: bool,
    /// Why the old file is still there.
    pub old_file_kept: Option<String>,
    /// The sealed file could not be read and was renamed to this.
    pub set_aside: Option<PathBuf>,
    /// Why the sealed file was left alone.
    pub left_alone: Option<String>,
    /// Values saved but not openable here.
    pub locked: usize,
}

enum Legacy {
    Absent,
    /// Everything in it is now dealt with; `deleted` says whether it went.
    Moved {
        moved: usize,
        deleted: Result<(), String>,
    },
    Damaged {
        deleted: Result<(), String>,
    },
}

#[cfg(test)]
thread_local! {
    /// Makes deleting the old file fail, as a read-only or held file would.
    static REFUSE_DELETE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub struct FileSecretStore {
    dir: PathBuf,
    sealer: Box<dyn Sealer>,
    /// Held for writing across every change *and* its write to disk.
    state: RwLock<State>,
    unreadable: Option<Unreadable>,
    report: OpenReport,
}

impl FileSecretStore {
    pub fn open(dir: &Path) -> io::Result<Self> {
        Self::open_with(dir, platform_sealer())
    }

    pub fn open_with(dir: &Path, sealer: Box<dyn Sealer>) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut store = Self {
            dir: dir.to_path_buf(),
            sealer,
            state: RwLock::new(State::default()),
            unreadable: None,
            report: OpenReport::default(),
        };
        store.remove_stale_temporary_files();
        let path = store.dir.join(FILE);
        let mut state = match std::fs::read(&path) {
            Ok(bytes) => match parse_file(&bytes) {
                Ok(file) => store.state_from(file),
                Err(Some(format)) => {
                    store.unreadable = Some(Unreadable::LeftAlone);
                    store.report.left_alone = Some(format!("written in format {format}, not {FORMAT}"));
                    State::default()
                }
                Err(None) => {
                    // Not overwritten: what is in it is sealed, so keeping it
                    // costs nothing, and it may be recoverable by hand.
                    let aside = store.dir.join(format!("{FILE}.unreadable-{}", unix_now()));
                    match std::fs::rename(&path, &aside) {
                        Ok(()) => {
                            store.unreadable = Some(Unreadable::SetAside);
                            store.report.set_aside = Some(aside);
                        }
                        Err(e) => {
                            store.unreadable = Some(Unreadable::LeftAlone);
                            store.report.left_alone = Some(format!("could not be read or moved aside: {e}"));
                        }
                    }
                    State::default()
                }
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e),
        };

        let moving = if store.unreadable != Some(Unreadable::LeftAlone) {
            store.take_legacy(&mut state)
        } else if store.dir.join(LEGACY).exists() {
            Err(io::Error::other("the sealed file cannot be written"))
        } else {
            Ok(Legacy::Absent)
        };
        match moving {
            Ok(Legacy::Absent) => {}
            Ok(Legacy::Moved { moved, deleted }) => {
                store.report.moved = moved;
                store.report.old_file_kept = deleted.err();
            }
            Ok(Legacy::Damaged { deleted }) => match deleted {
                Ok(()) => store.report.removed_damaged_old_file = true,
                Err(e) => store.report.old_file_kept = Some(e),
            },
            Err(e) => {
                // Nothing was written unsealed and the old file stays. Its
                // values are used for this session, and every save tries the
                // move again first.
                store.report.old_file_kept = Some(e.to_string());
                store.legacy_for_now(&mut state);
            }
        }
        store.report.locked = locked_in(&state).len();
        store.state = RwLock::new(state);
        store.restrict();
        Ok(store)
    }

    pub fn report(&self) -> &OpenReport {
        &self.report
    }

    /// Which keys have a value that can be used here. Never the values
    /// themselves — this is what the Settings screen shows.
    pub fn keys(&self) -> Vec<String> {
        let state = self.read();
        let mut keys: Vec<String> = state.plain.keys().chain(state.from_old.keys()).cloned().collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Keys saved but not openable here.
    pub fn locked(&self) -> Vec<String> {
        locked_in(&self.read())
    }

    /// How this build protects what it saves.
    pub fn protection(&self) -> Protection {
        self.sealer.protection()
    }

    /// On a build that seals, whether anything unsealed is still on disk.
    pub fn unsealed_left(&self) -> bool {
        if !self.sealer.protection().seals() {
            return false;
        }
        let state = self.read();
        self.dir.join(LEGACY).exists()
            || !state.from_old.is_empty()
            || state.stored.values().any(|s| s.protection == Protection::File)
    }

    pub fn credential_state(&self) -> CredentialState {
        CredentialState {
            protection: self.protection(),
            unsealed_left: self.unsealed_left(),
            locked: self.locked(),
            unreadable: self.unreadable,
        }
    }

    /// Save a value, or clear it when blank. Memory changes only once the
    /// file has, and a value that cannot be sealed is not saved at all.
    pub fn set(&self, key: &str, value: &str) -> io::Result<()> {
        if self.unreadable == Some(Unreadable::LeftAlone) {
            return Err(io::Error::other(
                "the file saved passwords are kept in can't be read, so I won't save over it",
            ));
        }
        let mut state = self.state.write().unwrap_or_else(|p| p.into_inner());
        // Another copy of the app may have saved since: start from the file.
        self.reload(&mut state)?;
        let blank = value.trim().is_empty();
        // An old unsealed file still here is moved first. Clearing a value
        // does not wait on that.
        if let Err(e) = self.take_legacy(&mut state) {
            if !blank {
                return Err(e);
            }
        }
        let mut next = state.clone();
        next.from_old.remove(key);
        if blank {
            next.stored.remove(key);
            next.plain.remove(key);
            // Still in the old file and not yet moved: mark it dealt with,
            // or the next start would bring it back. An old file that is
            // there but cannot be read now fails the clear rather than
            // leaving it unmarked.
            if let Some(old) = self.old_values_strict()?.and_then(|mut old| old.remove(key)) {
                if !old.trim().is_empty() && !self.is_marked(&next, key, &old) {
                    next.taken.insert(key.to_string(), self.mark(key, &old));
                }
            }
        } else {
            next.stored.insert(key.to_string(), self.seal(value)?);
            next.plain.insert(key.to_string(), value.to_string());
        }
        self.write(&next)?;
        *state = next;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> io::Result<()> {
        self.set(key, "")
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(|p| p.into_inner())
    }

    fn state_from(&self, file: SecretsFile) -> State {
        let mut state = State { taken: file.taken, ..State::default() };
        for (key, stored) in file.values {
            if let Some(value) = unseal(self.sealer.as_ref(), &stored) {
                state.plain.insert(key.clone(), value);
            }
            state.stored.insert(key, stored);
        }
        state
    }

    /// Replace what is held with what is on disk, keeping this session's
    /// values from the old file. A file that cannot be read now is not saved
    /// over.
    fn reload(&self, state: &mut State) -> io::Result<()> {
        let from_old = std::mem::take(&mut state.from_old);
        let bytes = match std::fs::read(self.dir.join(FILE)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Gone — deleted by hand, or set aside by another copy of the
                // app: what was in it is not written back.
                *state = State { from_old, ..State::default() };
                return Ok(());
            }
            Err(e) => {
                state.from_old = from_old;
                return Err(e);
            }
        };
        let file = parse_file(&bytes).map_err(|_| {
            io::Error::other("the file saved passwords are kept in can't be read, so I won't save over it")
        });
        let file = match file {
            Ok(file) => file,
            Err(e) => {
                state.from_old = from_old;
                return Err(e);
            }
        };
        *state = State { from_old, ..self.state_from(file) };
        Ok(())
    }

    fn seal(&self, value: &str) -> io::Result<Stored> {
        let sealed = self.sealer.seal(value.as_bytes())?;
        Ok(Stored { protection: self.sealer.protection(), sealed: hex::encode(sealed) })
    }

    fn old_values(&self) -> Option<BTreeMap<String, String>> {
        parse_legacy(&std::fs::read(self.dir.join(LEGACY)).ok()?)
    }

    /// The old file's values, `None` when there is no old file (or nothing
    /// usable in it), and an error when it is there but cannot be read now.
    fn old_values_strict(&self) -> io::Result<Option<BTreeMap<String, String>>> {
        match std::fs::read(self.dir.join(LEGACY)) {
            Ok(bytes) => Ok(parse_legacy(&bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// The mark for a value of the old file: sealed when sealing works.
    fn mark(&self, key: &str, value: &str) -> Stored {
        let hash = fingerprint(key, value);
        match self.sealer.seal(&hash) {
            Ok(sealed) => Stored { protection: self.sealer.protection(), sealed: hex::encode(sealed) },
            Err(_) => Stored { protection: Protection::File, sealed: hex::encode(hash) },
        }
    }

    fn is_marked(&self, state: &State, key: &str, value: &str) -> bool {
        let Some(mark) = state.taken.get(key) else { return false };
        let Ok(bytes) = hex::decode(&mark.sealed) else { return false };
        let opened = if mark.protection == Protection::File {
            Some(bytes)
        } else if mark.protection == self.sealer.protection() {
            self.sealer.unseal(&bytes).ok()
        } else {
            None
        };
        opened.as_deref() == Some(fingerprint(key, value).as_slice())
    }

    /// Move the old unsealed file's values in, sealed: each one not already
    /// dealt with, which is every one the first time, and afterwards only one
    /// an older version of Mimic saved there since. The values are written
    /// and read back from disk before they are marked as dealt with, and the
    /// old file is deleted only then — so a value that opens is never lost
    /// with it, and a value changed or cleared since is not brought back.
    fn take_legacy(&self, state: &mut State) -> io::Result<Legacy> {
        let path = self.dir.join(LEGACY);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Gone, perhaps mid-session: nothing of it is used any more.
                state.from_old.clear();
                self.tidy_marks(state);
                return Ok(Legacy::Absent);
            }
            Err(e) => return Err(e),
        };
        let Some(old) = parse_legacy(&bytes) else {
            // Not a JSON object at all — half-written by the old store, which
            // did not write atomically. Nothing in it can be used, and
            // whatever can be read of it is unsealed, so it goes.
            let deleted = self.delete_old_file().map_err(|e| e.to_string());
            if deleted.is_ok() {
                self.tidy_marks(state);
            }
            return Ok(Legacy::Damaged { deleted });
        };
        let mut next = state.clone();
        let mut moved = Vec::new();
        for (key, value) in old {
            if value.trim().is_empty() {
                continue;
            }
            if self.is_marked(&next, &key, &value) {
                continue;
            }
            next.stored.insert(key.clone(), self.seal(&value)?);
            next.plain.insert(key.clone(), value.clone());
            moved.push((key, value));
        }
        if !moved.is_empty() {
            self.write(&next)?;
            self.verify(&moved)?;
            for (key, value) in &moved {
                next.taken.insert(key.clone(), self.mark(key, value));
            }
            self.write(&next)?;
        }
        next.from_old.clear();
        *state = next;
        let deleted = self.delete_old_file().map_err(|e| e.to_string());
        if deleted.is_ok() {
            self.tidy_marks(state);
        }
        Ok(Legacy::Moved { moved: moved.len(), deleted })
    }

    /// With the old file gone, a mark left unsealed (written while sealing
    /// was refused) is sealed now, or dropped if it still cannot be. Best
    /// effort: it is tried again at every start and save.
    fn tidy_marks(&self, state: &mut State) {
        if !self.sealer.protection().seals() || state.taken.values().all(|m| m.protection != Protection::File) {
            return;
        }
        let mut next = state.clone();
        let mut changed = false;
        for (key, mark) in state.taken.iter().filter(|(_, m)| m.protection == Protection::File) {
            changed = true;
            let resealed = hex::decode(&mark.sealed).ok().and_then(|hash| self.sealer.seal(&hash).ok());
            match resealed {
                Some(sealed) => {
                    next.taken.insert(
                        key.clone(),
                        Stored { protection: self.sealer.protection(), sealed: hex::encode(sealed) },
                    );
                }
                None => {
                    next.taken.remove(key);
                }
            }
        }
        if changed && self.write(&next).is_ok() {
            *state = next;
        }
    }

    /// Use the old file's values for this session without saving them.
    fn legacy_for_now(&self, state: &mut State) {
        let Some(old) = self.old_values() else { return };
        for (key, value) in old {
            if !value.trim().is_empty() && !self.is_marked(state, &key, &value) {
                state.from_old.insert(key, value);
            }
        }
    }

    fn delete_old_file(&self) -> io::Result<()> {
        #[cfg(test)]
        {
            if REFUSE_DELETE.with(|r| r.get()) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "delete refused"));
            }
        }
        match std::fs::remove_file(self.dir.join(LEGACY)) {
            // Already gone — another copy of the app got there first.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// Read the sealed file back from disk and check each moved value opens
    /// to what it was.
    fn verify(&self, moved: &[(String, String)]) -> io::Result<()> {
        let bytes = std::fs::read(self.dir.join(FILE))?;
        let file = parse_file(&bytes).map_err(|_| io::Error::other("the sealed file did not read back"))?;
        for (key, value) in moved {
            let back = file.values.get(key).and_then(|s| unseal(self.sealer.as_ref(), s));
            if back.as_deref() != Some(value.as_str()) {
                return Err(io::Error::other(format!("{key} did not read back as it was saved")));
            }
        }
        Ok(())
    }

    /// Replace the sealed file in one step, so a crash leaves either the
    /// previous file or this one, never half of one.
    fn write(&self, state: &State) -> io::Result<()> {
        let file = SecretsFile { format: FORMAT, values: state.stored.clone(), taken: state.taken.clone() };
        let text = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
        // Per process, so two copies of the app saving at once cannot write
        // into the same temporary file.
        let tmp = self.dir.join(format!("{FILE}.{}.tmp", std::process::id()));
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut f = options.open(&tmp)?;
            f.write_all(&text)?;
            f.sync_all()?;
        }
        if let Err(e) = std::fs::rename(&tmp, self.dir.join(FILE)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        self.restrict();
        Ok(())
    }

    /// Temporary files a crash left behind. They hold only sealed values; a
    /// minute's grace keeps this from racing another copy of the app.
    fn remove_stale_temporary_files(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return };
        let grace = std::time::Duration::from_secs(60);
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|at| at.elapsed().ok())
                .is_some_and(|age| age > grace);
            if name.starts_with(&format!("{FILE}.")) && name.ends_with(".tmp") && stale {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    fn restrict(&self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(self.dir.join(FILE), std::fs::Permissions::from_mode(0o600));
        }
    }
}

fn locked_in(state: &State) -> Vec<String> {
    state.stored.keys().filter(|k| !state.plain.contains_key(*k) && !state.from_old.contains_key(*k)).cloned().collect()
}

/// A SHA-256 of a key and the value the old file held under it.
fn fingerprint(key: &str, value: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(key.as_bytes());
    hash.update([0u8]);
    hash.update(value.as_bytes());
    hash.finalize().to_vec()
}

/// The sealed file, or why not: `Some(format)` for a format this version
/// does not write, `None` for anything that is not the file at all.
fn parse_file(bytes: &[u8]) -> Result<SecretsFile, Option<u64>> {
    let value: Value = serde_json::from_slice(strip_bom(bytes)).map_err(|_| None)?;
    match value.get("format").and_then(Value::as_u64) {
        Some(FORMAT) => serde_json::from_value(value).map_err(|_| None),
        Some(other) => Err(Some(other)),
        None => Err(None),
    }
}

/// The old file's string values. A byte-order mark (Notepad adds one) and
/// values that are not strings are tolerated; only something that is not a
/// JSON object at all is damaged.
fn parse_legacy(bytes: &[u8]) -> Option<BTreeMap<String, String>> {
    let map: BTreeMap<String, Value> = serde_json::from_slice(strip_bom(bytes)).ok()?;
    Some(map.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect())
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

fn unseal(sealer: &dyn Sealer, stored: &Stored) -> Option<String> {
    if stored.protection != sealer.protection() {
        return None;
    }
    let sealed = hex::decode(&stored.sealed).ok()?;
    String::from_utf8(sealer.unseal(&sealed).ok()?).ok()
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl SecretStore for FileSecretStore {
    fn get(&self, key: &str) -> Option<String> {
        let state = self.read();
        state.plain.get(key).or_else(|| state.from_old.get(key)).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Stands in for DPAPI where it does not exist: seals by tagging and
    /// scrambling, and refuses anything it did not seal.
    struct Tagged;
    impl Sealer for Tagged {
        fn protection(&self) -> Protection {
            Protection::Account
        }
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            let mut out = b"TAG".to_vec();
            out.extend(plain.iter().rev().map(|b| b ^ 0x5a));
            Ok(out)
        }
        fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            let body = sealed.strip_prefix(b"TAG").ok_or_else(|| io::Error::other("not sealed here"))?;
            Ok(body.iter().rev().map(|b| b ^ 0x5a).collect())
        }
    }

    /// A Windows that will not seal for the first `n` calls, then will.
    struct RefusingFor(AtomicUsize);
    impl Sealer for RefusingFor {
        fn protection(&self) -> Protection {
            Protection::Account
        }
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            if self.0.load(Ordering::SeqCst) > 0 {
                self.0.fetch_sub(1, Ordering::SeqCst);
                return Err(io::Error::other("sealing refused"));
            }
            Tagged.seal(plain)
        }
        fn unseal(&self, sealed: &[u8]) -> io::Result<Vec<u8>> {
            Tagged.unseal(sealed)
        }
    }

    /// Seals, but what it seals does not open again.
    struct SealsOnly;
    impl Sealer for SealsOnly {
        fn protection(&self) -> Protection {
            Protection::Account
        }
        fn seal(&self, plain: &[u8]) -> io::Result<Vec<u8>> {
            Tagged.seal(plain)
        }
        fn unseal(&self, _: &[u8]) -> io::Result<Vec<u8>> {
            Err(io::Error::other("does not open"))
        }
    }

    fn tagged(dir: &Path) -> FileSecretStore {
        FileSecretStore::open_with(dir, Box::new(Tagged)).unwrap()
    }

    fn refusing(dir: &Path, times: usize) -> FileSecretStore {
        FileSecretStore::open_with(dir, Box::new(RefusingFor(AtomicUsize::new(times)))).unwrap()
    }

    fn on_disk(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(FILE)).unwrap_or_default()
    }

    fn write_old(dir: &Path, text: &str) {
        std::fs::write(dir.join(LEGACY), text).unwrap();
    }

    fn old_exists(dir: &Path) -> bool {
        dir.join(LEGACY).exists()
    }

    /// The login Keychain's part, played by a key the test holds.
    struct TestKey([u8; 32]);
    impl mimic_keychain::KeySource for TestKey {
        fn key(&self) -> io::Result<[u8; 32]> {
            Ok(self.0)
        }
    }

    fn keychain(dir: &Path, byte: u8) -> FileSecretStore {
        let sealer = keychain::Keychain(mimic_keychain::KeyedSealer::new(TestKey([byte; 32])));
        FileSecretStore::open_with(dir, Box::new(sealer)).unwrap()
    }

    #[test]
    fn with_the_keychain_a_value_is_sealed_and_opens_only_with_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = keychain(dir.path(), 1);
        store.set("provider.anthropic.apiKey", "sk-live-secret").unwrap();
        assert!(!on_disk(dir.path()).contains("sk-live-secret"));
        assert!(on_disk(dir.path()).contains("\"keychain\""), "{}", on_disk(dir.path()));
        assert_eq!(store.protection(), Protection::Keychain);
        assert!(!store.unsealed_left());
        assert_eq!(
            serde_json::to_value(store.credential_state()).unwrap()["protection"],
            serde_json::json!("keychain")
        );

        let reopened = keychain(dir.path(), 1);
        assert_eq!(reopened.get("provider.anthropic.apiKey").as_deref(), Some("sk-live-secret"));
        // Another key — another Mac, another user — opens nothing, and
        // deletes nothing.
        let elsewhere = keychain(dir.path(), 2);
        assert_eq!(elsewhere.get("provider.anthropic.apiKey"), None);
        assert_eq!(elsewhere.locked(), vec!["provider.anthropic.apiKey"]);
        assert_eq!(keychain(dir.path(), 1).get("provider.anthropic.apiKey").as_deref(), Some("sk-live-secret"));
    }

    #[test]
    fn a_secret_survives_a_restart_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::open(dir.path()).unwrap();
        assert!(store.keys().is_empty());
        store.set("provider.anthropic.apiKey", "sk-test").unwrap();

        let reopened = FileSecretStore::open(dir.path()).unwrap();
        assert_eq!(reopened.get("provider.anthropic.apiKey").as_deref(), Some("sk-test"));
        assert_eq!(reopened.keys(), vec!["provider.anthropic.apiKey"], "keys are listable, values are not");
        assert_eq!(reopened.protection(), platform_sealer().protection());
        assert!(!reopened.unsealed_left());

        reopened.remove("provider.anthropic.apiKey").unwrap();
        assert!(FileSecretStore::open(dir.path()).unwrap().keys().is_empty());
    }

    #[test]
    fn a_blank_value_clears_rather_than_storing_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let store = tagged(dir.path());
        store.set("k", "v").unwrap();
        store.set("k", "   ").unwrap();
        assert_eq!(store.get("k"), None);
        assert!(!on_disk(dir.path()).contains("\"k\""));
    }

    #[test]
    fn what_is_written_is_sealed_and_opens_again() {
        let dir = tempfile::tempdir().unwrap();
        tagged(dir.path()).set("imap:abc", "app-password-1").unwrap();
        let raw = on_disk(dir.path());
        assert!(!raw.contains("app-password-1"));
        assert!(!raw.contains(&hex::encode("app-password-1")));
        let reopened = tagged(dir.path());
        assert_eq!(reopened.get("imap:abc").as_deref(), Some("app-password-1"));
        assert_eq!(reopened.protection(), Protection::Account);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "no temporary file is left behind: {leftovers:?}");
    }

    #[test]
    fn the_old_unsealed_file_is_moved_in_sealed_and_then_deleted() {
        let dir = tempfile::tempdir().unwrap();
        // With the byte-order mark Notepad adds, and a value that is not a
        // string: neither makes the file damaged.
        write_old(
            dir.path(),
            "\u{feff}{\"provider.anthropic.apiKey\": \"sk-old\", \"imap:abc\": \"pw-old\", \"blank\": \"  \", \"n\": 3}",
        );
        let store = tagged(dir.path());
        assert_eq!(store.report().moved, 2);
        assert!(store.report().old_file_kept.is_none());
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-old"));
        assert_eq!(store.get("imap:abc").as_deref(), Some("pw-old"));
        assert!(!old_exists(dir.path()), "the unsealed file is gone");
        let raw = on_disk(dir.path());
        assert!(!raw.contains("sk-old") && !raw.contains("pw-old"));
        assert!(!raw.contains(&hex::encode(fingerprint("imap:abc", "pw-old"))), "the marks are sealed like the values");
        assert!(!store.unsealed_left());
        // And it stays moved.
        assert_eq!(tagged(dir.path()).get("imap:abc").as_deref(), Some("pw-old"));
    }

    #[test]
    fn a_key_saved_by_an_older_version_after_this_one_ran_is_not_lost() {
        // This version ran and moved the old file in; an older one was
        // reinstalled and the key saved there; then this version again.
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-before"}"#);
        tagged(dir.path());
        assert!(!old_exists(dir.path()));
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-replaced"}"#);

        let store = tagged(dir.path());
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-replaced"));
        assert!(!old_exists(dir.path()));
        assert_eq!(tagged(dir.path()).get("provider.anthropic.apiKey").as_deref(), Some("sk-replaced"));
    }

    #[test]
    fn when_the_old_file_stays_only_what_an_older_version_changed_in_it_is_taken_again() {
        // The old file could not be deleted; the key was changed here, and
        // then an older version changed a different one in the old file.
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-1", "imap:abc": "pw-1"}"#);
        REFUSE_DELETE.with(|r| r.set(true));
        tagged(dir.path()).set("provider.anthropic.apiKey", "sk-2").unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-1", "imap:abc": "pw-2"}"#);
        REFUSE_DELETE.with(|r| r.set(false));

        let store = tagged(dir.path());
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-2"), "a stale value does not win");
        assert_eq!(store.get("imap:abc").as_deref(), Some("pw-2"), "a changed one does");
        assert!(!old_exists(dir.path()));
    }

    #[test]
    fn when_windows_will_not_seal_nothing_is_written_unsealed_and_the_old_file_stays() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-old", "imap:gone": "pw-gone"}"#);
        let store = refusing(dir.path(), usize::MAX);
        assert!(store.report().old_file_kept.is_some());
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-old"), "still usable this session");
        assert!(old_exists(dir.path()));
        assert!(store.unsealed_left(), "and the screen is told something unsealed is left");

        assert!(store.set("imap:abc", "pw-new").is_err(), "a value that cannot be sealed is not saved");
        assert_eq!(store.get("imap:abc"), None);
        assert!(!on_disk(dir.path()).contains("pw-new"));
        assert!(old_exists(dir.path()), "a failed save does not lose the old file");
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-old"));

        // Clearing does not wait on sealing, and marks the value dealt with
        // so it does not come back; the old file itself is not rewritten.
        store.remove("imap:gone").unwrap();
        assert_eq!(store.get("imap:gone"), None);
        assert!(refusing(dir.path(), usize::MAX).get("imap:gone").is_none());

        // Once sealing works, the next start moves the rest in, and the mark
        // written unsealed while it could not is sealed.
        let later = tagged(dir.path());
        assert_eq!(later.report().moved, 1);
        assert_eq!(later.get("provider.anthropic.apiKey").as_deref(), Some("sk-old"));
        assert_eq!(later.get("imap:gone"), None);
        assert!(!old_exists(dir.path()));
        assert!(!on_disk(dir.path()).contains(&hex::encode(fingerprint("imap:gone", "pw-gone"))));
    }

    #[test]
    fn a_save_moves_the_old_file_in_once_sealing_works() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-old"}"#);
        // Refuses the move at start-up, then works.
        let store = refusing(dir.path(), 1);
        assert!(store.report().old_file_kept.is_some());
        store.set("imap:abc", "pw").unwrap();
        assert!(!old_exists(dir.path()));
        assert!(!store.unsealed_left());
        let reopened = tagged(dir.path());
        assert_eq!(reopened.get("provider.anthropic.apiKey").as_deref(), Some("sk-old"));
        assert_eq!(reopened.get("imap:abc").as_deref(), Some("pw"));
    }

    #[test]
    fn the_old_file_is_kept_until_what_was_moved_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"imap:abc": "pw-old"}"#);
        let store = FileSecretStore::open_with(dir.path(), Box::new(SealsOnly)).unwrap();
        assert!(store.report().old_file_kept.is_some());
        assert_eq!(store.get("imap:abc").as_deref(), Some("pw-old"));
        assert!(old_exists(dir.path()));

        // Saves in the meantime do not take the only copy that opens with them.
        assert!(store.set("other", "x").is_err());
        store.remove("nothing").unwrap();
        assert!(old_exists(dir.path()));
        let again = FileSecretStore::open_with(dir.path(), Box::new(SealsOnly)).unwrap();
        assert!(old_exists(dir.path()));
        assert_eq!(again.get("imap:abc").as_deref(), Some("pw-old"));
        assert!(again.locked().is_empty(), "a value usable from the old file is not called locked");

        let working = tagged(dir.path());
        assert_eq!(working.get("imap:abc").as_deref(), Some("pw-old"));
        assert!(!old_exists(dir.path()));
    }

    #[test]
    fn a_saved_value_that_no_longer_opens_is_replaced_from_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"format": 2, "values": {"imap:abc": {"protection": "account", "sealed": "00ff"}}}"#,
        )
        .unwrap();
        write_old(dir.path(), r#"{"imap:abc": "pw-old"}"#);
        let store = tagged(dir.path());
        assert_eq!(store.get("imap:abc").as_deref(), Some("pw-old"));
        assert!(store.locked().is_empty());
        assert!(!old_exists(dir.path()));
    }

    #[test]
    fn when_the_old_file_cannot_be_deleted_saves_still_work_and_a_cleared_key_stays_cleared() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-old", "imap:abc": "pw-old"}"#);
        REFUSE_DELETE.with(|r| r.set(true));
        let store = tagged(dir.path());
        assert_eq!(store.report().moved, 2);
        assert!(store.report().old_file_kept.is_some());
        assert!(store.unsealed_left(), "the old file is still on disk, and the screen says so");

        store.set("provider.anthropic.apiKey", "sk-new").unwrap();
        store.remove("imap:abc").unwrap();
        let still = tagged(dir.path());
        assert_eq!(still.get("provider.anthropic.apiKey").as_deref(), Some("sk-new"));
        assert_eq!(still.get("imap:abc"), None, "a cleared key does not come back from the old file");

        REFUSE_DELETE.with(|r| r.set(false));
        let later = tagged(dir.path());
        assert!(!old_exists(dir.path()));
        assert_eq!(later.get("provider.anthropic.apiKey").as_deref(), Some("sk-new"));
        assert_eq!(later.get("imap:abc"), None);
    }

    #[test]
    fn a_restored_copy_of_the_old_file_brings_nothing_back() {
        let dir = tempfile::tempdir().unwrap();
        let old = r#"{"provider.anthropic.apiKey": "sk-1", "imap:abc": "pw-1"}"#;
        write_old(dir.path(), old);
        let store = tagged(dir.path());
        assert!(!old_exists(dir.path()));
        store.set("provider.anthropic.apiKey", "sk-2").unwrap();
        store.remove("imap:abc").unwrap();

        write_old(dir.path(), old);
        let store = tagged(dir.path());
        assert_eq!(store.report().moved, 0);
        assert_eq!(store.get("provider.anthropic.apiKey").as_deref(), Some("sk-2"));
        assert_eq!(store.get("imap:abc"), None);
        assert!(!old_exists(dir.path()));
    }

    #[test]
    fn a_save_after_the_file_was_deleted_by_hand_does_not_write_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = tagged(dir.path());
        store.set("k1", "v1").unwrap();
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        store.set("k2", "v2").unwrap();
        assert_eq!(tagged(dir.path()).keys(), vec!["k2"]);
    }

    #[test]
    fn a_save_starts_from_the_file_so_another_copy_of_the_app_is_not_undone() {
        let dir = tempfile::tempdir().unwrap();
        let a = tagged(dir.path());
        let b = tagged(dir.path());
        a.set("k1", "v1").unwrap();
        b.set("k2", "v2").unwrap();
        a.remove("k2").unwrap();
        b.set("k3", "v3").unwrap();
        let now = tagged(dir.path());
        assert_eq!(now.keys(), vec!["k1", "k3"]);
    }

    #[test]
    fn a_value_that_does_not_open_here_is_kept_and_reported_locked() {
        let dir = tempfile::tempdir().unwrap();
        // Sealed by "another account": a blob this sealer refuses.
        std::fs::write(
            dir.path().join(FILE),
            r#"{"format": 2, "values": {"imap:abc": {"protection": "account", "sealed": "00ff"}}}"#,
        )
        .unwrap();
        let store = tagged(dir.path());
        assert_eq!(store.get("imap:abc"), None);
        assert!(store.keys().is_empty());
        assert_eq!(store.locked(), vec!["imap:abc"]);
        assert_eq!(store.report().locked, 1);

        store.set("provider.anthropic.apiKey", "sk").unwrap();
        assert!(on_disk(dir.path()).contains("00ff"), "a save of something else does not drop it");

        store.set("imap:abc", "pw-again").unwrap();
        assert!(store.locked().is_empty(), "typing it again replaces it");
        assert_eq!(tagged(dir.path()).get("imap:abc").as_deref(), Some("pw-again"));
    }

    #[test]
    fn a_value_written_unsealed_is_not_read_as_sealed_or_the_other_way_round() {
        let dir = tempfile::tempdir().unwrap();
        FileSecretStore::open_with(dir.path(), Box::new(Unsealed)).unwrap().set("k", "v").unwrap();
        let store = tagged(dir.path());
        assert_eq!(store.get("k"), None);
        assert_eq!(store.locked(), vec!["k"]);
        assert!(store.unsealed_left(), "an unsealed value on disk is said to be there");
    }

    #[test]
    fn a_damaged_old_file_is_deleted_rather_than_left_unsealed() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.api"#);
        let store = tagged(dir.path());
        assert!(store.report().removed_damaged_old_file);
        assert!(!old_exists(dir.path()));
        assert!(store.keys().is_empty());
    }

    #[test]
    fn a_damaged_sealed_file_is_set_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE), "{not json").unwrap();
        let store = tagged(dir.path());
        let aside = store.report().set_aside.clone().expect("set aside");
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{not json");
        assert_eq!(store.credential_state().unreadable, Some(Unreadable::SetAside));
        store.set("k", "v").unwrap();
        assert_eq!(tagged(dir.path()).get("k").as_deref(), Some("v"));
    }

    #[test]
    fn a_file_another_version_wrote_is_left_alone_and_not_saved_over() {
        let dir = tempfile::tempdir().unwrap();
        let newer = r#"{"format": 3, "values": {}, "somethingNew": true}"#;
        std::fs::write(dir.path().join(FILE), newer).unwrap();
        let store = tagged(dir.path());
        assert_eq!(store.credential_state().unreadable, Some(Unreadable::LeftAlone));
        assert!(store.set("k", "v").is_err());
        assert!(store.remove("k").is_err());
        assert_eq!(on_disk(dir.path()), newer);
    }

    #[test]
    fn temporary_files_a_crash_left_behind_are_cleared_away() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join(format!("{FILE}.99999.tmp"));
        let fresh = dir.path().join(format!("{FILE}.99998.tmp"));
        for p in [&stale, &fresh] {
            std::fs::write(p, "{}").unwrap();
        }
        let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(an_hour_ago).unwrap();
        tagged(dir.path());
        assert!(!stale.exists());
        assert!(fresh.exists(), "one another copy of the app may be writing is left");
    }

    #[test]
    fn saves_from_several_threads_are_all_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(tagged(dir.path()));
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = store.clone();
                std::thread::spawn(move || store.set(&format!("k{i}"), &format!("v{i}")).unwrap())
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(tagged(dir.path()).keys().len(), 8);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = FileSecretStore::open(dir.path()).unwrap();
        store.set("k", "v").unwrap();
        let mode = std::fs::metadata(dir.path().join(FILE)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_values_are_sealed_to_the_account() {
        let dir = tempfile::tempdir().unwrap();
        write_old(dir.path(), r#"{"provider.anthropic.apiKey": "sk-live-old"}"#);
        let store = FileSecretStore::open(dir.path()).unwrap();
        assert_eq!(store.report().moved, 1);
        store.set("imap:abc", "app-password-live").unwrap();
        assert_eq!(store.protection(), Protection::Account);
        assert!(!store.unsealed_left());
        let raw = on_disk(dir.path());
        for plain in ["sk-live-old", "app-password-live"] {
            assert!(!raw.contains(plain) && !raw.contains(&hex::encode(plain)));
        }
        let reopened = FileSecretStore::open(dir.path()).unwrap();
        assert_eq!(reopened.get("provider.anthropic.apiKey").as_deref(), Some("sk-live-old"));
        assert_eq!(reopened.get("imap:abc").as_deref(), Some("app-password-live"));
    }
}
