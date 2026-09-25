#!/usr/bin/env bash
set -euo pipefail
root="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
exec cargo run --manifest-path "$root/apps/native/workbench/Cargo.toml" --locked -- --demo
