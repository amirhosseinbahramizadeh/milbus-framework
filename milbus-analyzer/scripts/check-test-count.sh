#!/usr/bin/env bash
# Fail when the workspace test count drops below the committed baseline.
#
# Usage: scripts/check-test-count.sh
# The baseline lives in .github/test-baseline.txt and is compared against the
# sum of every `test result: ok. N passed` line emitted by `cargo test`.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Make cargo discoverable when PATH was not inherited from the parent shell
# (some Windows shells start bash without the Rust toolchain on PATH).
if ! command -v cargo >/dev/null 2>&1; then
  for candidate in "$HOME/.cargo/bin" /mnt/c/Users/*/.cargo/bin; do
    if [ -x "$candidate/cargo" ] || [ -x "$candidate/cargo.exe" ]; then
      export PATH="$PATH:$candidate"
      break
    fi
  done
fi

baseline_file="$repo_root/.github/test-baseline.txt"
baseline="$(grep -v '^#' "$baseline_file" | tr -d '[:space:]')"

output="$(cd "$repo_root" && cargo test --workspace 2>&1)"

if echo "$output" | grep -q 'test result: FAILED'; then
  echo "cargo test reported failures; refusing to check the count"
  exit 1
fi

actual=0
for count in $(echo "$output" | grep -oE 'test result: ok\. [0-9]+ passed' | grep -oE '[0-9]+'); do
  actual=$(( actual + count ))
done

echo "test count baseline: $baseline, observed: $actual"
if (( actual < baseline )); then
  echo "ERROR: test count regressed ($actual < $baseline)"
  exit 1
fi
