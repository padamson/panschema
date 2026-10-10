#!/usr/bin/env bash
# Run a built panschema binary through one pass of each reader, an RDF
# writer, the HTML writer, and the validator. For a build the test suite
# never runs on, such as the static musl release, this is the evidence it
# does more than start.
#
# Usage: scripts/smoke-test-binary.sh <path-to-panschema>
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <path-to-panschema>" >&2
  exit 2
fi
bin=$1
out=$(mktemp -d "${TMPDIR:-/tmp}/panschema-smoke.XXXXXX")
trap 'rm -rf "$out"' EXIT

"$bin" generate -s panschema/tests/fixtures/reference.ttl -o "$out/html" --offline
test -s "$out/html/index.html"

"$bin" generate -s panschema-model/tests/fixtures/sample_schema.yaml -f ttl -o "$out/schema.ttl"
test -s "$out/schema.ttl"

"$bin" verify \
  -s panschema-model/tests/fixtures/catalog.yaml \
  -d panschema-model/tests/fixtures/catalog_data.yaml
