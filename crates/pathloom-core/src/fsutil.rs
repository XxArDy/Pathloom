use crate::error::Result;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::time::Duration;

/// Write `bytes` to `path` atomically (temp file in the same dir, fsync, rename).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(bytes)?;
    let hash = crate::hash::hash_bytes(bytes);
    tmp.as_file().sync_all()?;
    let mut attempt = 0u32;
    loop {
        match tmp.persist(path) {
            Ok(_) => {
                crate::writes::record(path, &hash);
                return Ok(());
            }
            // Windows: target briefly locked by AV/indexer/reader.
            Err(e) if e.error.kind() == ErrorKind::PermissionDenied && attempt < 4 => {
                attempt += 1;
                tmp = e.file;
                std::thread::sleep(Duration::from_millis(20 * u64::from(attempt)));
            }
            Err(e) => return Err(e.error.into()),
        }
    }
}

/// Current UTC time as unix seconds (0 when the clock is before 1970).
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// Current UTC time as RFC3339 `YYYY-MM-DDTHH:MM:SSZ` (log entries, claims).
pub fn now_rfc3339() -> String {
    format_secs(now_secs())
}

/// Current UTC time as `YYYY-MM-DDTHH:MMZ` (the comment timestamp format).
pub(crate) fn now_minute() -> String {
    format_minute(now_secs())
}

/// Unix seconds to `YYYY-MM-DDTHH:MMZ`.
pub(crate) fn format_minute(secs: i64) -> String {
    format!("{}Z", &format_secs(secs)[..16])
}

/// Unix seconds to RFC3339 UTC `YYYY-MM-DDTHH:MM:SSZ` (date part: [`crate::agenda::days_to_date`]).
/// Fixed width, so the strings compare in time order.
pub(crate) fn format_secs(secs: i64) -> String {
    let rem = secs.rem_euclid(86_400);
    let date = crate::agenda::days_to_date(secs.div_euclid(86_400));
    format!("{date}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Public atomic write (temp file, fsync, rename) that also records the write in [`crate::writes`].
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic(path, bytes).map_err(|e| match e {
        crate::error::Error::Io(io) => io,
        other => std::io::Error::other(other.to_string()),
    })
}


/// Cross-process lock: an OS file lock (`File::try_lock`) on `path`, released when dropped (or when the
/// process dies, so there is no stale-lock guessing). Waits up to the given time, then fails with `TimedOut`.
/// The lock file itself stays on disk (it is git-ignored); deleting it would race with the next locker.
pub(crate) struct FileLock {
    _file: std::fs::File,
}

impl FileLock {
    pub(crate) fn acquire(path: &Path) -> Result<FileLock> {
        Self::acquire_for(path, Duration::from_secs(2))
    }

    pub(crate) fn acquire_for(path: &Path, wait: Duration) -> Result<FileLock> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        let deadline = std::time::Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(FileLock { _file: file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(ErrorKind::TimedOut, format!("lock {} is busy", path.display())).into());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}

thread_local! {
    /// Lock files this thread holds (for re-entrant locking).
    static HELD: std::cell::RefCell<Vec<std::path::PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// [`FileLock`] that the same thread can take again (nested calls) without waiting for itself.
/// Not for use across `.await`: the lock is bound to the thread.
pub(crate) struct ReentrantLock {
    path: std::path::PathBuf,
    _file: Option<FileLock>,
}

impl ReentrantLock {
    pub(crate) fn acquire(path: &Path, wait: Duration) -> Result<ReentrantLock> {
        let held = HELD.with(|h| h.borrow().iter().any(|p| p == path));
        let file = if held { None } else { Some(FileLock::acquire_for(path, wait)?) };
        HELD.with(|h| h.borrow_mut().push(path.to_path_buf()));
        Ok(ReentrantLock { path: path.to_path_buf(), _file: file })
    }
}

impl Drop for ReentrantLock {
    fn drop(&mut self) {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(i) = h.iter().rposition(|p| *p == self.path) {
                h.remove(i);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_lock_excludes_and_times_out() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("x.lock");
        let held = super::FileLock::acquire(&path).unwrap();
        let busy = super::FileLock::acquire_for(&path, std::time::Duration::from_millis(30));
        assert!(matches!(busy, Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::TimedOut));
        drop(held);
        assert!(super::FileLock::acquire_for(&path, std::time::Duration::from_millis(30)).is_ok());
    }

    #[test]
    fn minute_format() {
        assert_eq!(super::format_minute(0), "1970-01-01T00:00Z");
        assert_eq!(super::format_minute(1_793_788_920), "2026-11-04T10:42Z");
        assert_eq!(super::format_secs(1_793_788_925), "2026-11-04T10:42:05Z");
    }
}
