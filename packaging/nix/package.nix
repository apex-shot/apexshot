{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  cmake,
  makeWrapper,
  wrapGAppsHook4,
  libsForQt5,
  gtk4,
  gtk4-layer-shell,
  gsettings-desktop-schemas,
  librsvg,
  gst_all_1,
  pipewire,
  tesseract5,
  leptonica,
  dbus,
  wayland,
  libX11,
  libXtst,
  ffmpeg,
  pulseaudio,
  alsa-utils,
  wl-clipboard,
  xclip,
  libnotify,
  glib,
  gtk3,
  curl,
  wget,
  wf-recorder,
  procps,
}:

let
  cargoToml = lib.importTOML ../../Cargo.toml;

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../build.rs
      ../../.cargo
      ../../src
      ../../data
      ../../po
      ../../assets
      ../../capture-overlay
      ../../native-host
      ../../gnome-extension
      ../../packaging/io.github.codegoddy.apexshot.desktop
      ../../packaging/io.github.codegoddy.apexshot.metainfo.xml
      ../../packaging/apexshot-daemon.desktop
      ../../packaging/apexshot.svg
    ];
  };

  tesseract = tesseract5.override { enableLanguages = [ "eng" ]; };

  runtimeTools = [
    ffmpeg
    pulseaudio
    pipewire
    alsa-utils
    wl-clipboard
    xclip
    libnotify
    glib
    dbus
    gtk3
    curl
    wget
    wf-recorder
    procps
  ];

  releaseDir = "target/${stdenv.hostPlatform.rust.cargoShortTarget}/release";
in
rustPlatform.buildRustPackage {
  pname = "apexshot";
  version = cargoToml.package.version;
  inherit src;

  cargoLock.lockFile = "${src}/Cargo.lock";

  buildFeatures = [ "nix" ];
  cargoBuildFlags = [
    "--bin"
    "apexshot"
  ];
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    cmake
    makeWrapper
    wrapGAppsHook4
    libsForQt5.wrapQtAppsHook
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    gtk4
    gtk4-layer-shell
    gsettings-desktop-schemas
    librsvg
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-plugins-bad
    gst_all_1.gst-libav
    pipewire
    tesseract
    leptonica
    dbus
    wayland
    libX11
    libXtst
    libsForQt5.qtbase
    libsForQt5.qtx11extras
  ];

  postInstall = ''
    install -Dm755 ${releaseDir}/apexshot-capture -t $out/bin

    install -Dm644 packaging/io.github.codegoddy.apexshot.metainfo.xml -t $out/share/metainfo
    install -Dm644 packaging/apexshot.svg $out/share/icons/hicolor/scalable/apps/apexshot.svg
    install -Dm644 packaging/apexshot.svg $out/share/icons/hicolor/scalable/apps/io.github.codegoddy.apexshot.svg
    install -Dm644 packaging/apexshot.svg $out/share/pixmaps/apexshot.svg

    install -Dm644 packaging/io.github.codegoddy.apexshot.desktop \
      $out/share/applications/io.github.codegoddy.apexshot.desktop
    substituteInPlace $out/share/applications/io.github.codegoddy.apexshot.desktop \
      --replace-fail /usr/bin/apexshot $out/bin/apexshot
    install -Dm644 packaging/apexshot-daemon.desktop $out/etc/xdg/autostart/apexshot.desktop
    substituteInPlace $out/etc/xdg/autostart/apexshot.desktop \
      --replace-fail /usr/bin/apexshot $out/bin/apexshot

    install -Dm755 native-host/apexshot-native-host -t $out/bin
    substituteInPlace $out/bin/apexshot-native-host \
      --replace-fail /usr/local/bin/apexshot $out/bin/apexshot
    install -Dm644 native-host/io.github.codegoddy.apexshot.json \
      -t $out/share/apexshot/native-messaging-hosts
    substituteInPlace $out/share/apexshot/native-messaging-hosts/io.github.codegoddy.apexshot.json \
      --replace-fail /usr/bin/apexshot-native-host $out/bin/apexshot-native-host

    install -Dm644 src/capture/editor/background-images/*.jpg -t $out/share/apexshot/background-images
    install -Dm644 assets/sounds/*.ogg -t $out/share/apexshot/sounds

    test -d ${releaseDir}/locale
    cp -r ${releaseDir}/locale $out/share/locale

    install -Dm644 gnome-extension/*.js gnome-extension/metadata.json \
      -t $out/share/gnome-shell/extensions/apexshot-gnome-integration@apexshot.github.io
  '';

  postFixup = ''
    wrapProgram $out/bin/apexshot \
      --set APEXSHOT_CAPTURE_BIN $out/bin/apexshot-capture \
      --set-default TESSDATA_PREFIX ${tesseract}/share/tessdata \
      --prefix PATH : ${lib.makeBinPath (map lib.getBin runtimeTools)}
  '';

  meta = {
    description = "Open-source Linux screenshot, annotation, and screen recording tool";
    homepage = "https://github.com/apex-shot/apexshot";
    license = lib.licenses.gpl3Plus;
    mainProgram = "apexshot";
    platforms = [ "x86_64-linux" ];
  };
}
