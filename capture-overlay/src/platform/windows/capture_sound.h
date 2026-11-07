#pragma once

namespace Platform {

/// Hold the audio endpoint open while a capture is imminent, so the shutter
/// itself starts without waiting for the engine.
void primeShutterSound();

/// Stop the primed silence when no capture follows.
void releaseShutterSound();

/// Play the configured shutter sound once. No-op when sounds are off, the
/// selection is "None", or the bundled WAV is missing.
void playShutterSound();

}  // namespace Platform
