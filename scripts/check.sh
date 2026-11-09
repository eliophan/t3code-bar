#!/usr/bin/env bash
set -euo pipefail
bash -n "$(dirname "${BASH_SOURCE[0]}")/install-macos.sh"
bash -n "$(dirname "${BASH_SOURCE[0]}")/install-linux.sh"
python3 "$(dirname "${BASH_SOURCE[0]}")/test-install-linux.py"
cd "$(dirname "${BASH_SOURCE[0]}")/../desktop"
npm ci
npm test
npm run build
for crate in bar-threads bar-usage bar-limits bar-updater bar-core; do
  cargo test --locked --manifest-path "crates/${crate}/Cargo.toml"
done
cargo test --locked --manifest-path src-tauri/Cargo.toml
