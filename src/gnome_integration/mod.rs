use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

static PREVIEW_XID: AtomicU32 = AtomicU32::new(0);

pub fn emit_preview_opened(xid: u32) {
    PREVIEW_XID.store(xid, Ordering::SeqCst);

    std::thread::spawn(move || {
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.apexshot.Preview",
                "/org/apexshot/Preview",
                "org.apexshot.Preview.PreviewOpened",
                &format!("uint32:{}", xid),
            ])
            .spawn();
    });
}

pub fn emit_preview_closed() {
    let xid = PREVIEW_XID.swap(0, Ordering::SeqCst);
    if xid == 0 {
        return;
    }

    std::thread::spawn(move || {
        let _ = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.apexshot.Preview",
                "/org/apexshot/Preview",
                "org.apexshot.Preview.PreviewClosed",
                &format!("uint32:{}", xid),
            ])
            .spawn();
    });
}

pub fn get_current_xid() -> u32 {
    PREVIEW_XID.load(Ordering::SeqCst)
}
