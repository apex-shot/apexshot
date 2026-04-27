use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};

use anyhow::Context;
use tokio::{sync::mpsc, task::JoinHandle};

pub const RECORDING_CONTROL_OBJECT_PATH: &str = "/org/apexshot/RecordingControl";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingControlCommand {
    Pause,
    Resume,
    Restart,
    StopSave,
    StopDiscard,
}

impl RecordingControlCommand {
    pub fn ends_session(self) -> bool {
        matches!(self, Self::StopSave | Self::StopDiscard)
    }
}

#[derive(Clone)]
struct ActiveRecordingControl {
    session_id: String,
    command_tx: Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
    paused: Arc<AtomicBool>,
}

fn active_recording_control() -> &'static Mutex<Option<ActiveRecordingControl>> {
    static ACTIVE_RECORDING_CONTROL: OnceLock<Mutex<Option<ActiveRecordingControl>>> =
        OnceLock::new();
    ACTIVE_RECORDING_CONTROL.get_or_init(|| Mutex::new(None))
}

pub fn has_active_recording_control() -> bool {
    active_recording_control()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
        .is_some()
}

fn notify_shell_overlay(command: RecordingControlCommand, session_id: &str) {
    let result = match command {
        RecordingControlCommand::Pause => {
            crate::gnome_shell::set_recording_paused(session_id, true)
        }
        RecordingControlCommand::Resume => {
            crate::gnome_shell::set_recording_paused(session_id, false)
        }
        RecordingControlCommand::Restart => crate::gnome_shell::restart_recording_ui(session_id),
        RecordingControlCommand::StopSave | RecordingControlCommand::StopDiscard => {
            crate::gnome_shell::end_recording_ui(session_id)
        }
    };
    let _ = result;
}

fn apply_command_side_effects(
    command: RecordingControlCommand,
    paused: &AtomicBool,
    session_id: &str,
) {
    match command {
        RecordingControlCommand::Pause => paused.store(true, Ordering::Relaxed),
        RecordingControlCommand::Resume
        | RecordingControlCommand::Restart
        | RecordingControlCommand::StopSave
        | RecordingControlCommand::StopDiscard => paused.store(false, Ordering::Relaxed),
    }

    notify_shell_overlay(command, session_id);
}

fn register_active_recording_control(
    session_id: String,
    command_tx: Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
    paused: Arc<AtomicBool>,
) {
    if let Ok(mut guard) = active_recording_control().lock() {
        *guard = Some(ActiveRecordingControl {
            session_id,
            command_tx,
            paused,
        });
    }
}

fn clear_active_recording_control(
    command_tx: &Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
) {
    if let Ok(mut guard) = active_recording_control().lock() {
        if guard
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(&active.command_tx, command_tx))
        {
            *guard = None;
        }
    }
}

pub fn send_active_recording_command(command: RecordingControlCommand) -> bool {
    let active = active_recording_control()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let Some(active) = active else {
        return false;
    };

    let tx = active
        .command_tx
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let Some(tx) = tx else {
        return false;
    };

    if tx.send(command).is_err() {
        return false;
    }

    apply_command_side_effects(command, &active.paused, &active.session_id);
    true
}

pub fn toggle_active_recording_pause() -> bool {
    let active = active_recording_control()
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    let Some(active) = active else {
        return false;
    };

    let command = if active.paused.load(Ordering::Relaxed) {
        RecordingControlCommand::Resume
    } else {
        RecordingControlCommand::Pause
    };
    send_active_recording_command(command)
}

#[derive(Clone)]
struct RecordingControlIface {
    session_id: String,
    command_tx: Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
    paused: Arc<AtomicBool>,
}

impl RecordingControlIface {
    fn send(&self, session_id: &str, command: RecordingControlCommand) -> zbus::fdo::Result<bool> {
        if session_id != self.session_id {
            return Ok(false);
        }

        let Some(tx) = self.command_tx.lock().ok().and_then(|guard| guard.clone()) else {
            return Ok(false);
        };

        tx.send(command).map_err(|err| {
            zbus::fdo::Error::Failed(format!("recording command channel unavailable: {err}"))
        })?;
        apply_command_side_effects(command, &self.paused, &self.session_id);
        Ok(true)
    }
}

