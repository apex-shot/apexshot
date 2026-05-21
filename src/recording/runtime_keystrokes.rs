use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::thread;

use anyhow::{anyhow, Context};
use ashpd::desktop::{
    remote_desktop::{DeviceType, RemoteDesktop},
    PersistMode,
};
use futures_util::StreamExt;
use reis::{
    ei,
    event::{DeviceCapability, EiEvent, KeyboardKey, KeyboardModifiers, Keymap},
};
use tokio::sync::oneshot;
use xkbcommon::xkb;

const COMMAND_FILTER_MODE: u8 = 1;
const XKB_KEYCODE_OFFSET: u32 = 8;

pub struct RuntimeKeystrokeForwarder {
    stop_tx: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl RuntimeKeystrokeForwarder {
    pub fn stop(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn spawn_runtime_keystroke_forwarder(
    session_id: String,
    filter_mode: u8,
) -> RuntimeKeystrokeForwarder {
    let (stop_tx, stop_rx) = oneshot::channel();
    let thread = thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(err) => {
                eprintln!("[recording] Failed to create keystroke runtime: {err}");
                return;
            }
        };

        runtime.block_on(async move {
            if let Err(err) =
                run_runtime_keystroke_forwarder(&session_id, filter_mode, stop_rx).await
            {
                eprintln!("[recording] Runtime keystroke capture failed: {err}");
            }
        });
    });

    RuntimeKeystrokeForwarder {
        stop_tx: Some(stop_tx),
        thread: Some(thread),
    }
}

