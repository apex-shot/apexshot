#pragma once

namespace Platform {

/// Play the configured shutter sound once. No-op when sounds are off, the
/// selection is "None", or the bundled WAV is missing.
void playShutterSound();

}  // namespace Platform
