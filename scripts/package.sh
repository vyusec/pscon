#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk -F'"' '/^version = / { print $2; exit }' Cargo.toml)"
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
NAME="pscon-${VERSION}-${TARGET}"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"

echo "[pscon] building release binary"
cargo build --release --locked -p pscon

echo "[pscon] staging $NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE"

cp target/release/pscon "$STAGE/pscon"
cp README.md LICENSE NOTICE CHANGELOG.md config.example.toml "$STAGE/"
cp -r docs schemas "$STAGE/"
mkdir -p "$STAGE/scripts"
cp scripts/install.sh scripts/setup-laya.sh "$STAGE/scripts/"
cp scripts/install.sh "$STAGE/install.sh"

echo "[pscon] creating archive"
SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --owner=0 --group=0 --numeric-owner \
  -C "$DIST" -cf - "$NAME" | gzip -n > "$DIST/$NAME.tar.gz"
(
  cd "$DIST"
  sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256"
)

echo
echo "package: $DIST/$NAME.tar.gz"
echo "sha256:  $(cat "$DIST/$NAME.tar.gz.sha256")"
