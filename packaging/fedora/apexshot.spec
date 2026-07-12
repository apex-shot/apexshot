Name:           apexshot
Version:        0.2.31
Release:        1%{?dist}
Summary:        Linux screenshot, annotation, OCR, and screen recording tool
License:        GPL-3.0-only
URL:            https://github.com/apex-shot/apexshot
Source0:        https://github.com/apex-shot/apexshot/archive/v%{version}/%{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  clang
BuildRequires:  cmake
BuildRequires:  dbus-devel
BuildRequires:  gcc-c++
BuildRequires:  git
BuildRequires:  gtk4-devel
BuildRequires:  gtk4-layer-shell-devel
BuildRequires:  libadwaita-devel
BuildRequires:  pipewire-devel
BuildRequires:  pkgconf-pkg-config
BuildRequires:  qt5-qtbase-devel
BuildRequires:  qt5-qtx11extras-devel
BuildRequires:  tesseract-devel
BuildRequires:  libXtst-devel
BuildRequires:  desktop-file-utils
BuildRequires:  hicolor-icon-theme
BuildRequires:  pkgconfig(gstreamer-1.0)
BuildRequires:  pkgconfig(gstreamer-app-1.0)
BuildRequires:  pkgconfig(gstreamer-video-1.0)

Requires:       curl
Requires:       ffmpeg-free
Requires:       gstreamer1-plugins-base
Requires:       gstreamer1-plugins-good
Requires:       gstreamer1-plugins-bad-free
Requires:       gstreamer1-plugin-libav
Requires:       pipewire
Requires:       pipewire-pulseaudio
Requires:       tesseract
Requires:       unzip
Requires:       wget
Requires:       wl-clipboard
Requires:       xdg-desktop-portal
Requires:       xdg-utils
Recommends:     xdg-desktop-portal-gnome
Recommends:     xdg-desktop-portal-kde
Recommends:     xdg-desktop-portal-gtk

%description
ApexShot is an open-source Linux screenshot, annotation, OCR, and screen
recording tool with Wayland portal and PipeWire support.

%prep
%autosetup -n %{name}-%{version}

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

%post
if [ -x %{_bindir}/update-desktop-database ]; then
    %{_bindir}/update-desktop-database -q %{_datadir}/applications || :
fi
if [ -x %{_bindir}/gtk-update-icon-cache ]; then
    %{_bindir}/gtk-update-icon-cache -q %{_datadir}/icons/hicolor || :
fi

%postun
if [ -x %{_bindir}/update-desktop-database ]; then
    %{_bindir}/update-desktop-database -q %{_datadir}/applications || :
fi
if [ -x %{_bindir}/gtk-update-icon-cache ]; then
    %{_bindir}/gtk-update-icon-cache -q %{_datadir}/icons/hicolor || :
fi

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
* Sun Jul 12 2026 codegoddy <codegoddy@gmail.com> - 0.2.31-1
- KDE/Fedora capture UX, KWin-native screencast, and recording support notes

* Sun Jul 12 2026 codegoddy <codegoddy@gmail.com> - 0.2.30-1
- Cloud upload (ApexShot + XBackBone), editor/recording fixes, and upload double-click guard

* Wed Jun 24 2026 codegoddy <codegoddy@gmail.com> - 0.2.29-1
- KDE portal hotkey listener fixes and Fedora install/update improvements

* Tue Jun 23 2026 codegoddy <codegoddy@gmail.com> - 0.2.28-1
- Initial Fedora RPM recipe
