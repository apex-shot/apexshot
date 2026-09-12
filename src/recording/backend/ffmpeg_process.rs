use super::super::{
    gst_audio::ActiveGstAudio, notify_daemon_event, RecordError, RecordResult,
    RecordingControlCommand, RecordingTerminalAction,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Give the ffmpeg child the audio pipe as fd 3 (`-i pipe:3`).
///
/// The `OwnedFd` is moved into the pre_exec closure so the parent keeps the
/// read end open until after `spawn()` forks; the parent's copy then closes
/// with the closure, leaving ffmpeg's fd 3 as the only reader (EOF propagates
/// when the GStreamer writer thread exits).
#[cfg(unix)]
pub(in crate::recording) fn attach_audio_pipe_as_fd3(
    cmd: &mut std::process::Command,
    read_fd: std::os::fd::OwnedFd,
) {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(move || {
            let fd = read_fd.as_raw_fd();
            if fd != 3 && libc::dup2(fd, 3) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // dup2 clears CLOEXEC on its target, but dup2(x, x) is a no-op —
            // clear it explicitly so fd 3 survives exec.
            if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Drop the original read end so ffmpeg does not keep a second copy
            // of the pipe. Extra copies (especially a leftover write end) stop
            // EOF from ever arriving, so ffmpeg hangs after recording stops.
            if fd != 3 && libc::close(fd) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub(super) fn attach_audio_pipe_as_fd3(
    _cmd: &mut std::process::Command,
    read_fd: std::os::fd::OwnedFd,
) {
    drop(read_fd);
}

pub(super) fn ffmpeg_error_detail(stderr: &str) -> String {
    let lines: Vec<_> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return " Check that FFmpeg supports the selected encoder and audio devices.".into();
    }

    let start = lines.len().saturating_sub(3);
    let mut detail = lines[start..].join(" ");
    if detail.len() > 600 {
        detail.truncate(600);
        detail.push_str("...");
    }
    format!(" FFmpeg reported: {detail}")
}

pub(super) fn wait_for_ffmpeg_child(
    child: &mut std::process::Child,
) -> RecordResult<std::process::ExitStatus> {
    if let Some(status) = wait_for_ffmpeg_exit(child, Duration::from_secs(20))? {
        return Ok(status);
    }

    eprintln!("[recording] ffmpeg did not exit after stdin close; interrupting");
    interrupt_child(child);
    if let Some(status) = wait_for_ffmpeg_exit(child, Duration::from_secs(5))? {
        return Ok(status);
    }

    let _ = child.kill();
    child
        .wait()
        .map_err(|e| RecordError::GStreamerError(format!("Failed to wait for ffmpeg: {e}")))
}

#[cfg(unix)]
pub(super) fn set_child_stdin_nonblocking(stdin: &std::process::ChildStdin) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let fd = stdin.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_child_stdin_nonblocking(
    _stdin: &std::process::ChildStdin,
) -> std::io::Result<()> {
    Ok(())
}

pub(super) fn write_ffmpeg_frame_interruptible(
    stdin: &mut std::process::ChildStdin,
    pixels: &[u8],
    command_rx: &mut Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
    stop_action: &mut RecordingTerminalAction,
    paused: &mut bool,
    active_audio: &mut Option<ActiveGstAudio>,
) -> std::io::Result<bool> {
    use std::io::Write;

    let mut offset = 0;
    let mut stop_after_frame = false;
    while offset < pixels.len() {
        match stdin.write(&pixels[offset..]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(written) => offset += written,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(rx) = command_rx {
                    match rx.try_recv() {
                        Ok(RecordingControlCommand::Restart) => {
                            *stop_action = RecordingTerminalAction::Restart;
                            return Ok(false);
                        }
                        Ok(RecordingControlCommand::StopSave) => {
                            stop_after_frame = true;
                        }
                        Ok(RecordingControlCommand::StopDiscard) => {
                            *stop_action = RecordingTerminalAction::Discard;
                            return Ok(false);
                        }
                        Ok(RecordingControlCommand::Pause) if !*paused => {
                            if let Some(audio) = active_audio.as_mut() {
                                audio.set_paused(true);
                            }
                            *paused = true;
                            notify_daemon_event("recording_session_paused");
                        }
                        Ok(RecordingControlCommand::Resume) if *paused => {
                            if let Some(audio) = active_audio.as_mut() {
                                audio.set_paused(false);
                            }
                            *paused = false;
                            notify_daemon_event("recording_session_resumed");
                        }
                        Ok(_) | Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                            *command_rx = None;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err),
        }
    }
    Ok(!stop_after_frame)
}

fn wait_for_ffmpeg_exit(
    child: &mut std::process::Child,
    timeout: Duration,
) -> RecordResult<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(RecordError::GStreamerError(format!(
                    "Failed to wait for ffmpeg: {e}"
                )));
            }
        }
    }
}

fn interrupt_child(child: &std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        if pid > 0 {
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = child;
}
