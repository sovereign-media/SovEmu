#!/usr/bin/env bash
# The backend's test coverage, on this machine: runs the workspace's tests under cargo-llvm-cov
# and writes the reports to coverage/ (gitignored), then prints the summary.
#
#   scripts/coverage.sh                    # every crate
#   scripts/coverage.sh -p sovemu-server   # one crate (any cargo-llvm-cov arguments)
#   scripts/coverage.sh --open             # and open the HTML report
#
# Reports: coverage/summary.txt (per file: regions, functions, lines), coverage/lcov.info (for an
# editor's coverage gutters) and coverage/html/index.html (line by line). CI's Coverage job
# writes the same reports from the container, where the GPU tests run too (Mesa's llvmpipe); on
# a Mac they skip themselves, so the GPU code reads lower here.
#
# Needs cargo-llvm-cov and the llvm-tools component, once:
#   rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/coverage"
cd "$root/SovEmu.Backend"

if ! cargo llvm-cov --version >/dev/null 2>&1; then
  echo "cargo-llvm-cov is missing: rustup component add llvm-tools-preview && cargo install cargo-llvm-cov --locked" >&2
  exit 1
fi

open=false
args=()
for arg in "$@"; do
  if [[ "$arg" == "--open" ]]; then open=true; else args+=("$arg"); fi
done
[[ ${#args[@]} -eq 0 ]] && args=(--workspace)
# The stand-in core the tests load (SovEmu.TestCore) is test equipment, not code under test.
args+=(--ignore-filename-regex 'SovEmu\.TestCore/')

mkdir -p "$out"
# A failing test still leaves a report (and is named in the output); `cargo test` is the gate.
cargo llvm-cov "${args[@]}" --ignore-run-fail --lcov --output-path "$out/lcov.info"
cargo llvm-cov report "${args[@]}" --summary-only | tee "$out/summary.txt"
cargo llvm-cov report "${args[@]}" --html --output-dir "$out"
echo "coverage: $out/html/index.html"
if $open; then
  case "$(uname -s)" in
    Darwin) open "$out/html/index.html" ;;
    *) xdg-open "$out/html/index.html" >/dev/null 2>&1 || true ;;
  esac
fi