#[zbus::interface(name = "org.apexshot.RecordingControl")]
impl RecordingControlIface {
    async fn stop(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.send(session_id, RecordingControlCommand::StopSave)
    }

    async fn discard(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.send(session_id, RecordingControlCommand::StopDiscard)
    }

    async fn pause(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.send(session_id, RecordingControlCommand::Pause)
    }

    async fn resume(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.send(session_id, RecordingControlCommand::Resume)
    }

    async fn restart(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.send(session_id, RecordingControlCommand::Restart)
    }
}

pub struct RecordingControlServer {
    bus_name: String,
    session_id: String,
    command_tx: Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
    paused: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

impl RecordingControlServer {
    pub async fn start(session_id: String) -> anyhow::Result<Self> {
        let bus_name = format!("org.apexshot.RecordingControl.p{}", std::process::id());
        let command_tx = Arc::new(Mutex::new(None));
        let paused = Arc::new(AtomicBool::new(false));
        let iface = RecordingControlIface {
            session_id: session_id.clone(),
            command_tx: command_tx.clone(),
            paused: paused.clone(),
        };
        let bus_name_for_task = bus_name.clone();
        let task = tokio::spawn(async move {
            let result = async {
                let builder = zbus::connection::Builder::session()
                    .context("failed to open session bus for recording controls")?;
                let _conn = builder
                    .name(bus_name_for_task.as_str())?
                    .serve_at(RECORDING_CONTROL_OBJECT_PATH, iface)?
                    .build()
                    .await
                    .context("failed to build recording control D-Bus service")?;
                futures_util::future::pending::<()>().await;
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            }
            .await;

            if let Err(err) = result {
                eprintln!("[recording] Recording control service failed: {err}");
            }
        });

        Ok(Self {
            bus_name,
            session_id,
            command_tx,
            paused,
            task,
        })
    }

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn set_command_sender(&self, tx: mpsc::UnboundedSender<RecordingControlCommand>) {
        if let Ok(mut guard) = self.command_tx.lock() {
            *guard = Some(tx);
        }
        self.paused.store(false, Ordering::Relaxed);
        register_active_recording_control(
            self.session_id.clone(),
            self.command_tx.clone(),
            self.paused.clone(),
        );
    }

    pub fn clear_command_sender(&self) {
        if let Ok(mut guard) = self.command_tx.lock() {
            *guard = None;
        }
        self.paused.store(false, Ordering::Relaxed);
        clear_active_recording_control(&self.command_tx);
    }

}

impl Drop for RecordingControlServer {
    fn drop(&mut self) {
        clear_active_recording_control(&self.command_tx);
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        has_active_recording_control, send_active_recording_command, RecordingControlCommand,
        RecordingControlServer,
    };
    use tokio::sync::mpsc;

    #[test]
    fn session_ending_commands_are_limited_to_stop_and_discard() {
        assert!(RecordingControlCommand::StopSave.ends_session());
        assert!(RecordingControlCommand::StopDiscard.ends_session());
        assert!(!RecordingControlCommand::Pause.ends_session());
        assert!(!RecordingControlCommand::Resume.ends_session());
        assert!(!RecordingControlCommand::Restart.ends_session());
    }

    #[tokio::test]
    async fn active_recording_command_is_forwarded_to_registered_session() {
        let server = RecordingControlServer::start("recording-test".into())
            .await
            .expect("control server should start");
        let (tx, mut rx) = mpsc::unbounded_channel();
        server.set_command_sender(tx);

        assert!(has_active_recording_control());
        assert!(send_active_recording_command(
            RecordingControlCommand::Pause
        ));
        assert_eq!(rx.recv().await, Some(RecordingControlCommand::Pause));

        server.clear_command_sender();
        assert!(!has_active_recording_control());
        drop(server);
    }
}
