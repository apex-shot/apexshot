//! The one promo-window slot, shared by every window that advertises
//! something: the Black Friday card, the Cloud copy invitation, the AI names
//! cards, and the What's new window.
//!
//! Those windows must never be on screen together. Each one claims the slot
//! before it builds its window and holds the returned guard until it closes; a
//! window that cannot claim exits without opening. The claim is a file so it
//! spans processes, because every one of these windows runs in its own.
//!
//! The file holds the claiming pid and the time of the claim. A lock whose
//! process is gone, or that is older than ten minutes, is taken over, so a
//! crashed card never blocks the slot forever.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The day's Black Friday offer card.
pub const KIND_BLACK_FRIDAY: &str = "black_friday";
/// The "Copied. Want a link instead?" invitation after a copied capture.
pub const KIND_CLOUD_INVITE: &str = "cloud_invite";
/// Either AI names card: the trial ran out, or the feature is worth a try.
pub const KIND_AI_NAMES: &str = "ai_names";
/// The release-highlights window.
pub const KIND_WHATS_NEW: &str = "whats_new";

/// A claim older than this is taken over even when its process still answers:
/// ten minutes is longer than any of these cards can stay up.
const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

const LOCK_FILE: &str = "promo-window.lock";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LockFile {
    pid: u32,
    kind: String,
    since: u64,
}

/// An open promo window's claim on the slot. The lock is removed when the
/// guard drops, but only while the file still records this claim.
pub struct PromoSlotGuard {
    path: PathBuf,
    pid: u32,
    kind: String,
}

impl Drop for PromoSlotGuard {
    fn drop(&mut self) {
        let Some(lock) = read_lock(&self.path) else {
            return;
        };
        if lock.pid == self.pid && lock.kind == self.kind {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Claim the slot for a window of `kind`; `None` when another promo window
/// holds it. A stale claim is taken over.
pub fn try_claim(kind: &str) -> Option<PromoSlotGuard> {
    try_claim_in(&slot_dir(), kind)
}

/// True when no promo window holds the slot. Triggers check this before they
/// spend a one-off allowance, then the window itself claims the slot.
pub fn is_free() -> bool {
    is_free_in(&slot_dir())
}

/// The directory-aware claim, split out so the tests can use a temp dir.
pub fn try_claim_in(dir: &Path, kind: &str) -> Option<PromoSlotGuard> {
    if fs::create_dir_all(dir).is_err() {
        return None;
    }
    let path = dir.join(LOCK_FILE);
    if create_lock(&path, kind) {
        return Some(guard(path, kind));
    }

    let stale = match read_lock(&path) {
        Some(lock) => is_stale(&lock),
        // An unreadable or truncated lock cannot name a live window.
        None => true,
    };
    if !stale {
        return None;
    }

    // Take the abandoned claim over. If another process creates the lock in
    // between, this retry loses and the caller stays out.
    let _ = fs::remove_file(&path);
    if !create_lock(&path, kind) {
        return None;
    }
    Some(guard(path, kind))
}

/// The directory-aware free check, split out for the tests.
pub fn is_free_in(dir: &Path) -> bool {
    match read_lock(&dir.join(LOCK_FILE)) {
        Some(lock) => is_stale(&lock),
        None => true,
    }
}

fn guard(path: PathBuf, kind: &str) -> PromoSlotGuard {
    PromoSlotGuard {
        path,
        pid: process::id(),
        kind: kind.to_string(),
    }
}

/// Where the lock lives. Both a card process and the trigger that starts it
/// must resolve the same path, so it stays per-user and outside any package.
fn slot_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::runtime_dir()
            .or_else(|| dirs::cache_dir().map(|dir| dir.join("apexshot")))
            .unwrap_or_else(|| std::env::temp_dir().join("apexshot"))
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .map(|dir| dir.join("apexshot"))
            .unwrap_or_else(|| std::env::temp_dir().join("apexshot"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        std::env::temp_dir().join("apexshot")
    }
}

fn is_stale(lock: &LockFile) -> bool {
    !process_alive(lock.pid) || now_secs().saturating_sub(lock.since) > STALE_AFTER.as_secs()
}

fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };

        if pid == 0 {
            return false;
        }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = unsafe { GetExitCodeProcess(handle, &mut code) };
        let _ = unsafe { CloseHandle(handle) };
        ok != 0 && code == STILL_ACTIVE as u32
    }
    #[cfg(not(target_os = "windows"))]
    {
        pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
    }
}

