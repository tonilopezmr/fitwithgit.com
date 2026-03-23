#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -f "$script_dir/.env" ]]; then
  set -a
  source "$script_dir/.env"
  set +a
fi

exec cargo run --quiet --manifest-path "$script_dir/Cargo.toml" -- "$@"
