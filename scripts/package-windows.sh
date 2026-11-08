#!/usr/bin/env bash
#
# Build the Windows app tree and, unless asked not to, the .exe installer that
# ships it.
#
# The tree is laid out the way the code already looks for its files: binaries
# and DLLs in `bin`, and everything they read - fonts, sounds, the background
# images, the GTK runtime's schemas and icon theme - under `share` and `lib`
# beside it. The GLib, GTK, and gdk-pixbuf lookups resolve `<dll_dir>/../share`
# and `<dll_dir>/../lib` on their own, which is why the layout is not flat.
#
#   dist/windows/ApexShot/
#     bin/            apexshot-*.exe, apexshot-capture.exe, ffmpeg, Qt + GTK DLLs
#     share/apexshot/ fonts, sounds, background images
#     share/          glib-2.0, icons, gtk-4.0
#     lib/            gdk-pixbuf loaders
#     etc/            fontconfig
#
# Run it from a shell where pkg-config finds GTK4 (an MSYS2 shell with
# /mingw64 or C:/gtk on PATH) and cmake finds Qt 5; both are the same tools a
# developer build already needs.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

stage=$repo_root/dist/windows/ApexShot
output=$repo_root/dist/windows
ffmpeg_dir=${APEXSHOT_FFMPEG_DIR:-}
gtk_dir=${APEXSHOT_GTK_DIR:-}
qt_dir=${APEXSHOT_QT_DIR:-}
iscc=${APEXSHOT_ISCC:-}
build_rust=1
build_shell=1
build_installer=1
jobs=${APEXSHOT_JOBS:-4}

usage() {
    cat <<'EOF'
Usage: scripts/package-windows.sh [options]

  --skip-build        reuse the existing release binaries and Qt shell
  --skip-installer    stage the app tree only; do not compile the .exe
  --ffmpeg-dir DIR    folder holding ffmpeg.exe, ffprobe.exe, and their DLLs
  --gtk-dir DIR       GTK4 runtime prefix (default: pkg-config --variable=prefix gtk4)
  --qt-dir DIR        Qt 5 prefix (default: read from capture-overlay/build)
  --iscc PATH         Inno Setup compiler (default: the usual install folders)
  -h, --help          print this text

Environment: APEXSHOT_FFMPEG_DIR, APEXSHOT_GTK_DIR, APEXSHOT_QT_DIR,
APEXSHOT_ISCC, APEXSHOT_JOBS.
EOF
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build) build_rust=0; build_shell=0 ;;
        --skip-installer) build_installer=0 ;;
        --ffmpeg-dir) ffmpeg_dir=$2; shift ;;
        --gtk-dir) gtk_dir=$2; shift ;;
        --qt-dir) qt_dir=$2; shift ;;
        --iscc) iscc=$2; shift ;;
        -h|--help) usage; exit 0 ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[[ -n $version ]] || die "could not read the version from Cargo.toml"

# --- toolchain discovery ------------------------------------------------

if [[ -z $gtk_dir ]]; then
    command -v pkg-config >/dev/null 2>&1 ||
        die "pkg-config is not on PATH; install it or pass --gtk-dir"
    gtk_dir=$(pkg-config --variable=prefix gtk4) ||
        die "pkg-config cannot find gtk4; install the GTK4 runtime or pass --gtk-dir"
fi
[[ -d $gtk_dir/bin ]] || die "no bin folder in the GTK prefix: $gtk_dir"
[[ -f $gtk_dir/bin/gdk-pixbuf-query-loaders.exe || -d $gtk_dir/lib/gdk-pixbuf-2.0 ]] ||
    die "the GTK prefix at $gtk_dir is missing the gdk-pixbuf loaders"

if [[ -z $qt_dir ]]; then
    cache=$repo_root/capture-overlay/build/CMakeCache.txt
    if [[ -f $cache ]]; then
        qt5_cmake=$(sed -n 's/^Qt5_DIR:PATH=//p' "$cache" | head -1)
        if [[ -n $qt5_cmake ]]; then
            # <prefix>/lib/cmake/Qt5 -> <prefix>
            qt_dir=$(cd "$qt5_cmake/../../.." && pwd)
        fi
    fi
