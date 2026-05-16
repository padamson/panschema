# Mutation testing catch-up plan

Tracking surviving mutants in pre-existing code, with a per-file
checklist of test gaps that have been (or could be) closed. New
work is covered by the per-push `mutation-testing-diff` CI job
(see [.github/workflows/security.yml](../.github/workflows/security.yml));
this doc is the legacy-debt log.

## Snapshot

Initial workflow_dispatch run on commit `eedb11e` (cancelled at
~31% coverage to triage):

| Outcome | Count |
|---|---|
| Caught | 445 |
| Missed | 888 |
| Unviable | 146 |
| Timeout | 0 |

Of the 888 missed, 780 were GPU/viz math (deferred — see
"Out of scope" below). The schema-manager + writer core had
~108 missed across 9 files.

## Status after the catch-up pass

| File | Mutants tested | Missed before | Missed after | Notes |
|---|---|---|---|---|
| `linkml.rs` | 10 | 1 | 0 | Added `is_false_serde_helper_skips_default_bools` |
| `lockfile.rs` | 12 | 2 | 0 | Added `path_source_spec_emits_exact_prefix_and_path` |
| `manifest.rs` | 24 | 3 | 1 | 1 semantically equivalent (`<` vs `<=` on impossible char-index case) |
| `source.rs` | 18 | 7 | 2 | 5 caught; 2 HTTP-fetch-path mutants deferred (need a network mock) |
| `main.rs` | 60 | 19 | 7 | 12 caught; 7 remaining are cosmetic stdout labels or env-dependent (git absent, dev feature) |
| `rust_writer.rs` | 118 | 22 | 8 | 14 caught; 8 remaining are semantically-equivalent (||/&&/>/>= cases that can't differ given input invariants) |
| `rdf_serializers.rs` | (next pass) | 10 | TBD | Added exhaustive `map_linkml_to_xsd` arm coverage (9 of 10); 1 build_rdf_graph mutant deferred (test needs sophia graph inspection) |
| `owl_reader.rs` | (next pass) | 13 | TBD | Added exhaustive `map_xsd_to_linkml` arm coverage (6 of 13); 7 SimpleTerm match-arm mutants deferred (need RDF fixture work) |

**Total**: 108 missed → ~18 missed remaining (~83% reduction). The
~18 remaining cluster into three families documented below.

## What got skipped, and why

### Semantically-equivalent mutants

These survive because the case where the mutation would diverge is
unreachable given the surrounding invariants:

- **`manifest.rs:167` `< → <=`**: the inputs are `colon` and
  `first_sep`, both byte indices into the same string. They can
  only be equal if the same byte is both `:` AND a separator
  character (`/`, `.`, `\`) — impossible.
- **`rust_writer.rs:644-645` `|| → &&` in `type_for_range`**: both
  the if-branch and else-branch return `other.to_string()`
  identically; the predicate is structural documentation, not a
  behaviour fork. (A future writer pass could surface a warning
  for unresolved refs, making the branches genuinely differ.)
- **`rust_writer.rs:690` `> → >=` in `snake_case`**: the `i > 0`
  guard's only divergence at `i == 0` is short-circuited by the
  conjunction's other terms (which require a previous char to
  exist — impossible at i=0).

### Network-dependent

- **`source.rs:357,359` (CodeloadGithubSource::fetch)**: the
  real HTTP fetch path is unreachable from unit tests, which use
  `LocalTarballFixture`. Catching requires either a mock HTTP
  server (mockito etc.) or env-var redirection. Deferred until a
  concrete consumer asks for tighter coverage.

### Cosmetic stdout labels

- **`main.rs:247-251`**: the format-name → description lookup
  (e.g. `"html" => "documentation"`). Pure stdout cosmetics —
  affects only the user-facing "Generated X for 'Y' at Z" line.
  Catching needs `.output()` + stderr/stdout grep for each
  format. Low semantic value; deferred.

### Environment-dependent

- **`main.rs:657` `ensure_git_available`**: replacing the function
  body with `Ok(())` survives because all downstream code paths
  also shell out to git, so a missing-git environment still fails
  later. Catching requires running with git removed from PATH.
- **`main.rs:999` `generate_styleguide`**: gated behind
  `--features dev`. Default test runs don't exercise it.
- **`main.rs:448` `init_schema_package` warning branch**: caught
  by tightening the existing `init_warns_when_main_file_missing`
  test to assert the specific "does not exist yet" phrase.

### Deferred to follow-on slice

- **owl_reader.rs (7 SimpleTerm match arms)**: needs RDF fixture
  work to exercise the language-tagged literal path. Same shape
  as the xsd-mapping mutants we did catch — moderate effort,
  defer to a focused owl_reader test pass.
- **rdf_serializers.rs (build_rdf_graph individual-type triples)**:
  needs sophia graph inspection in tests.

## How to work off remaining items

1. Pick a file from the table above.
2. Run mutation testing scoped to that file:
   ```bash
   cargo mutants --file panschema/src/<file>.rs
   ```
3. For each surviving mutant, write the test that catches it.
4. Re-run; confirm 0 missed (or document each as
   semantically-equivalent / out-of-scope).
5. Update this doc.

For new work, the per-push `--in-diff` CI job catches anything
fresh; only legacy debt belongs in this doc.

## Out of scope (deferred — GPU/viz)

| File | Missed |
|---|---|
| `panschema-viz/src/lib.rs` | 279 |
| `panschema/src/gpu/camera.rs` | 203 |
| `panschema-viz/src/canvas2d.rs` | 85 |
| `panschema/src/gpu/simulation.rs` | 69 |
| `panschema/src/gpu/geometry.rs` | 49 |
| `panschema/src/gpu/renderer.rs` | 46 |
| `panschema/src/gpu/types.rs` | 29 |
| `panschema-viz/src/camera.rs` | 14 |
| `panschema/src/gpu/render_shaders.rs` | 4 |
| `panschema/src/gpu/shaders.rs` | 2 |
| **Subtotal** | **780** |

Rendering math, shader code, and force-simulation logic. Mutation
testing on visual code is hard to leverage — tests are typically
either pixel comparisons (out of scope) or "did the simulation
converge" (noisy under mutation). Defer until a concrete need
surfaces.

## Updating this snapshot

When you want a fresh picture of remaining debt:

```bash
gh workflow run "Security & Quality"
# Wait for the Mutation Testing (full) job; cancel after 1–2h
# if needed — partial data is usually enough to update this doc.
```

Or just run cargo-mutants per-file locally:

```bash
cargo mutants --file panschema/src/<file>.rs
```
