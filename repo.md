# ApexShot Repository Information

ApexShot is a premium Linux screen capture tool with a Next.js marketing frontend and a Rust desktop application backend.

## Project Structure

- **`frontend/`**: The Next.js landing page and marketing site.
- **`backend/`**: The Rust desktop application and related subprojects.
    - **Rust Project**: The main ApexShot application (`backend/Cargo.toml`).
    - **`capture-overlay/`**: Native C++ overlay helpers.
    - **`native-host/`**: Native messaging host files.
    - **`test_gtk/`**: GTK integration sandbox project.
    - **`tests/`**: Rust integration tests.
    - **`web-scroll-extension/`**: Browser extension for scroll capture.
- **`repo.md`**: High-level repository overview.

---

## 1. Frontend

A modern marketing site built with current web tooling.

- **Tech Stack**:
    - **Framework**: [Next.js 16 (App Router)](https://nextjs.org/)
    - **Library**: [React 19](https://react.dev/)
    - **Styling**: [Tailwind CSS 4](https://tailwindcss.com/), [Framer Motion](https://www.framer.com/motion/)
    - **Graphics**: [Three.js](https://threejs.org/) via `@react-three/fiber`, [ShaderGradient](https://www.shadergradient.co/)
    - **Language**: [TypeScript](https://www.typescriptlang.org/)
- **Key Areas**:
    - Marketing pages under `frontend/app/`
    - Shared UI under `frontend/components/`
    - Frontend utilities under `frontend/lib/`

---

## 2. Backend

The Rust desktop application handles screen capture, editing, OCR, recording, and Linux desktop integration.

- **Tech Stack**:
    - **Language**: [Rust](https://www.rust-lang.org/)
    - **Runtime**: [Tokio](https://tokio.rs/)
    - **GUI/Overlay**: [GTK4](https://www.gtk.org/), [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell)
    - **Display Servers**: [x11rb](https://github.com/logical-robot/x11rb), [wayland-client](https://gitlab.freedesktop.org/wayland/wayland-rs), [ashpd](https://github.com/fedora-selinux/ashpd)
    - **Media Processing**: [GStreamer](https://gstreamer.freedesktop.org/)
    - **OCR**: [Tesseract](https://github.com/tesseract-ocr/tesseract)
    - **Clipboard**: [arboard](https://github.com/1Password/arboard)
- **Notable Paths**:
    - `backend/src/`: Main application source
    - `backend/src/capture/editor/`: Editor implementation and background assets
    - `backend/capture-overlay/`: C++ capture overlay project
    - `backend/web-scroll-extension/`: Browser extension assets

---

## 3. Notes

- `backend/` is a normal folder inside the main repository and should not contain its own nested Git repository.
- `frontend/` remains at the repository root.

---

## Getting Started

### Frontend
```bash
cd frontend
pnpm install
pnpm dev
```

### Backend
```bash
cd backend
cargo build
```
