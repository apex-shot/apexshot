# ApexShot Repository Information

ApexShot is a premium screen capture tool for Linux, featuring a modern landing page and a powerful Rust-based backend.

## Project Structure

- **`frontend/`**: The web landing page for ApexShot.
- **`backend/`**: The core application logic and subprojects.
    - **Rust Project**: The main implementation of the screen capture tool (in root of `backend/`).
    - **`ksnip/`**: A C++ based tool (likely integrated for editing features).
    - **`test_gtk/`**: A small Rust project for testing GTK integration.

---

## 1. Frontend

A beautiful, modern landing page built with the latest web technologies.

- **Tech Stack**:
    - **Framework**: [Next.js 16 (App Router)](https://nextjs.org/)
    - **Library**: [React 19](https://react.dev/)
    - **Styling**: [Tailwind CSS 4](https://tailwindcss.com/), [Framer Motion](https://www.framer.com/motion/)
    - **Graphics**: [Three.js](https://threejs.org/) (via `@react-three/fiber`), [ShaderGradient](https://www.shadergradient.co/)
    - **Language**: [TypeScript](https://www.typescriptlang.org/)
- **Key Features**:
    - Mesh Gradient Backgrounds
    - Mouse-following Spotlight effects
    - Infinite scrolling Marquees
    - Fully Responsive design

---

## 2. Backend (Rust)

The core engine of ApexShot, handling screen capture, OCR, and system integration on Linux.

- **Tech Stack**:
    - **Language**: [Rust](https://www.rust-lang.org/)
    - **Runtime**: [Tokio](https://tokio.rs/) (Async)
    - **GUI/Overlay**: [GTK4](https://www.gtk.org/), [gtk4-layer-shell](https://github.com/wmww/gtk4-layer-shell)
    - **Display Servers**: [X11 (x11rb)](https://github.com/logical-robot/x11rb), [Wayland (wayland-client, ashpd)](https://github.com/fedora-selinux/ashpd)
    - **Media Processing**: [GStreamer](https://gstreamer.freedesktop.org/)
    - **OCR**: [Tesseract](https://github.com/tesseract-ocr/tesseract)
    - **Clipboard**: [arboard](https://github.com/1Password/arboard)
- **Sub-components**:
    - **Capture Overlay**: C++ based overlay (`backend/capture-overlay/`).
    - **Web Scroll Extension**: Browser extension for scrolling captures (`backend/web-scroll-extension/`).

---

## 3. Ksnip (C++)

An integrated tool for screenshot editing and additional functionality.

- **Tech Stack**:
    - **Language**: C++
    - **Framework**: [Qt](https://www.qt.io/)
    - **Build System**: CMake

---

## 4. Design & Documentation

- **[AGENT.md](./AGENT.md)**: Agent-specific instructions and context.
- **[ApexShot Landing Page Design Document (1).md](./ApexShot%20Landing%20Page%20Design%20Document%20(1).md)**: Detailed design specifications for the landing page.
- **[setup-frontend.sh](./setup-frontend.sh)**: Script to automate frontend environment setup.

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
