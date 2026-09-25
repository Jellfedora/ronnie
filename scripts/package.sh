#!/usr/bin/env bash
# Builds the release archive for the current OS into dist/: the files published on GitHub releases,
# and those the in-app updater downloads (their names must match src/update.rs).
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rm -rf dist
mkdir -p dist/stage

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