fi
if [[ -z $qt_dir ]] && command -v qmake >/dev/null 2>&1; then
    qt_dir=$(qmake -query QT_INSTALL_PREFIX)
fi
[[ -n $qt_dir ]] || die "cannot find Qt 5; pass --qt-dir"
[[ -f $qt_dir/bin/windeployqt.exe ]] || die "no windeployqt in $qt_dir/bin"

if [[ -z $ffmpeg_dir ]]; then
    for candidate in "$repo_root/capture-overlay/third_party/ffmpeg/bin" "$repo_root/dist/ffmpeg/bin"; do
        if [[ -f $candidate/ffmpeg.exe ]]; then
            ffmpeg_dir=$candidate
            break
        fi
    done
fi
[[ -n $ffmpeg_dir ]] || die "no ffmpeg payload; pass --ffmpeg-dir DIR or set APEXSHOT_FFMPEG_DIR"
[[ -f $ffmpeg_dir/ffmpeg.exe ]] || die "no ffmpeg.exe in $ffmpeg_dir"
[[ -f $ffmpeg_dir/ffprobe.exe ]] || die "no ffprobe.exe in $ffmpeg_dir (the editors probe colour and frame rate with it)"

if [[ $build_installer == 1 && -z $iscc ]]; then
    for candidate in \
        "${LOCALAPPDATA:-}/Programs/Inno Setup 6/ISCC.exe" \
        "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
        "/c/Program Files/Inno Setup 6/ISCC.exe"; do
        if [[ -f $candidate ]]; then
            iscc=$candidate
            break
        fi
    done
fi
if [[ $build_installer == 1 ]]; then
    [[ -n $iscc && -f $iscc ]] ||
        die "Inno Setup is not installed; get it with 'winget install JRSoftware.InnoSetup' or pass --iscc, or run with --skip-installer"
fi

printf 'ApexShot %s\n  gtk   %s\n  qt    %s\n  ffmpeg %s\n' "$version" "$gtk_dir" "$qt_dir" "$ffmpeg_dir"

# --- Visual Studio ------------------------------------------------------

# MSVC needs its own environment for the compiler's include and library paths,
# which a plain shell does not carry, and the redistributable folder is the only
# licensed source for the C++ runtime the Qt and GTK DLLs import.
vcvars=""
vc_dir=""
for candidate in \
    "/c/Program Files/Microsoft Visual Studio/2022/Community/VC/Auxiliary/Build/vcvars64.bat" \
    "/c/Program Files/Microsoft Visual Studio/2022/Professional/VC/Auxiliary/Build/vcvars64.bat" \
    "/c/Program Files/Microsoft Visual Studio/2022/Enterprise/VC/Auxiliary/Build/vcvars64.bat" \
    "/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Auxiliary/Build/vcvars64.bat"; do
    if [[ -f $candidate ]]; then
        vcvars=$(cygpath -w "$candidate")
        vc_dir=$(cd "$(dirname "$candidate")/../.." && pwd)
        break
    fi
done

# --- builds -------------------------------------------------------------

# Run one command inside the MSVC environment. cmd has no way to quote the
# vcvars path inside a `cmd /c "..."` string - the nested quotes have to be
# escaped, and Git Bash rewrites them to backslashes that cmd then reads
# literally. Writing both lines to a batch file leaves the quoting to cmd.
run_in_msvc_env() {
    [[ -n $vcvars ]] || die "no Visual Studio 2022 vcvars64.bat; the Qt shell needs MSVC"
    local dir bat status
    dir=$(mktemp -d)
    bat=$dir/build.bat
    {
        printf '@call "%s" >nul\r\n' "$vcvars"
        printf '@%s\r\n' "$1"
    } > "$bat"
    MSYS_NO_PATHCONV=1 cmd /c "$(cygpath -w "$bat")"
    status=$?
    rm -rf "$dir"
    return $status
}

