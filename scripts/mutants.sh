#!/usr/bin/env bash
#
# Run cargo-mutants locally over the lines changed in HEAD (or any base
# ref you choose), so the runtime is in the "useful while iterating"
# range rather than "leave on overnight."
#
# Usage:
#   ./scripts/mutants.sh                      # diff HEAD~1..HEAD
#   ./scripts/mutants.sh main                 # diff main..HEAD
#   ./scripts/mutants.sh 0bb7329              # diff <sha>..HEAD
#   ./scripts/mutants.sh HEAD~5               # diff last 5 commits
#   ./scripts/mutants.sh -- --jobs 4          # default base + extra cargo-mutants args
#   ./scripts/mutants.sh main --jobs 4        # explicit base + extra args
#
# The first non-dash argument is the base ref; anything else (and
# everything after the first dash-prefixed arg) passes through to
# cargo-mutants. See https://mutants.rs/ for the full CLI surface.
#
# Two pre-setup steps the CI workflow also does:
# 1. panschema/src/html_writer.rs uses include_str!/include_bytes! for
#    wasm-pack artifacts. cargo-mutants copies the source tree to a
#    tempdir; the real pkg/ outputs don't follow. Empty placeholders
#    satisfy the includes (mutation testing doesn't exercise the wasm
#    bytes at runtime).
# 2. The .mutants.toml `examine_globs` covers the whole panschema
#    crate (~4800 mutants). `--in-diff` narrows that to just the
#    lines you touched in your active diff.
#
# Prerequisites: `cargo install cargo-mutants` (once per machine).
#
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# Pre-setup: wasm artifact placeholders.
mkdir -p panschema-viz/pkg
touch panschema-viz/pkg/panschema_viz.js panschema-viz/pkg/panschema_viz_bg.wasm

# Resolve the base ref: first non-dash positional arg, defaulting to
# HEAD~1. Anything starting with `-` is treated as a cargo-mutants arg.
if [[ $# -gt 0 && "$1" != -* && "$1" != "--" ]]; then
  BASE="$1"
  shift
else
  BASE="HEAD~1"
fi

# `--` separator is allowed for clarity; consume it so it doesn't pass
# through to cargo-mutants as a positional.
if [[ $# -gt 0 && "$1" == "--" ]]; then
  shift
fi

DIFF="$(mktemp -t panschema-mutants.XXXXXX.diff)"
trap 'rm -f "$DIFF"' EXIT

git diff "${BASE}..HEAD" > "$DIFF"

if [[ ! -s "$DIFF" ]]; then
  echo "no diff between ${BASE} and HEAD — nothing to mutate."
  echo "tip: commit your changes locally first, then re-run."
  exit 0
fi

echo "mutating changes in ${BASE}..HEAD ($(wc -l < "$DIFF") diff lines)"
exec cargo mutants --in-diff "$DIFF" "$@"
