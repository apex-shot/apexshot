# <div style="font-size: 72px;">ALPHA AS FUCK</div>

### ApexShot

screenshot++ tool for linux. handles x11 and wayland.

ever since i moved from macos to linux ApexShot is the one app i actually miss.

so let's rip it off and share it with the linux world for free.

i'm building this while HEAVILY assissted by LLMs (Gemini 3s, Claudes, GLM 4.7). it's fine if you don't approve etc, i don't really care. it's been 2 years since i created this repo and nothing proper has come out still. imo it's better this project exists because of AI than never having existed. you don't have to use it.

## what actually works right now

**screenshots:**
- `cargo run -- capture screen` - grabs the whole screen
- `cargo run -- capture area` - drag to select an area (x11 has gtk overlay, wayland uses portal dialogs)
- `cargo run -- capture window` - window capture (wayland: portal, x11: not implemented yet)

**screen recording:**
- `cargo run -- record screen` - record full screen to MP4 (Wayland/X11)
- `cargo run -- record area` - record selected window/monitor (Wayland) or drawn area (X11)
- `cargo run -- record area --gif` - record high-quality GIF and **copy to clipboard**
- automatic fallback to WebM/Theora if H.264 codecs are missing

**OCR - text extraction from screenshots:**
- `cargo run -- capture area --ocr` - select area, extract text, copy to clipboard
- `cargo run -- ocr screenshot.png` - run ocr on existing image
- `cargo run -- ocr screenshot.png --lang eng+fra --min-conf 60` - multi-language + confidence threshold

saves to ~/Pictures (screenshots) or ~/Videos (recordings) by default. you can change that with `--output /some/path`

options:
- `--output <path>` - save somewhere specific
- `--no-cursor` - don't include the mouse cursor
- `--jpeg [quality]` - save as jpeg instead of png (quality 1-100)
- `--prefix <text>` - custom filename prefix
- `--ocr` - run ocr after capture and copy text to clipboard
- `--gif` - record as high-quality GIF (automatic clipboard copy)
- `--lang <code>` - ocr language (eng, fra, deu, eng+fra, etc.)
- `--min-conf <n>` - ocr confidence threshold (0-100, default 50)
- `--no-clipboard` - don't copy ocr result to clipboard

## technicals

**x11 backend:**
- uses x11rb directly (no xlib garbage)
- XFixes for cursor capture
- handles every pixel format variant (rgb/bgr, 24/32-bit, lsb/msb)
- gtk4 overlay for area selection

**wayland backend:**
- uses xdg-desktop-portal via ashpd
- manual DBus implementation to support modern portal features (Region recording)
- works on hyprland, kde, sway, gnome
- security note: wayland doesn't let programs just capture whatever they want. area/window capture go through portal dialogs. this is a feature, not a bug.

**recording (MP4/WebM):**
- GStreamer pipeline with hardware-accelerated encoder fallback.
- Wayland: PipeWire integration via Portal.
- X11: `ximagesrc` for low-latency capture.

**recording (GIF):**
- GStreamer -> FFmpeg Pipe architecture.
- real-time streaming for high performance (no dropped frames).
- `palettegen` filter for superior color quality.
- copies File URI (`file://`) for instant paste into Discord/Slack/Browsers.

**ocr:**
- tesseract for text extraction
- wl-copy on wayland (arboard on x11) for clipboard
- **preprocessing pipeline optimized for ui text:**
  - 3x upscaling (lanczos3) for better dpi - tesseract prefers ~300 dpi
  - color inversion for dark-mode uis - tesseract is trained on dark-on-light documents
  - mild contrast enhancement
- **before these improvements:** ocr on dark-mode apps (telegram, discord) had frequent misreads like "i"→"1", "b"→"87", "?"→"p"
- **after:** ~91% confidence on typical chat text with proper character recognition

## building

1. install rust
2. **install system dependencies:**
   
   **Arch Linux:**
   ```bash
   sudo pacman -S tesseract leptonica tesseract-data-eng # OCR
   sudo pacman -S gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly # Recording
   sudo pacman -S ffmpeg wl-clipboard # GIF and Clipboard support
   ```

   **Ubuntu/Debian:**
   ```bash
   sudo apt install tesseract-ocr libtesseract-dev # OCR
   sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
       gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
       gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly # Recording
   sudo apt install ffmpeg wl-clipboard # GIF and Clipboard support
   ```

   **Fedora:**
   ```bash
   sudo dnf install tesseract leptonica # OCR
   sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel \
       gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free # Recording
   sudo dnf install ffmpeg wl-clipboard # GIF and Clipboard support
   ```

3. `cargo build`
4. `cargo run -- capture area` or `cargo run -- record screen --gif`

## what's coming (eventually)
hotkeys - right now everything's run from the terminal, but the plan is to be able to configure the shortcuts and run 
everything from those. right now just burning through the features i want to get a mvp and then polish it.

**audio support:**
doesn't work yet, audio is not a priority

**scrolling capture (next up):**
- auto-scroll detection
- frame stitching

**editor:**
- annotations (arrows, boxes, text, blur)
- undo/redo

**cloud upload:**
- s3/custom server
- url shortening

## status

things are starting to work

x11 backend: complete (needs to be tested on x11, haven't done that myself)
wayland backend: complete
ocr: complete
screen recording: complete (MP4/WebM/GIF)
gtk4 area overlay: complete

check ROADMAP.md if you want the full picture. i build and test everything on Arch+Hyprland setup, so it's very much a 'it works on my machine' situation fyi.

## license

do whatever the fuck you want public license
