Name:           apexshot
Version:        0.2.30
Release:        0
Summary:        Linux screenshot, annotation, OCR, and screen recording tool
License:        GPL-3.0-only
URL:            https://github.com/apex-shot/apexshot
Source0:        https://github.com/apex-shot/apexshot/archive/v%{version}.tar.gz#/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  cmake
BuildRequires:  dbus-1-devel
BuildRequires:  fdupes
BuildRequires:  gcc-c++
BuildRequires:  git
BuildRequires:  gtk4-devel
BuildRequires:  gtk4-layer-shell-devel
BuildRequires:  libQt5Core-devel
BuildRequires:  libQt5DBus-devel
BuildRequires:  libQt5Network-devel
BuildRequires:  libQt5Widgets-devel
BuildRequires:  libadwaita-devel
BuildRequires:  libqt5-qtx11extras-devel
BuildRequires:  libXtst-devel
BuildRequires:  pkgconfig
BuildRequires:  pipewire-devel
BuildRequires:  rust
BuildRequires:  tesseract-ocr-devel
BuildRequires:  update-desktop-files
BuildRequires:  pkgconfig(gstreamer-1.0)
BuildRequires:  pkgconfig(gstreamer-app-1.0)
BuildRequires:  pkgconfig(gstreamer-video-1.0)

Requires:       curl
Requires:       ffmpeg
Requires:       gstreamer-plugins-base
Requires:       gstreamer-plugins-good
Requires:       gstreamer-plugins-bad
Requires:       gstreamer-plugin-pipewire
Requires:       pipewire
Requires:       pipewire-pulseaudio
Requires:       tesseract-ocr
Requires:       unzip
Requires:       wget
Requires:       wl-clipboard
Requires:       xdg-desktop-portal
Requires:       xdg-utils
Recommends:     xdg-desktop-portal-gnome
Recommends:     xdg-desktop-portal-kde
Recommends:     xdg-desktop-portal-wlr

%description
ApexShot is an open-source Linux screenshot, annotation, OCR, and screen
recording tool with Wayland portal and PipeWire support.

%prep
%autosetup

%build
cargo build --release

%install
install -Dm0755 target/release/apexshot %{buildroot}%{_bindir}/apexshot
install -Dm0755 target/release/apexshot-capture %{buildroot}%{_bindir}/apexshot-capture
install -Dm0755 packaging/deb/apexshot-native-host %{buildroot}%{_bindir}/apexshot-native-host

install -Dm0644 packaging/apexshot.desktop %{buildroot}%{_datadir}/applications/io.github.codegoddy.apexshot.desktop
install -Dm0644 packaging/apexshot-daemon.desktop %{buildroot}%{_sysconfdir}/xdg/autostart/apexshot.desktop
install -Dm0644 packaging/apexshot.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/apexshot.svg
install -Dm0644 packaging/apexshot.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg
install -Dm0644 packaging/apexshot.svg %{buildroot}%{_datadir}/pixmaps/apexshot.svg

install -Dm0644 native-host/io.github.codegoddy.apexshot.json %{buildroot}%{_sysconfdir}/opt/chrome/NativeMessagingHosts/io.github.codegoddy.apexshot.json
install -Dm0644 native-host/io.github.codegoddy.apexshot.json %{buildroot}%{_sysconfdir}/chromium/NativeMessagingHosts/io.github.codegoddy.apexshot.json

extension_dir=%{buildroot}%{_datadir}/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
install -d "${extension_dir}"
install -Dm0644 gnome-extension/metadata.json "${extension_dir}/metadata.json"
install -Dm0644 gnome-extension/extension.js "${extension_dir}/extension.js"
install -Dm0644 gnome-extension/controls-ui.js "${extension_dir}/controls-ui.js"
install -Dm0644 gnome-extension/controls-ui-layout.js "${extension_dir}/controls-ui-layout.js"
install -Dm0644 gnome-extension/runtime-overlays.js "${extension_dir}/runtime-overlays.js"
install -Dm0644 gnome-extension/runtime-overlays-visibility.js "${extension_dir}/runtime-overlays-visibility.js"
install -Dm0644 gnome-extension/mask-ui.js "${extension_dir}/mask-ui.js"
install -Dm0644 gnome-extension/window-list.js "${extension_dir}/window-list.js"
install -Dm0644 gnome-extension/session-state.js "${extension_dir}/session-state.js"
install -Dm0644 gnome-extension/screenshot-lock.js "${extension_dir}/screenshot-lock.js"
install -Dm0644 gnome-extension/gnome-version.js "${extension_dir}/gnome-version.js"

for img in src/capture/editor/background-images/*.jpg; do
    install -Dm0644 "${img}" "%{buildroot}%{_datadir}/apexshot/background-images/$(basename "${img}")"
done

for snd in assets/sounds/*.ogg; do
    install -Dm0644 "${snd}" "%{buildroot}%{_datadir}/apexshot/sounds/$(basename "${snd}")"
done

%fdupes %{buildroot}%{_datadir}

%post
%desktop_database_post
%icon_theme_cache_post

%postun
%desktop_database_postun
%icon_theme_cache_postun

%files
%license LICENSE
%doc README.md
%{_bindir}/apexshot
%{_bindir}/apexshot-capture
%{_bindir}/apexshot-native-host
%{_datadir}/applications/io.github.codegoddy.apexshot.desktop
%config %{_sysconfdir}/xdg/autostart/apexshot.desktop
%{_datadir}/icons/hicolor/scalable/apps/apexshot.svg
%{_datadir}/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg
%{_datadir}/pixmaps/apexshot.svg
%dir %{_sysconfdir}/opt
%dir %{_sysconfdir}/opt/chrome
%dir %{_sysconfdir}/opt/chrome/NativeMessagingHosts
%config %{_sysconfdir}/opt/chrome/NativeMessagingHosts/io.github.codegoddy.apexshot.json
%dir %{_sysconfdir}/chromium
%dir %{_sysconfdir}/chromium/NativeMessagingHosts
%config %{_sysconfdir}/chromium/NativeMessagingHosts/io.github.codegoddy.apexshot.json
%{_datadir}/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io/
%{_datadir}/apexshot/

%changelog
