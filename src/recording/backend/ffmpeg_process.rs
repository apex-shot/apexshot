use super::super::{
    gst_audio::ActiveGstAudio, RecordError, RecordResult, RecordingControlCommand,
    RecordingTerminalAction,
};
use gstreamer::glib;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Give the ffmpeg child `read_fd` as `target_fd` (`-i pipe:<target_fd>`).
///
/// The `OwnedFd` is moved into the pre_exec closure so the parent keeps the
/// read end open until after `spawn()` forks; the parent's copy then closes
/// with the closure, leaving ffmpeg's `target_fd` as the only reader (EOF
/// propagates when the writer thread exits).
///
/// Callers attach one pipe per stream: fd 3 carries the audio pipe and the GPU
/// ripple warp, fd 4 carries the streamed cursor track.
#[cfg(unix)]
pub(in crate::recording) fn attach_pipe_as_fd(
    cmd: &mut std::process::Command,
    target_fd: i32,
    read_fd: std::os::fd::OwnedFd,
) {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(move || {
            let fd = read_fd.as_raw_fd();
            if fd != target_fd && libc::dup2(fd, target_fd) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // dup2 clears CLOEXEC on its target, but dup2(x, x) is a no-op —
            // clear it explicitly so the fd survives exec.
            if libc::fcntl(target_fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Drop the original read end so ffmpeg does not keep a second copy
            // of the pipe. Extra copies (especially a leftover write end) stop
            // EOF from ever arriving, so ffmpeg hangs after recording stops.
            if fd != target_fd && libc::close(fd) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Give the ffmpeg child the audio pipe as fd 3 (`-i pipe:3`).
#[cfg(unix)]
pub(in crate::recording) fn attach_audio_pipe_as_fd3(
    cmd: &mut std::process::Command,
    read_fd: std::os::fd::OwnedFd,
) {
    attach_pipe_as_fd(cmd, 3, read_fd);
}

#[cfg(not(unix))]
pub(in crate::recording) fn attach_pipe_as_fd(
    _cmd: &mut std::process::Command,
    _target_fd: i32,
    read_fd: std::os::fd::OwnedFd,
) {
    drop(read_fd);
}

#[cfg(not(unix))]
pub(in crate::recording) fn attach_audio_pipe_as_fd3(
    cmd: &mut std::process::Command,
    read_fd: std::os::fd::OwnedFd,
) {
    attach_pipe_as_fd(cmd, 3, read_fd);
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
    // 8–33MB frames need fewer write() round-trips: grow the pipe to 1MB.
    // Best-effort — the kernel caps at /proc/sys/fs/pipe-max-size.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::fcntl(fd, libc::F_SETPIPE_SZ, 1024 * 1024);
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_child_stdin_nonblocking(
    _stdin: &std::process::ChildStdin,
) -> std::io::Result<()> {
    Ok(())
}

pub(super) struct FfmpegFrameControl<'a> {
    pub command_rx: &'a mut Option<mpsc::UnboundedReceiver<RecordingControlCommand>>,
    pub stop_action: &'a mut RecordingTerminalAction,
    pub paused: &'a mut bool,
    pub active_audio: &'a mut Option<ActiveGstAudio>,
    pub timeline: &'a mut super::super::timeline::RecordingTimeline,
    pub notify: fn(&str),
}

pub(super) fn write_ffmpeg_frame_interruptible(
    stdin: &mut std::process::ChildStdin,
    chunks: &[&[u8]],
    controls: &mut FfmpegFrameControl<'_>,
) -> std::io::Result<bool> {
    use std::io::Write;

    let mut stop_after_frame = false;
    for chunk in chunks {
        let mut offset = 0;
        while offset < chunk.len() {
            match stdin.write(&chunk[offset..]) {
                Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                Ok(written) => offset += written,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(rx) = controls.command_rx {
                        match rx.try_recv() {
                            Ok(RecordingControlCommand::Restart) => {
                                *controls.stop_action = RecordingTerminalAction::Restart;
                                return Ok(false);
                            }
                            Ok(RecordingControlCommand::StopSave) => {
                                stop_after_frame = true;
                            }
                            Ok(RecordingControlCommand::StopDiscard) => {
                                *controls.stop_action = RecordingTerminalAction::Discard;
                                return Ok(false);
                            }
                            Ok(RecordingControlCommand::Pause) if !*controls.paused => {
                                controls.timeline.pause(glib::monotonic_time());
                                if let Some(audio) = controls.active_audio.as_mut() {
                                    audio.set_paused(true);
                                }
                                *controls.paused = true;
                                (controls.notify)("recording_session_paused");
                            }
                            Ok(RecordingControlCommand::Resume) if *controls.paused => {
                                controls.timeline.resume(glib::monotonic_time());
                                if let Some(audio) = controls.active_audio.as_mut() {
                                    audio.set_paused(false);
                                }
                                *controls.paused = false;
                                (controls.notify)("recording_session_resumed");
                            }
                            Ok(_) | Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                                *controls.command_rx = None;
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(err) => return Err(err),
            }
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn blocked_frame_writer_keeps_header_and_pixels_complete_through_pause_resume_and_save() {
        let output = std::env::temp_dir().join(format!(
            "apexshot-blocked-frame-save-{}",
            std::process::id()
        ));
        let file = std::fs::File::create(&output).unwrap();
        let mut child = Command::new("sh")
            .args(["-c", "sleep 0.2; cat"])
            .stdin(Stdio::piped())
            .stdout(file)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        set_child_stdin_nonblocking(&input).unwrap();
        let filler = vec![7u8; 2 * 1024 * 1024];
        let filled = input.write(&filler).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        for command in [
            RecordingControlCommand::Pause,
            RecordingControlCommand::Resume,
            RecordingControlCommand::StopSave,
        ] {
            tx.send(command).unwrap();
        }
        let mut receiver = Some(rx);
        let mut action = RecordingTerminalAction::Save;
        let mut paused = false;
        let mut audio = None;
        let mut timeline = super::super::super::timeline::RecordingTimeline::default();
        let origin = glib::monotonic_time();
        timeline.frame_time_us(origin);
        let header = [0x81u8; 64];
        let pixels = vec![0x55u8; 2 * 1024 * 1024];
        let keep_recording = write_ffmpeg_frame_interruptible(
            &mut input,
            &[&header, &pixels],
            &mut FfmpegFrameControl {
                command_rx: &mut receiver,
                stop_action: &mut action,
                paused: &mut paused,
                active_audio: &mut audio,
                timeline: &mut timeline,
                notify: |_| {},
            },
        )
        .unwrap();
        let end = glib::monotonic_time();
        assert!(!keep_recording);
        assert!(!paused);
        assert_eq!(action, RecordingTerminalAction::Save);
        assert!(timeline.frame_time_us(end) < (end - origin) as u64);
        drop(input);
        assert!(child.wait().unwrap().success());
        let expected = [&filler[..filled], &header, &pixels].concat();
        assert_eq!(std::fs::read(&output).unwrap(), expected);
        std::fs::remove_file(output).unwrap();
    }

    #[test]
    fn blocked_frame_writer_cancels_restart_and_discard_without_finishing_the_payload() {
        for (command, expected_action) in [
            (
                RecordingControlCommand::Restart,
                RecordingTerminalAction::Restart,
            ),
            (
                RecordingControlCommand::StopDiscard,
                RecordingTerminalAction::Discard,
            ),
        ] {
            let mut child = Command::new("sh")
                .args(["-c", "sleep 0.2; cat >/dev/null"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            let mut input = child.stdin.take().unwrap();
            set_child_stdin_nonblocking(&input).unwrap();
            let (tx, rx) = mpsc::unbounded_channel();
            tx.send(command).unwrap();
            let mut receiver = Some(rx);
            let mut action = RecordingTerminalAction::Save;
            let mut paused = false;
            let mut audio = None;
            let mut timeline = super::super::super::timeline::RecordingTimeline::default();
            let pixels = vec![0x55u8; 2 * 1024 * 1024];
            let keep_recording = write_ffmpeg_frame_interruptible(
                &mut input,
                &[&[0x81; 64], &pixels],
                &mut FfmpegFrameControl {
                    command_rx: &mut receiver,
                    stop_action: &mut action,
                    paused: &mut paused,
                    active_audio: &mut audio,
                    timeline: &mut timeline,
                    notify: |_| {},
                },
            )
            .unwrap();
            assert!(!keep_recording);
            assert_eq!(action, expected_action);
            assert!(receiver.as_mut().unwrap().try_recv().is_err());
            drop(input);
            assert!(child.wait().unwrap().success());
        }
    }
}
