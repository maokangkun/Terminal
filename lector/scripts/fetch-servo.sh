#!/usr/bin/env sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
target="$root/vendor/servo"

if [ -e "$target/.git" ]; then
  echo "Servo checkout already exists at $target"
  echo "If it is incomplete, remove it and rerun this script."
  exit 0
fi

mkdir -p "$root/vendor"
git clone --filter=blob:none --depth 1 https://github.com/servo/servo.git "$target"
(
  cd "$target"
  git restore --source=HEAD :/
)
