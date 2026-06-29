#!/usr/bin/env bash
#
# Vendor a released dogfood schema as a checked-in regression fixture.
#
# This script is run BY HAND when a dogfood schema cuts a new release, and
# its output is committed explicitly. It is the only network path in this
# feature: the test suite reads the vendored snapshots offline. The weekly
# release-monitor workflow opens an issue when a release is missing a
# snapshot, prompting a maintainer to run this and commit the result.
#
# Usage:
#   scripts/vendor-dogfood-schemas.sh <repo> <tag>
#   scripts/vendor-dogfood-schemas.sh <repo> all
#
#   <repo>  dogfood schema repo under github.com/padamson (e.g. scimantic-schema)
#   <tag>   release tag to vendor (e.g. v0.2.0); `all` discovers every tag via
#           `gh api repos/padamson/<repo>/tags`.
#
# Each snapshot lands at tests/fixtures/dogfood/<repo>/<tag>.yaml with a
# header naming the source repo + tag. Re-running is idempotent: an existing
# snapshot is overwritten in place from a fresh fetch.
#
# Mechanism: `panschema add github:padamson/<repo>@<version>` fetches the repo
# tarball into a cache and resolves the schema file via the repo's
# panschema-publish.toml, so this script does not hardcode each repo's schema
# path — it reads `[files].main` from the cached publish manifest.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE_ROOT="$REPO_ROOT/panschema/tests/fixtures/dogfood"
OWNER="padamson"

usage() {
  echo "usage: $0 <repo> <tag|all>" >&2
  exit 2
}

[ "$#" -eq 2 ] || usage
REPO="$1"
TAG_ARG="$2"

# `panschema add` takes the version without a leading `v`; the fixture file
# keeps the tag verbatim (e.g. v0.2.0.yaml).
add_version() { echo "${1#v}"; }

# Build the panschema binary once so `all` reuses it across tags.
echo "Building panschema (release)..." >&2
( cd "$REPO_ROOT" && cargo build --release --quiet --bin panschema )
PANSCHEMA="$REPO_ROOT/target/release/panschema"

vendor_one() {
  local tag="$1"
  local version
  version="$(add_version "$tag")"
  local out="$FIXTURE_ROOT/$REPO/$tag.yaml"

  echo "Vendoring $REPO@$tag -> $out" >&2

  local cache consumer
  cache="$(mktemp -d)"
  consumer="$(mktemp -d)"
  printf '[schemas]\n' > "$consumer/panschema.toml"

  (
    cd "$consumer"
    PANSCHEMA_CACHE_ROOT="$cache" "$PANSCHEMA" add "github:$OWNER/$REPO@$version" >/dev/null
  )

  local pkg_dir="$cache/github/$OWNER/$REPO/$version/$REPO-$version"
  local publish="$pkg_dir/panschema-publish.toml"
  if [ ! -f "$publish" ]; then
    echo "error: no panschema-publish.toml in cached $REPO@$tag at $pkg_dir" >&2
    rm -rf "$cache" "$consumer"
    return 1
  fi

  # Read `[files].main` from the publish manifest to locate the schema in the
  # tarball without hardcoding each repo's layout.
  local main_rel
  main_rel="$(sed -n 's/^[[:space:]]*main[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$publish" | head -1)"
  if [ -z "$main_rel" ]; then
    echo "error: panschema-publish.toml for $REPO@$tag has no [files].main" >&2
    rm -rf "$cache" "$consumer"
    return 1
  fi

  local schema_src="$pkg_dir/$main_rel"
  if [ ! -f "$schema_src" ]; then
    echo "error: resolved schema not found at $schema_src" >&2
    rm -rf "$cache" "$consumer"
    return 1
  fi

  mkdir -p "$FIXTURE_ROOT/$REPO"
  {
    echo "# Frozen vendored snapshot of $REPO $tag"
    echo "# (github:$OWNER/$REPO@$tag), checked in so the real-world"
    echo "# regression tests run self-contained with no network fetch."
    echo "# Do not hand-edit: to refresh, re-run"
    echo "#   scripts/vendor-dogfood-schemas.sh $REPO $tag"
    echo "# and commit the result."
    cat "$schema_src"
  } > "$out"

  rm -rf "$cache" "$consumer"
  echo "Wrote $out" >&2
}

if [ "$TAG_ARG" = "all" ]; then
  command -v gh >/dev/null || { echo "error: 'all' needs the gh CLI" >&2; exit 1; }
  mapfile -t TAGS < <(gh api "repos/$OWNER/$REPO/tags" --jq '.[].name')
  if [ "${#TAGS[@]}" -eq 0 ]; then
    echo "No tags found for $OWNER/$REPO; nothing to vendor." >&2
    exit 0
  fi
  for tag in "${TAGS[@]}"; do
    vendor_one "$tag"
  done
else
  vendor_one "$TAG_ARG"
fi

echo "Done. Review and commit the vendored fixture(s)." >&2
