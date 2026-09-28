#!/usr/bin/env bash
# Builds the release archive for the current OS into dist/: the files published on GitHub releases,
# and those the in-app updater downloads (their names must match src/update.rs).
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# A published build: its own config directory and in-app updates (builds made locally are "dev").
export RONNIE_OFFICIAL=1
rm -rf dist
mkdir -p dist/stage

# Pinned AppImage tools, checked against the digests GitHub publishes for them. The static type2 runtime
# needs no libfuse2 (absent from Ubuntu since 22.04).
APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

# Downloads $1 to $3 (kept in target/ between builds) unless already there, and checks its SHA-256 ($2).
fetch() {
    if [ ! -f "$3" ] || ! echo "$2  $3" | sha256sum -c --status; then
        curl -fsSL --retry 3 -o "$3.part" "$1"
        echo "$2  $3.part" | sha256sum -c --quiet
        mv "$3.part" "$3"
    fi
}

# One-file Linux app (Ubuntu and others): Ronnie-<arch>.AppImage, from the release binary. Its name must
# match src/update.rs.
appimage() {
    local arch=$1 tools=target/appimage-tools dir=dist/stage/Ronnie.AppDir
    mkdir -p "$tools" "$dir/usr/bin"
    fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256" "$tools/appimagetool"
    fetch "$RUNTIME_URL" "$RUNTIME_SHA256" "$tools/runtime"
    chmod +x "$tools/appimagetool"
    cp target/release/ronnie "$dir/usr/bin/"
    ln -sf usr/bin/ronnie "$dir/AppRun"
    cp packaging/linux/ronnie.desktop "$dir/"
    cp assets/icon/icon.png "$dir/ronnie.png"
    ln -sf ronnie.png "$dir/.DirIcon"
    # Extracts itself instead of mounting: works without FUSE (CI containers).
    APPIMAGE_EXTRACT_AND_RUN=1 ARCH=$arch "$tools/appimagetool" --no-appstream --runtime-file "$tools/runtime" \
        "$dir" "dist/Ronnie-$arch.AppImage"
}

case "$(uname -s)" in
Darwin)
    # One universal app for Apple Silicon and Intel Macs.
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --locked --target aarch64-apple-darwin
    cargo build --release --locked --target x86_64-apple-darwin
    app=dist/stage/Ronnie.app
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    lipo -create -output "$app/Contents/MacOS/ronnie" \
        target/aarch64-apple-darwin/release/ronnie target/x86_64-apple-darwin/release/ronnie
    cp assets/icon/Ronnie.icns "$app/Contents/Resources/"
    sed "s/{{VERSION}}/$version/g" packaging/macos/Info.plist >"$app/Contents/Info.plist"
    # Ad-hoc signature: required for arm64 code to run at all (no Apple Developer ID).
    codesign --force --deep --sign - "$app"
    tar -C dist/stage -czf dist/ronnie-universal-apple-darwin.tar.gz Ronnie.app
    ;;
Linux)
    target=$(rustc -vV | sed -n 's/^host: //p')
    cargo build --release --locked
    dir=dist/stage/ronnie
    mkdir -p "$dir"
    cp target/release/ronnie packaging/linux/ronnie.desktop packaging/linux/install.sh "$dir/"
    cp assets/icon/icon.png "$dir/ronnie.png"
    tar -C dist/stage -czf "dist/ronnie-$target.tar.gz" ronnie
    appimage "$(uname -m)"
    ;;
MINGW* | MSYS* | CYGWIN*)
    target=$(rustc -vV | sed -n 's/^host: //p')
    cargo build --release --locked
    cp target/release/ronnie.exe dist/stage/
    (cd dist/stage && 7z a -tzip "../ronnie-$target.zip" ronnie.exe >/dev/null)
    ;;
*)
    echo "OS non géré : $(uname -s)" >&2
    exit 1
    ;;
esac

rm -rf dist/stage
ls -l dist