async fn run_runtime_keystroke_forwarder(
    session_id: &str,
    filter_mode: u8,
    mut stop_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let remote = RemoteDesktop::new()
        .await
        .context("RemoteDesktop proxy init failed")?;
    let session = remote
        .create_session()
        .await
        .context("RemoteDesktop create_session failed")?;

    remote
        .select_devices(
            &session,
            DeviceType::Keyboard.into(),
            None,
            PersistMode::DoNot,
        )
        .await
        .context("RemoteDesktop select_devices failed")?;

    remote
        .start(&session, None)
        .await
        .context("RemoteDesktop start request failed")?
        .response()
        .context("RemoteDesktop start denied")?;

    let fd = remote
        .connect_to_eis(&session)
        .await
        .context("RemoteDesktop connect_to_eis failed")?;
    let stream = UnixStream::from(fd);
    stream
        .set_nonblocking(true)
        .context("failed to set EIS socket nonblocking")?;

    let context = ei::Context::new(stream).context("failed to create EIS context")?;
    let (_connection, mut event_stream) = context
        .handshake_tokio(
            "apexshot-recording-keystrokes",
            ei::handshake::ContextType::Receiver,
        )
        .await
        .context("EIS handshake failed")?;

    let mut formatter = KeystrokeFormatter::default();
    eprintln!("[recording] Runtime keystroke capture started.");

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            maybe_event = event_stream.next() => {
                let Some(event) = maybe_event else {
                    break;
                };
                let event = event.context("failed to read EIS event")?;
                match event {
                    EiEvent::SeatAdded(seat_event) => {
                        seat_event
                            .seat
                            .bind_capabilities(DeviceCapability::Keyboard.into());
                        let _ = context.flush();
                    }
                    EiEvent::DeviceAdded(device_event)
                        if device_event
                            .device
                            .has_capability(DeviceCapability::Keyboard) =>
                    {
                        formatter
                            .try_load_keymap(device_event.device.keymap())
                            .context("failed to load keyboard keymap")?;
                    }
                    EiEvent::DeviceAdded(_) => {}
                    EiEvent::KeyboardModifiers(modifier_event) => {
                        formatter.apply_modifiers(&modifier_event);
                    }
                    EiEvent::KeyboardKey(key_event) => {
                        if let Some(text) = formatter.format_key_press(&key_event, filter_mode) {
                            if let Err(err) =
                                crate::gnome_shell::push_recording_keystroke(session_id, &text)
                            {
                                eprintln!("[recording] Failed to push keystroke to shell overlay: {err}");
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = session.close().await;
    eprintln!("[recording] Runtime keystroke capture stopped.");
    Ok(())
}

#[derive(Default)]
struct KeystrokeFormatter {
    keymap: Option<xkb::Keymap>,
    state: Option<xkb::State>,
}

impl KeystrokeFormatter {
    fn try_load_keymap(&mut self, keymap: Option<&Keymap>) -> anyhow::Result<()> {
        let Some(keymap) = keymap else {
            return Ok(());
        };

        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let duplicated_fd = dup_owned_fd(&keymap.fd)?;
        let loaded = unsafe {
            xkb::Keymap::new_from_fd(
                &context,
                duplicated_fd,
                keymap.size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        }
        .context("failed to map xkb keymap fd")?
        .ok_or_else(|| anyhow!("xkbcommon rejected the portal keymap"))?;

        self.state = Some(xkb::State::new(&loaded));
        self.keymap = Some(loaded);
        Ok(())
    }

    fn apply_modifiers(&mut self, event: &KeyboardModifiers) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        state.update_mask(
            event.depressed,
            event.latched,
            event.locked,
            event.group,
            0,
            0,
        );
    }

    fn format_key_press(&self, event: &KeyboardKey, filter_mode: u8) -> Option<String> {
        if event.state != ei::keyboard::KeyState::Press {
            return None;
        }

        let state = self.state.as_ref()?;
        let keycode = xkb::Keycode::new(event.key + XKB_KEYCODE_OFFSET);
        let key_label = normalize_key_label(state, keycode)?;

        let ctrl = state.mod_name_is_active(&xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let alt = state.mod_name_is_active(&xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let shift = state.mod_name_is_active(&xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
        let meta = state.mod_name_is_active(&xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);

        if filter_mode == COMMAND_FILTER_MODE && !(ctrl || alt || meta) {
            return None;
        }

        let mut parts = Vec::with_capacity(5);
        if ctrl {
            parts.push("Ctrl".to_string());
        }
        if alt {
            parts.push("Alt".to_string());
        }
        if shift {
            parts.push("Shift".to_string());
        }
        if meta {
            parts.push("Super".to_string());
        }
        parts.push(key_label);
        Some(parts.join(" + "))
    }
}

fn dup_owned_fd(fd: &OwnedFd) -> anyhow::Result<OwnedFd> {
    let duplicated = unsafe { libc::dup(fd.as_raw_fd()) };
    if duplicated < 0 {
        return Err(std::io::Error::last_os_error()).context("dup keymap fd failed");
    }

    Ok(unsafe { OwnedFd::from_raw_fd(duplicated) })
}

fn normalize_key_label(state: &xkb::State, keycode: xkb::Keycode) -> Option<String> {
    let keysym_name = xkb::keysym_get_name(state.key_get_one_sym(keycode));
    if is_modifier_only_key(&keysym_name) {
        return None;
    }

    let unicode = state.key_get_utf8(keycode);
    if !unicode.trim().is_empty() {
        return Some(unicode.to_uppercase());
    }

    if let Some(label) = special_key_label(&keysym_name) {
        return Some(label.to_string());
    }

    if keysym_name.len() == 1 {
        return Some(keysym_name.to_uppercase());
    }

    if keysym_name.starts_with('F')
        && keysym_name.len() <= 3
        && keysym_name[1..].chars().all(|ch| ch.is_ascii_digit())
    {
        return Some(keysym_name);
    }

    if let Some(rest) = keysym_name.strip_prefix("KP_") {
        return Some(title_case_words(rest));
    }

    if keysym_name.is_empty() {
        None
    } else {
        Some(title_case_words(&keysym_name))
    }
}

fn special_key_label(keysym_name: &str) -> Option<&'static str> {
    match keysym_name {
        "BackSpace" => Some("Backspace"),
        "Delete" => Some("Delete"),
        "Down" => Some("Down"),
        "Escape" => Some("Esc"),
        "ISO_Left_Tab" => Some("Tab"),
        "KP_Enter" => Some("Enter"),
        "Left" => Some("Left"),
        "Return" => Some("Enter"),
        "Right" => Some("Right"),
        "space" => Some("Space"),
        "Tab" => Some("Tab"),
        "Up" => Some("Up"),
        _ => None,
    }
}

fn is_modifier_only_key(keysym_name: &str) -> bool {
    matches!(
        keysym_name,
        "Alt_L"
            | "Alt_R"
            | "Caps_Lock"
            | "Control_L"
            | "Control_R"
            | "ISO_Level3_Shift"
            | "Meta_L"
            | "Meta_R"
            | "Shift_L"
            | "Shift_R"
            | "Super_L"
            | "Super_R"
    )
}

fn title_case_words(text: &str) -> String {
    text.split(['_', ' '])
        .filter(|part| !part.is_empty())
        .map(title_case_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn title_case_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    let mut output = String::new();
    output.extend(first.to_uppercase());
    output.push_str(&chars.as_str().to_lowercase());
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_cases_key_names() {
        assert_eq!(title_case_words("KP_Page_Down"), "Kp Page Down");
        assert_eq!(title_case_words("audio_lower_volume"), "Audio Lower Volume");
    }

    #[test]
    fn special_key_labels_match_extension_logic() {
        assert_eq!(special_key_label("Return"), Some("Enter"));
        assert_eq!(special_key_label("space"), Some("Space"));
        assert_eq!(special_key_label("Foo"), None);
    }

    #[test]
    fn modifier_only_keys_are_filtered() {
        assert!(is_modifier_only_key("Shift_L"));
        assert!(is_modifier_only_key("Control_R"));
        assert!(!is_modifier_only_key("a"));
    }
}
