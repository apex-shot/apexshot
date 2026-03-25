# System Audio Monitoring Plan

## Problem
- Mic tile picks up speaker output (feedback bleed) because the PipeWire stream autoconnects to a combined source
- Need separate, clean audio sources: physical mic vs system audio output

## Approach
Two PipeWire streams with explicit targets:
- **Mic**: Targets `alsa_input.*` (physical mic) via `TARGET_OBJECT` property
- **System Audio**: Targets sink monitor via `STREAM_CAPTURE_SINK="true"` + `TARGET_OBJECT` on the sink

## Files to Modify

### 1. `src/daemon/mod.rs`
- [x] `MIC_LEVEL` atomic exists
- [ ] Add `SPEAKER_LEVEL` atomic (same pattern)
- [ ] Refactor `start_mic_monitoring()` to take a target name + level atomic + stream name
- [ ] Rename to `start_audio_monitoring()` or keep two calls
- [ ] Mic stream: set `TARGET_OBJECT` to `alsa_input.*` source
- [ ] Speaker stream: set `STREAM_CAPTURE_SINK="true"` + `TARGET_OBJECT` to `alsa_output.*` sink
- [ ] Add `GetSpeakerLevel` D-Bus method (mirrors `GetMicLevel`)

### 2. `capture-overlay/src/CaptureOverlay.h`
- [ ] Add `double m_speakerLevel` member variable

### 3. `capture-overlay/src/CaptureOverlay.cpp`
- [ ] Initialize `m_speakerLevel = 0.0` in constructor
- [ ] In timer callback: poll `GetSpeakerLevel` alongside `GetMicLevel`
- [ ] Draw blue VU bars on Speaker tile (i==1) when `m_recSpeaker` is active
- [ ] Color scheme: cool blue/teal gradient (vs warm orange for mic)

### 4. No changes to `Cargo.toml` or `CMakeLists.txt`

## PipeWire Properties

Mic stream:
```
MEDIA_TYPE=Audio, MEDIA_CATEGORY=Capture, MEDIA_ROLE=Production
TARGET_OBJECT=alsa_input.pci-0000_00_1f.3.analog-stereo
```

Speaker stream:
```
MEDIA_TYPE=Audio, MEDIA_CATEGORY=Capture, MEDIA_ROLE=Production
STREAM_CAPTURE_SINK=true
TARGET_OBJECT=alsa_output.pci-0000_00_1f.3.analog-stereo
```

## Color Scheme
- Mic: warm orange gradient (#FF9632 → #FF6400, peak red #FF3C3C)
- Speaker: cool blue/teal gradient (#32C8FF → #008CFF, peak #FF6060)

## Testing
1. Build: `cargo build --release && cmake --build capture-overlay/build`
2. Install: `sudo install -m 755 target/release/apexshot /usr/local/bin/apexshot && sudo install -m 755 capture-overlay/build/apexshot-capture /usr/local/bin/apexshot-capture`
3. Kill + restart daemon: `pkill -x apexshot; sleep 1; /usr/local/bin/apexshot daemon`
4. Verify: Daemon should show two "PipeWire: capturing" lines (one for mic, one for speaker)
5. Play YouTube video → Speaker bars should animate, Mic bars should NOT
6. Talk → Mic bars should animate, Speaker bars should NOT
