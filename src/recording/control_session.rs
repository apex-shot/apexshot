use std::sync::{Arc, Mutex};

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
struct RecordingControlIface {
    session_id: String,
    command_tx: Arc<Mutex<Option<mpsc::UnboundedSender<RecordingControlCommand>>>>,
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
    task: JoinHandle<()>,
}

impl RecordingControlServer {
    pub async fn start(session_id: String) -> anyhow::Result<Self> {
        let bus_name = format!("org.apexshot.RecordingControl.p{}", std::process::id());
        let command_tx = Arc::new(Mutex::new(None));
        let iface = RecordingControlIface {
            session_id: session_id.clone(),
            command_tx: command_tx.clone(),
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
    }

    pub fn clear_command_sender(&self) {
        if let Ok(mut guard) = self.command_tx.lock() {
            *guard = None;
        }
    }
}

impl Drop for RecordingControlServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::RecordingControlCommand;

    #[test]
    fn session_ending_commands_are_limited_to_stop_and_discard() {
        assert!(RecordingControlCommand::StopSave.ends_session());
        assert!(RecordingControlCommand::StopDiscard.ends_session());
        assert!(!RecordingControlCommand::Pause.ends_session());
        assert!(!RecordingControlCommand::Resume.ends_session());
        assert!(!RecordingControlCommand::Restart.ends_session());
    }
}