shell_build() {
    local build_dir=$repo_root/capture-overlay/build

    if [[ ! -f $build_dir/CMakeCache.txt ]]; then
        echo "Configuring the capture shell"
        run_in_msvc_env "cmake -S \"$(cygpath -w "$repo_root/capture-overlay")\" -B \"$(cygpath -w "$build_dir")\" -G Ninja -DCMAKE_BUILD_TYPE=Release -DQt5_DIR=\"$(cygpath -w "$qt_dir")\\lib\\cmake\\Qt5\"" ||
            die "cmake could not configure the capture shell"
    fi
    echo "Building the capture shell"
    run_in_msvc_env "cmake --build \"$(cygpath -w "$build_dir")\" --target apexshot-capture" ||
        die "the capture shell did not build"
}

if [[ $build_rust == 1 ]]; then
    echo "Building the release binaries"
    # The Studio pair is compile-time, so a build without it ships a package
    # that cannot verify a licence.
    # shellcheck source=/dev/null
    source "$repo_root/scripts/studio-build-env.sh"
    ( cd windows-ui && cargo build --release --bins -j "$jobs" ) ||
        die "cargo could not build the release binaries"
fi

if [[ $build_shell == 1 ]]; then
    shell_build
fi

# --- staging ------------------------------------------------------------

rm -rf "$stage"
mkdir -p "$stage/bin" "$stage/share" "$stage/lib" "$stage/etc"

echo "Staging the binaries"
for binary in apexshot-settings apexshot-history apexshot-video-editor apexshot-image-editor apexshot-update; do
    [[ -f windows-ui/target/release/$binary.exe ]] || die "missing windows-ui/target/release/$binary.exe; run without --skip-build"
    cp "windows-ui/target/release/$binary.exe" "$stage/bin/"
done
[[ -f capture-overlay/build/apexshot-capture.exe ]] || die "missing capture-overlay/build/apexshot-capture.exe; run without --skip-build"
cp capture-overlay/build/apexshot-capture.exe "$stage/bin/"

echo "Staging ffmpeg"
cp "$ffmpeg_dir/ffmpeg.exe" "$ffmpeg_dir/ffprobe.exe" "$stage/bin/"
for library in "$ffmpeg_dir"/*.dll; do
    [[ -f $library ]] && cp "$library" "$stage/bin/"
done
for license in "$ffmpeg_dir/../LICENSE.txt" "$ffmpeg_dir/LICENSE.txt"; do
    if [[ -f $license ]]; then
        cp "$license" "$stage/bin/ffmpeg-LICENSE.txt"
        break
    fi
done

echo "Staging the GTK runtime"
# The relocation closure rather than the whole prefix: the GTK build tree holds
# editors, headers, and Python wheels the app never loads. The loader needs the
# POSIX spelling of the prefix on PATH; a drive-letter path finds nothing.
gtk_posix=$(cygpath -u "$gtk_dir")
exe=$stage/bin/apexshot-settings.exe
PATH="$gtk_posix/bin:$PATH" ldd "$exe" 2>/dev/null |
    awk '{ for (field = 1; field <= NF; field++) if ($field ~ /^\//) print $field }' |
    while read -r library; do
        name=$(basename "$library")
        if [[ -f $gtk_dir/bin/$name ]]; then
            cp "$gtk_dir/bin/$name" "$stage/bin/"
        fi
    done
if [[ ! -f $stage/bin/gtk-4-1.dll ]]; then
    die "could not resolve the GTK runtime from $gtk_dir; the prefix must hold bin/gtk-4-1.dll and the DLLs it loads"
fi
cp -r "$gtk_dir/share/glib-2.0" "$gtk_dir/share/icons" "$gtk_dir/share/gtk-4.0" "$stage/share/"
mkdir -p "$stage/lib/gdk-pixbuf-2.0"
cp -r "$gtk_dir/lib/gdk-pixbuf-2.0/2.10.0" "$stage/lib/gdk-pixbuf-2.0/"
if [[ -d $gtk_dir/etc/fonts ]]; then
    cp -r "$gtk_dir/etc/fonts" "$stage/etc/"
fi

echo "Staging the Qt runtime"
# --compiler-runtime is left off on purpose: it stages the whole 25 MB
# redistributable installer, and the package ships the individual DLLs below.
MSYS_NO_PATHCONV=1 "$qt_dir/bin/windeployqt.exe" \
    --release \
    --no-translations \
    --no-opengl-sw \
    --no-system-d3d-compiler \
    --dir "$(cygpath -w "$stage/bin")" \
    "$(cygpath -w "$stage/bin/apexshot-capture.exe")" >/dev/null ||
    die "windeployqt could not stage the Qt runtime"

echo "Staging the Visual C++ runtime"
# Without these nothing in the package starts on a machine that never installed
# the redistributable: the Qt and GTK DLLs and every Rust binary import it.
for library in msvcp140 msvcp140_1 msvcp140_2 vcruntime140 vcruntime140_1; do
    [[ -f $stage/bin/$library.dll ]] && continue
    source_dll=""
    if [[ -n $vc_dir ]]; then
        for candidate in "$vc_dir"/Redist/MSVC/*/x64/Microsoft.VC143.CRT/$library.dll; do
            if [[ -f $candidate ]]; then
                source_dll=$candidate
                break
            fi
        done
    fi
    if [[ -z $source_dll ]]; then
        printf 'warning: %s.dll is not in the package; a machine without the Visual C++ redistributable will not start it\n' "$library" >&2
        continue
    fi
    cp "$source_dll" "$stage/bin/"
