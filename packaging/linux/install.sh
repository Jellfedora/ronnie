#!/bin/sh
# Installs Ronnie for the current user: binary in ~/.local/bin, menu entry and icon.
# Updates are then installed by Ronnie itself.
set -e
cd "$(dirname "$0")"
install -Dm755 ronnie "$HOME/.local/bin/ronnie"
install -Dm644 ronnie.png "$HOME/.local/share/icons/hicolor/512x512/apps/ronnie.png"
install -Dm644 ronnie.desktop "$HOME/.local/share/applications/ronnie.desktop"
echo "Ronnie est installé dans ~/.local/bin (lance-le depuis le menu des applications ou avec « ronnie »)."
