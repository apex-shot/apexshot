use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) const RESERVE_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(super) fn reserve_is_low(free: Option<u64>) -> bool {
    free.is_some_and(|free| free < RESERVE_BYTES)
}

pub(super) fn check_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|last| now.duration_since(last) >= POLL_INTERVAL)
}

fn existing_ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|candidate| candidate.exists())
}

#[allow(clippy::unnecessary_cast)]
fn free_bytes(path: &Path) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let probe = existing_ancestor(path)?;
        let raw = std::ffi::CString::new(probe.as_os_str().as_bytes()).ok()?;
        let mut buf: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(raw.as_ptr(), &mut buf) } != 0 {
            return None;
        }
        let block = if buf.f_frsize != 0 {
            buf.f_frsize as u64
        } else {
            buf.f_bsize as u64
        };
        Some((buf.f_bavail as u64).saturating_mul(block))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

pub(super) struct DiskSpaceGuard {
    path: PathBuf,
    probe: Box<dyn FnMut(&Path) -> Option<u64> + Send>,
    last_poll: Option<Instant>,
    tripped: bool,
}

impl DiskSpaceGuard {
    pub(super) fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            probe: Box::new(free_bytes),
            last_poll: None,
            tripped: false,
        }
    }

    #[cfg(test)]
    pub(super) fn with_probe(
        path: &Path,
        probe: impl FnMut(&Path) -> Option<u64> + Send + 'static,
    ) -> Self {
        Self {
            path: path.to_path_buf(),
            probe: Box::new(probe),
            last_poll: None,
            tripped: false,
        }
    }

    fn probe_low(&mut self) -> bool {
        if reserve_is_low((self.probe)(&self.path)) {
            self.tripped = true;
        }
        self.tripped
    }

    pub(super) fn refuse_start(&mut self) -> bool {
        self.probe_low()
    }

    pub(super) fn low(&mut self) -> bool {
        self.probe_low()
    }

    fn poll_at(&mut self, now: Instant) -> Option<bool> {
        if self.tripped {
            return Some(true);
        }
        if !check_due(self.last_poll, now) {
            return None;
        }
        self.last_poll = Some(now);
        Some(self.probe_low())
    }

    pub(super) fn poll(&mut self) -> Option<bool> {
        self.poll_at(Instant::now())
    }

    pub(super) fn tripped(&self) -> bool {
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn the_reserve_boundary_refuses_only_below_it() {
        assert!(reserve_is_low(Some(RESERVE_BYTES - 1)));
        assert!(!reserve_is_low(Some(RESERVE_BYTES)));
        assert!(!reserve_is_low(Some(RESERVE_BYTES + 1)));
        assert!(!reserve_is_low(Some(u64::MAX)));
    }

    #[test]
    fn unknown_free_space_never_trips() {
        assert!(!reserve_is_low(None));
    }

    #[test]
    fn checks_follow_the_two_second_cadence() {
        let start = Instant::now();
        assert!(check_due(None, start));
        assert!(!check_due(Some(start), start + Duration::from_millis(500)));
        assert!(check_due(Some(start), start + POLL_INTERVAL));
        assert!(check_due(Some(start), start + POLL_INTERVAL * 2));
    }

    #[test]
    fn injected_free_space_drives_poll_low_and_latch() {
        let free = Arc::new(Mutex::new(Some(RESERVE_BYTES + 1)));
        let probe_free = free.clone();
        let mut guard =
            DiskSpaceGuard::with_probe(Path::new("/out.mp4"), move |_| *probe_free.lock().unwrap());
        let start = Instant::now();

        assert!(!guard.refuse_start());
        assert_eq!(guard.poll_at(start), Some(false));
        assert_eq!(guard.poll_at(start + Duration::from_millis(500)), None);

        *free.lock().unwrap() = Some(RESERVE_BYTES - 1);
        assert_eq!(guard.poll_at(start + Duration::from_millis(500)), None);
        assert_eq!(guard.poll_at(start + POLL_INTERVAL), Some(true));
        assert!(guard.tripped());
        assert_eq!(guard.poll_at(start + POLL_INTERVAL * 2), Some(true));

        *free.lock().unwrap() = Some(u64::MAX);
        assert!(guard.low());
        assert!(guard.tripped());
    }

    #[test]
    fn a_missing_probe_answer_never_stops_a_recording() {
        let mut guard = DiskSpaceGuard::with_probe(Path::new("/out.mp4"), |_| None);
        assert!(!guard.refuse_start());
        assert_eq!(guard.poll_at(Instant::now()), Some(false));
        assert!(!guard.low());
        assert!(!guard.tripped());
    }
}
