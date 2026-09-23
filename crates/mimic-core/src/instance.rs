//! One Mimic per data folder.
//!
//! Two processes on one database each run the job queue. At start-up each
//! marks the other's running jobs interrupted and queues them again; both then
//! take the same queued job, check the same mailbox at once, and write the
//! same credentials file. Two copies running is easy to cause — a second click
//! on the icon, a shortcut and the Start menu — so it is refused here.
//!
//! The desktop shell hands a second launch to the first before it gets this
//! far. This lock is what makes "one at a time" true of the data itself,
//! whatever started the second process: an operating-system lock on a file
//! beside the database, held for the life of the process and released by the
//! system when the process ends, however it ends — so a crash never leaves
//! Mimic locked out of its own data.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Held for as long as this process uses the data folder. Dropping it (or the
/// process ending) lets another Mimic in.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

/// Another process holds the lock: a Mimic already using this data folder.
#[derive(Debug, thiserror::Error)]
#[error("another Mimic is already using {}", .0.display())]
pub struct AlreadyRunning(pub PathBuf);

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error(transparent)]
    AlreadyRunning(#[from] AlreadyRunning),
    #[error("could not lock {}: {source}", path.display())]
    Io { path: PathBuf, source: std::io::Error },
}

impl InstanceLock {
    /// Take the lock at `path`, creating the file if needed. Never waits: a
    /// lock held elsewhere is `AlreadyRunning` at once. The file's contents
    /// are never read or written; only the lock on it matters.
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let io = |source| LockError::Io { path: path.to_path_buf(), source };
        let file = OpenOptions::new().create(true).read(true).write(true).truncate(false).open(path).map_err(io)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path: path.to_path_buf() }),
            Err(TryLockError::WouldBlock) => Err(AlreadyRunning(path.to_path_buf()).into()),
            Err(TryLockError::Error(e)) => Err(io(e)),
        }
    }

    /// `acquire`, trying again for up to `wait` while the lock is held: a
    /// Mimic restarting (after an update, say) starts the new process while
    /// the old one is still letting go.
    pub fn acquire_within(path: &Path, wait: Duration) -> Result<Self, LockError> {
        let started = Instant::now();
        loop {
            match Self::acquire(path) {
                Err(LockError::AlreadyRunning(_)) if started.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                other => return other,
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_holder_is_refused_until_the_first_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mimic.lock");
        let first = InstanceLock::acquire(&path).unwrap();
        assert_eq!(first.path(), path);

        // A separate open of the same file is a separate holder, as another
        // process would be.
        match InstanceLock::acquire(&path) {
            Err(LockError::AlreadyRunning(AlreadyRunning(p))) => assert_eq!(p, path),
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }

        drop(first);
        let again = InstanceLock::acquire(&path).expect("free once the first is gone");
        drop(again);
        assert!(path.exists(), "the file stays; only the lock comes and goes");
    }

    #[test]
    fn a_holder_letting_go_is_waited_for_and_one_that_stays_is_not_waited_on_forever() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mimic.lock");

        let stays = InstanceLock::acquire(&path).unwrap();
        let started = Instant::now();
        assert!(matches!(
            InstanceLock::acquire_within(&path, Duration::from_millis(250)),
            Err(LockError::AlreadyRunning(_))
        ));
        assert!(started.elapsed() >= Duration::from_millis(250));

        // The old process is still exiting when the new one starts.
        let leaving = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(stays);
        });
        let taken = InstanceLock::acquire_within(&path, Duration::from_secs(10));
        leaving.join().unwrap();
        assert!(taken.is_ok(), "{taken:?}");
    }

    /// Set in the child process of the test below, to the lock it holds.
    const HOLD: &str = "MIMIC_TEST_HOLD_LOCK";

    /// A process that dies holding the lock — killed, as a crash would end it
    /// — lets go of it: a Mimic that died never locks the next one out. The
    /// holder is a real second process: this test binary, run again for this
    /// test alone.
    #[test]
    fn a_holder_that_is_killed_lets_go() {
        if let Some(path) = std::env::var_os(HOLD) {
            let _held = InstanceLock::acquire_within(Path::new(&path), Duration::from_secs(30))
                .expect("the child takes the lock");
            std::thread::sleep(Duration::from_secs(60));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mimic.lock");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "instance::tests::a_holder_that_is_killed_lets_go", "--nocapture"])
            .env(HOLD, &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            match InstanceLock::acquire(&path) {
                Err(LockError::AlreadyRunning(_)) => break,
                Ok(not_yet) => drop(not_yet),
                Err(LockError::Io { .. }) => {}
            }
            if started.elapsed() > Duration::from_secs(30) {
                let _ = child.kill();
                panic!("the child never took the lock");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let taken = InstanceLock::acquire_within(&path, Duration::from_secs(10));
        assert!(taken.is_ok(), "the system let go of a killed process's lock: {taken:?}");
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_not_a_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("mimic.lock");
        assert!(matches!(InstanceLock::acquire(&path), Err(LockError::Io { .. })));
    }
}
