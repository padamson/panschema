#!/usr/bin/env bash
#
# Rebuild the panschema-viz WASM bundle, then install the panschema
# CLI from source — the dogfood loop's install command.
#
# panschema/build.rs is a one-time bootstrap: once panschema-viz/pkg/
# exists it does nothing, so a bare `cargo install` re-embeds a stale
# bundle after a panschema-viz change (the rendered graph then breaks
# with no compile error). This script rebuilds the bundle first so the
# embedded WASM always matches the current viz sources.
#
# Usage:
#   ./scripts/dev-install.sh            # debug install (default)
#   ./scripts/dev-install.sh --release  # optimized install

set -euo pipefail

cd "$(dirname "$0")/.."

# Match the bundle profile to the install profile: --release installs
# get the size-optimized bundle, the default debug install gets the
# faster --dev bundle (wasm-opt skipped). `cargo install` builds
# release by default and only takes `--debug` for a debug build.
if [[ "${1:-}" == "--release" ]]; then
  bundle_profile="--release"
  install_flags=()
else
  bundle_profile="--dev"
  install_flags=(--debug)
fi

echo "==> Rebuilding panschema-viz bundle ($bundle_profile)"
wasm-pack build panschema-viz --target web "$bundle_profile" --features webgpu

echo "==> Installing panschema"
cargo install --path panschema --force "${install_flags[@]}"
