use ashpd::desktop::{
    remote_desktop::{Axis, DeviceType, KeyState, RemoteDesktop},
    screencast::{CursorMode, Screencast, SourceType},
    PersistMode, Session,
};

/// The D-Bus interface object. Holds a channel sender to forward actions to
/// the daemon's main loop.
pub(super) struct PortalScrollSession {
    remote: RemoteDesktop<'static>,
    session: Session<'static, RemoteDesktop<'static>>,
    stream_node_id: Option<u32>,
    stream_pos: (i32, i32),
    stream_size: Option<(i32, i32)>,
}

#[derive(Default)]
pub(super) struct ScrollInjector {
    portal: Option<PortalScrollSession>,
    focused: bool,
}

impl ScrollInjector {
    pub(super) async fn begin(&mut self) -> Result<bool, String> {
        if self.portal.is_some() {
            return Ok(true);
        }

        let remote: RemoteDesktop<'static> = RemoteDesktop::new()
            .await
            .map_err(|e| format!("RemoteDesktop proxy init failed: {e}"))?;

        let screencast = Screencast::new()
            .await
            .map_err(|e| format!("Screencast proxy init failed: {e}"))?;

        let session: Session<'static, RemoteDesktop<'static>> = remote
            .create_session()
            .await
            .map_err(|e| format!("RemoteDesktop create_session failed: {e}"))?;

        remote
            .select_devices(
                &session,
                DeviceType::Pointer | DeviceType::Keyboard,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| format!("RemoteDesktop select_devices failed: {e}"))?;

        screencast
            .select_sources(
                &session,
                CursorMode::Hidden,
                SourceType::Monitor.into(),
                false,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| format!("Screencast select_sources failed: {e}"))?;

        let selected = remote
            .start(&session, None)
            .await
            .map_err(|e| format!("RemoteDesktop start request failed: {e}"))?
            .response()
            .map_err(|e| format!("RemoteDesktop start denied: {e}"))?;

        let (stream_node_id, stream_pos, stream_size) =
            if let Some(stream) = selected.streams().and_then(|streams| streams.first()) {
                (
                    Some(stream.pipe_wire_node_id()),
                    stream.position().unwrap_or((0, 0)),
                    stream.size(),
                )
            } else {
                (None, (0, 0), None)
            };

        self.portal = Some(PortalScrollSession {
            remote,
            session,
            stream_node_id,
            stream_pos,
            stream_size,
        });
        self.focused = false;
        eprintln!(
            "[daemon] RemoteDesktop scroll session started (stream={:?})",
            stream_node_id
        );
        Ok(true)
    }

    pub(super) async fn step(&mut self, target_x: i32, target_y: i32, steps: i32) -> bool {
        if self.begin().await != Ok(true) {
            return false;
        }

        let Some(portal) = self.portal.as_ref() else {
            return false;
        };

        let mut ok = false;

        if let Some(stream_id) = portal.stream_node_id {
            let (sx, sy) = portal.stream_pos;
            let mut local_x = (target_x - sx).max(0) as f64;
            let mut local_y = (target_y - sy).max(0) as f64;
            if let Some((w, h)) = portal.stream_size {
                local_x = local_x.min((w.saturating_sub(1)) as f64);
                local_y = local_y.min((h.saturating_sub(1)) as f64);
            }

            if portal
                .remote
                .notify_pointer_motion_absolute(&portal.session, stream_id, local_x, local_y)
                .await
                .is_ok()
            {
                ok = true;
                let press_ok = portal
                    .remote
                    .notify_pointer_button(&portal.session, 272, KeyState::Pressed)
                    .await
                    .is_ok();
                let release_ok = portal
                    .remote
                    .notify_pointer_button(&portal.session, 272, KeyState::Released)
                    .await
                    .is_ok();
                self.focused = press_ok && release_ok;
                ok = ok || self.focused;
            }
        }

        let count = std::cmp::max(1, steps);
        for _ in 0..count {
            let axis_ok = portal
                .remote
                .notify_pointer_axis_discrete(&portal.session, Axis::Vertical, -1)
                .await
                .is_ok();

            let smooth_axis_ok = portal
                .remote
                .notify_pointer_axis(&portal.session, 0.0, 36.0, true)
                .await
                .is_ok();

            let keysym_ok = portal
                .remote
                .notify_keyboard_keysym(&portal.session, 0xFF56, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keysym(&portal.session, 0xFF56, KeyState::Released)
                    .await
                    .is_ok();

            let keycode_ok = portal
                .remote
                .notify_keyboard_keycode(&portal.session, 109, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keycode(&portal.session, 109, KeyState::Released)
                    .await
                    .is_ok();

            let down_keycode_ok = portal
                .remote
                .notify_keyboard_keycode(&portal.session, 108, KeyState::Pressed)
                .await
                .is_ok()
                && portal
                    .remote
                    .notify_keyboard_keycode(&portal.session, 108, KeyState::Released)
                    .await
                    .is_ok();

            eprintln!(
                "[daemon] portal scroll step: axis_ok={}, smooth_axis_ok={}, keysym_ok={}, keycode_ok={}, down_keycode_ok={}, focused={}, target=({}, {})",
                axis_ok,
                smooth_axis_ok,
                keysym_ok,
                keycode_ok,
                down_keycode_ok,
                self.focused,
                target_x,
                target_y
            );

            ok = ok || axis_ok || smooth_axis_ok || keysym_ok || keycode_ok || down_keycode_ok;
        }

        if !ok {
            self.end().await;
        }

        ok
    }

    pub(super) async fn end(&mut self) {
        if let Some(portal) = self.portal.take() {
            let _ = portal.session.close().await;
            eprintln!("[daemon] RemoteDesktop scroll session ended");
        }
        self.focused = false;
    }
}
