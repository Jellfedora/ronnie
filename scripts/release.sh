#!/usr/bin/env bash
# Publishes a new version: ./scripts/release.sh 0.2.0
# Bumps Cargo.toml, commits, tags vX.Y.Z and pushes; GitHub Actions then builds the macOS, Linux and
# Windows archives and publishes the release, which running apps offer to install.
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: scripts/release.sh X.Y.Z}
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version invalide : $version (attendu X.Y.Z)" >&2; exit 1; }
[[ -z $(git status --porcelain) ]] || { echo "des modifications ne sont pas commitées" >&2; exit 1; }
[[ $(git branch --show-current) == main ]] || { echo "à lancer depuis main" >&2; exit 1; }

# Only the package's own version (the first one in the file); works with GNU and BSD sed.
sed -i.bak "1,/^version = \".*\"/s/^version = \".*\"/version = \"$version\"/" Cargo.toml
rm -f Cargo.toml.bak
cargo check --quiet # refreshes Cargo.lock
git commit -am "Release v$version"
git tag -a "v$version" -m "Ronnie $version"
git push origin main "v$version"
echo "v$version poussée : suis la compilation dans l'onglet Actions du repo."
