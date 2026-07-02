#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ ! -d vendor/servo/components/servo ]]; then
  echo "vendor/servo is missing. Run ./scripts/fetch-servo.sh first." >&2
  exit 1
fi

VERSION="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
HOST="$(rustc -vV | awk '/host:/ { print $2 }')"
TARGET="${TARGET:-$HOST}"
OUT_DIR="${OUT_DIR:-dist}"
NAME="lector-${VERSION}-${TARGET}"

if [[ "$TARGET" == "$HOST" ]]; then
  cargo build --release --features servo-native
  BIN="target/release/lector"
else
  cargo build --release --features servo-native --target "$TARGET"
  BIN="target/$TARGET/release/lector"
fi

rm -rf "$OUT_DIR/$NAME"
mkdir -p "$OUT_DIR/$NAME"

cp "$BIN" "$OUT_DIR/$NAME/lector"
cp README.md "$OUT_DIR/$NAME/README.md"
if [[ -f LICENSE ]]; then
  cp LICENSE "$OUT_DIR/$NAME/LICENSE"
elif [[ -f ../LICENSE ]]; then
  cp ../LICENSE "$OUT_DIR/$NAME/LICENSE"
fi

(
  cd "$OUT_DIR"
  tar -czf "$NAME.tar.gz" "$NAME"
  shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256"
)

echo "$OUT_DIR/$NAME.tar.gz"
echo "$OUT_DIR/$NAME.tar.gz.sha256"