done

echo "Staging the app's own assets"
mkdir -p "$stage/share/apexshot/fonts" "$stage/share/apexshot/sounds" "$stage/share/apexshot/background-images"
cp capture-overlay/assets/fonts/*.ttf "$stage/share/apexshot/fonts/"
cp capture-overlay/assets/fonts/*LICENSE*.txt capture-overlay/assets/fonts/OFL.txt "$stage/share/apexshot/fonts/" 2>/dev/null || true
cp assets/sounds/* "$stage/share/apexshot/sounds/"
cp src/capture/editor/background-images/*.jpg "$stage/share/apexshot/background-images/"
cp LICENSE "$stage/LICENSE"

# --- checks -------------------------------------------------------------

for required in \
    bin/apexshot-settings.exe \
    bin/apexshot-capture.exe \
    bin/apexshot-history.exe \
    bin/apexshot-video-editor.exe \
    bin/apexshot-image-editor.exe \
    bin/apexshot-update.exe \
    bin/ffmpeg.exe \
    bin/ffprobe.exe \
    bin/gtk-4-1.dll \
    bin/Qt5Core.dll \
    share/apexshot/fonts/DMSans-Regular.ttf \
    share/apexshot/fonts/Inter-Regular.ttf \
    share/apexshot/sounds/camera.wav \
    share/glib-2.0/schemas/gschemas.compiled \
    lib/gdk-pixbuf-2.0/2.10.0/loaders.cache; do
    [[ -e $stage/$required ]] || die "the staged tree is missing $required"
done

printf 'Staged %s (%s)\n' "$stage" "$(du -sh "$stage" | cut -f1)"

if [[ $build_installer == 0 ]]; then
    exit 0
fi

# --- installer ----------------------------------------------------------

echo "Compiling the installer"
mkdir -p "$output"
MSYS_NO_PATHCONV=1 "$iscc" \
    "/DAppVersion=$version" \
    "/DSourceDir=$(cygpath -w "$stage")" \
    "/DOutputDir=$(cygpath -w "$output")" \
    "/DIconFile=$(cygpath -w "$repo_root/packaging/windows/apexshot.ico")" \
    "$(cygpath -w "$repo_root/packaging/windows/ApexShot.iss")" >/dev/null ||
    die "Inno Setup failed to compile the installer"

installer=$output/ApexShot-$version-setup.exe
[[ -f $installer ]] || die "Inno Setup reported success but $installer is missing"
printf 'Installer: %s (%s)\n' "$installer" "$(du -h "$installer" | cut -f1)"