fn read_lock(path: &Path) -> Option<LockFile> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Create the lock with its contents already complete, so a reader never sees
/// a half-written file. The temporary file is hard-linked into place, which
/// fails when the lock exists, exactly like `create_new` but after the write.
fn create_lock(path: &Path, kind: &str) -> bool {
    let lock = LockFile {
        pid: process::id(),
        kind: kind.to_string(),
        since: now_secs(),
    };
    let Ok(raw) = serde_json::to_vec(&lock) else {
        return false;
    };

    let temp = temp_path(path);
    if !write_new_file(&temp, &raw) {
        let _ = fs::remove_file(&temp);
        return false;
    }
    let created = match fs::hard_link(&temp, path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        // A filesystem without hard links falls back to plain create-new.
        Err(_) => write_new_file(path, &raw),
    };
    let _ = fs::remove_file(&temp);
    created
}

fn write_new_file(path: &Path, raw: &[u8]) -> bool {
    let Ok(mut file) = OpenOptions::new().write(true).create_new(true).open(path) else {
        return false;
    };
    if file.write_all(raw).is_err() {
        let _ = fs::remove_file(path);
        return false;
    }
    let _ = file.sync_all();
    true
}

fn temp_path(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(
        "{LOCK_FILE}.{}.{}.{}.tmp",
        process::id(),
        now_nanos(),
        nonce
    ))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "apexshot-promo-slot-{label}-{}-{}",
            process::id(),
            now_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_lock(dir: &Path, pid: u32, since: u64) {
        let lock = LockFile {
            pid,
            kind: KIND_AI_NAMES.to_string(),
            since,
        };
        fs::write(dir.join(LOCK_FILE), serde_json::to_vec(&lock).unwrap()).unwrap();
    }

    fn lock_path(dir: &Path) -> PathBuf {
        dir.join(LOCK_FILE)
    }

    #[test]
    fn a_claim_holds_the_slot_and_its_release_frees_it() {
        let dir = scratch_dir("claim");
        let guard = try_claim_in(&dir, KIND_BLACK_FRIDAY).unwrap();
        assert!(!is_free_in(&dir));
        assert!(try_claim_in(&dir, KIND_AI_NAMES).is_none());
        drop(guard);
        assert!(is_free_in(&dir));
        assert!(!lock_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_process_does_not_hold_the_slot() {
        let dir = scratch_dir("dead-pid");
        write_lock(&dir, u32::MAX - 1, now_secs());
        assert!(is_free_in(&dir));
        let guard = try_claim_in(&dir, KIND_WHATS_NEW).unwrap();
        assert_eq!(read_lock(&lock_path(&dir)).unwrap().kind, KIND_WHATS_NEW);
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_claim_is_taken_over_even_from_a_live_process() {
        let dir = scratch_dir("old");
        write_lock(&dir, process::id(), now_secs() - 601);
        assert!(is_free_in(&dir));
        let guard = try_claim_in(&dir, KIND_CLOUD_INVITE).unwrap();
        let lock = read_lock(&lock_path(&dir)).unwrap();
        assert_eq!(lock.kind, KIND_CLOUD_INVITE);
        assert_eq!(lock.pid, process::id());
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recent_claim_from_a_live_process_blocks_the_slot() {
        let dir = scratch_dir("live");
        write_lock(&dir, process::id(), now_secs());
        assert!(!is_free_in(&dir));
        assert!(try_claim_in(&dir, KIND_BLACK_FRIDAY).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_guard_leaves_a_lock_that_belongs_to_another_process() {
        let dir = scratch_dir("other");
        let guard = try_claim_in(&dir, KIND_BLACK_FRIDAY).unwrap();
        // Pretend another window took the slot over while this one was open.
        write_lock(&dir, process::id().wrapping_add(1), now_secs());
        drop(guard);
        assert!(lock_path(&dir).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_that_cannot_be_read_is_taken_over() {
        let dir = scratch_dir("garbage");
        fs::write(lock_path(&dir), b"not json").unwrap();
        assert!(is_free_in(&dir));
        let guard = try_claim_in(&dir, KIND_AI_NAMES).unwrap();
        assert_eq!(read_lock(&lock_path(&dir)).unwrap().pid, process::id());
        drop(guard);
        let _ = fs::remove_dir_all(&dir);
    }
}
