# Schema Package Manager - Implementation Plan

**Feature:** Schema Package Manager (panschema v0.3.0 milestone)

**User Story:** As a developer building tools that consume LinkML schemas, I want to declare schema dependencies in a manifest, fetch them reproducibly with a lockfile, and run codegen against them — so my project can depend on a versioned schema the way a Rust crate depends on a library.

**Related ADR (if applicable):** None

**Approach:** Vertical Slicing with Outside-In TDD

---

## Strategic Differentiation

No tool in the LinkML ecosystem provides schema-level package management today. LinkML schemas are consumed by reference (paths, URLs, git submodules) and downstream projects roll their own fetching, version pinning, and codegen orchestration.

panschema fills this gap with a `cargo`-style workflow:

- **Manifest + lockfile:** declarative dependencies, deterministic resolution
- **Publishing standard:** authoritative metadata read from each schema repo, not guessed from convention
- **Source protocols:** local paths, git tags, room for `gitlab:`, `zenodo:`, `https:` later
- **Codegen orchestration:** one tool walks the manifest and runs all configured writers across all fetched schemas

Solving schema management positions panschema as *the* LinkML ecosystem tool, not just "the Rust generator."

---

## Why Now

This milestone unblocks downstream consumers — most directly **t2t**, whose Ch 2 Phase 3 forward depends on the schema-manager workflow being available. The original plan to use git submodules was rejected as creating a "tool maturation arc" in t2t's narrative; the book teaches `panschema add` / `fetch` / `generate` from Phase 3 forward.

This is also the foundation for `scimantic-schema` to function as panschema's flagship LinkML dogfood, providing deterministic TTL, version-info round-trip, SHACL writer, Rust types writer, validation API.

---

## Architecture Overview

Three artifacts define the workflow:

```
┌──────────────────────────┐    ┌──────────────────────────┐
│ Schema repo              │    │ Consumer project         │
│ (e.g. scimantic-schema)  │    │ (e.g. t2t)               │
├──────────────────────────┤    ├──────────────────────────┤
│ schema/scimantic.yaml    │    │ panschema.toml           │
│ panschema-publish.toml   │    │ panschema.lock           │
│ (authoritative metadata) │    │ (resolved + checksums)   │
└────────────┬─────────────┘    │ src/generated/...        │
             │                  │ (writer outputs)         │
             ▼                  └────────────┬─────────────┘
      ┌──────────────┐                       │
      │ Local cache  │◀──────────────────────┘
      │ ~/.cache/    │   panschema fetch / generate
      │ panschema/   │
      └──────────────┘
```

**Key files:**

| File | Lives in | Authored by | Purpose |
|---|---|---|---|
| `panschema-publish.toml` | Schema repo root | Schema author | Declares what's in the schema (name, version, files, LinkML target version). Read at fetch time. |
| `panschema.toml` | Consumer project root | Consumer author | Declares schema dependencies and per-schema codegen config. |
| `panschema.lock` | Consumer project root | panschema | Pinned revisions + SHA-256 checksums. Committed alongside `panschema.toml`. |

**Source protocols (v0.3 minimum):** `path:` (local file/directory), `github:owner/repo@version` (tagged commit). Other protocols (`gitlab:`, `zenodo:`, `https:`, `pypi:`) deferred.

**Commands:** `init`, `add`, `fetch`, `generate`, `verify`, `release`. Existing `panschema generate --input <file>` continues to work as a no-manifest shorthand. `init` and `release` are producer-side; `add`, `fetch`, `verify`, `generate` are consumer-side.

**Cache:** `~/.cache/panschema/<source-hash>/<version>/` — XDG-compliant, shared across projects (cargo-style), no auto-eviction in v0.3.

---

## Vertical Slices

Each slice delivers end-to-end user value: a complete `manifest → fetch → generate` flow against a real schema, broadening one capability axis per slice.

### Slice 1: Local-path manifest (walking skeleton)

**Status:** ✅ Completed

**User Value:** A consumer can declare a local schema in a manifest and run codegen against it through panschema's manager workflow — no more `--input <file>` for the manifest-aware path.

**Acceptance Criteria:**

- [x] `panschema-publish.toml` parser covering `[schema]` (name, version, linkml) and `[files]` (main)
- [x] `panschema.toml` parser covering `[schemas]` (with `path:` source) and `[generate.<name>]` (writer-output mapping)
- [x] `panschema.toml` placement: discovered by walking up from CWD (cargo-style)
- [x] `panschema generate` walks the manifest, resolves each `path:` source, runs the configured writers, and errors clearly when the `path:` target doesn't exist
- [x] Existing `panschema generate --input <file>` continues to work as a no-manifest shorthand (backward compatibility)
- [x] At least one writer wired through the new pipeline (HtmlWriter — already exists, no new generation code on the critical path)
- [x] Integration test: a fixture consumer project with a `panschema.toml` pointing at a fixture schema, full `generate` produces expected output

**Notes:**
- No lockfile in this slice — slice 2 adds it.
- No remote sources — slice 3 adds them.
- No caching — local packages resolve directly via filesystem.
- Every schema dependency — `path:` or `github:` — is a "package":
  a directory containing `panschema-publish.toml` plus the main
  schema file it references at `[files].main`. Name + version come
  from the publish file. (This was finalized during slice 4; slice 1
  initially shipped a "raw schema file" path source which was
  retrofitted to the unified shape before v0.3 left development.)

### Slice 2: Lockfile + verify

**Status:** ✅ Completed

**User Value:** Builds become reproducible. `panschema fetch` records exact revisions and checksums in `panschema.lock`; `panschema verify` errors on drift; CI can guarantee the schemas it built against haven't changed.

**Acceptance Criteria:**

- [x] `panschema fetch` resolves all manifested schemas, computes SHA-256 of each schema's main file, writes `panschema.lock` with one entry per schema
- [x] `panschema verify` reads the lockfile and re-checksums each schema; errors with a clear diff when checksums disagree
- [x] `panschema generate` runs independently against the manifest (resolves fresh); doesn't require a lockfile
- [x] Lockfile format includes: name, version (from publish.toml — populated for both source types), source spec, revision (commit SHA for `github:` sources, `None` for `path:`), checksum
- [x] Local-path schemas are checksummed too — detects "schema edited but generate not re-run"
- [x] Integration test: edit a fixture schema's content after `fetch`, expect `verify` to fail

**Notes:**
- Reproducibility ratchet: once this slice ships, every consumer can pin and verify.
- File-locking on the cache deferred (no cache yet — slice 3 adds caching and concurrency concerns together).
- `panschema generate` does not read the lockfile (see AC #3) — the lockfile is verification metadata, not a source of inputs. Avoids double-resolution.
- Drift detection covers both checksum (main file content) and version (publish.toml's `[schema].version`) — a maintainer who bumps the version without re-fetching will get a clear `verify` error.

### Slice 3: `github:` source + cache

**Status:** ✅ Completed

**User Value:** Schemas can live in their own repos and be consumed by tag. Cross-project consumers share a global cache, cargo-style.

**Acceptance Criteria:**

- [x] `github:owner/repo` source protocol implemented
- [x] Fetch downloads the tagged commit's tarball from `codeload.github.com` (anonymous; avoids 60/hour API rate limit) — the AC originally named `raw.githubusercontent.com`, which only serves individual files; `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/<tag>` is the correct anonymous tarball endpoint
- [x] Tag resolution: `version = "0.1.3"` → fetches `v0.1.3` tag
- [x] `panschema-publish.toml` is read from the tagged commit; verifies its declared `version` matches the manifest
- [x] Cache populated at `~/.cache/panschema/github/<owner>/<repo>/<version>/` (hierarchical, not `<source-hash>/`) via the `directories` crate — owner/repo already provides the necessary uniqueness within github sources
- [x] Re-fetch is a no-op when the cached version is already extracted (no network call)
- [x] Lockfile entries for `github:` sources record the resolved revision — the commit SHA is extracted from the tarball's top-level directory name `<owner>-<repo>-<sha>`, no separate GitHub API call needed
- [x] File locking on the cache (`fs2`) so two `fetch` invocations against the same cache don't race
- [x] Symlink hygiene: refuses to follow paths that escape the extracted directory
- [x] Errors fast and clearly when:
  - Tag doesn't exist (`TarballFetchError::TagNotFound`)
  - `panschema-publish.toml` is absent at the tagged commit (`ResolveError::PublishMissing`)
  - `version` in the publish file disagrees with the manifest (`ResolveError::VersionMismatch`)
- [x] Integration tests (lib-level): happy path, version mismatch, missing publish file, cache hit, parent-dir traversal rejection

**Notes:**
- Other protocols (`gitlab:`, `zenodo:`, `https:`, `pypi:`) deferred to later releases
- Authenticated tarball fetch deferred — codeload is anonymous, and the `panschema-publish.toml` version check covers integrity. Private repos aren't a v0.3 goal.

### Slice 4: `panschema add`

**Status:** ✅ Completed

**User Value:** UX shorthand for the manifest. A single command replaces "edit manifest" + "run fetch" + (optionally) "edit `[generate]` config":

```
panschema add github:padamson/scimantic-schema@0.1.3
panschema add ./local-pkg
panschema add ./local-pkg --name custom-alias
```

The schema name is read from `panschema-publish.toml` at the resolved location — no duplicate typing, no name/source mismatch.

**Acceptance Criteria:**

- [x] Single positional spec: `<protocol>:<args>@<version>` (remote) or a filesystem path to a package directory. Parsed by clap via `FromStr` on `SchemaSpec`, so malformed input errors at parse time.
- [x] Schema name inferred from `panschema-publish.toml`; `--name <alias>` overrides for local renaming.
- [x] Path-source `path` field stored as a directory, canonicalized then re-relativized to the manifest's location.
- [x] Fetches the new schema and updates the lockfile (delegates to slice 2's `fetch`).
- [x] Writes a starter `[generate.<name>]` block by default (suppressed with `--no-generate-config`).
- [x] Idempotent: same shape is a no-op; different version → `AddError::VersionMismatch`; different source → `AddError::SourceMismatch`. A separate `update` command (out of scope for v0.3) handles the version-bump case.
- [x] Errors fast on invalid spec (missing version for remote, unknown protocol, missing manifest, missing `panschema-publish.toml` at the target).
- [x] CLI integration tests for happy path (path), `--name` alias, idempotency, missing-version error, unknown-protocol error, `--no-generate-config`.
- [x] Manifest edits via `toml_edit` so comments and whitespace survive.

**Notes:**
- Largely a TOML editor backed by the slice 1–3 machinery; small surface area.
- A `panschema update` (version bump) is out of scope for v0.3

### Slice 4.5: `panschema init`

**Status:** ✅ Completed

**User Value:** Producer-side counterpart to `panschema add` — `panschema init` scaffolds `panschema-publish.toml` instead of asking the schema author to hand-write the TOML. Documentation can lead with a command rather than a file template.

```
panschema init --name X --version 0.1.0 --main schema.yaml
panschema init --from path/to/schema.yaml       # extract name + version from a LinkML file
panschema init                                   # CWD basename + safe defaults
```

**Acceptance Criteria:**

- [x] Writes `panschema-publish.toml` in CWD using the resolved (name, version, main, linkml) tuple.
- [x] Argument precedence: explicit flags > values extracted via `--from <linkml-file>` > defaults (CWD basename, `0.1.0`, `schema.yaml`, `1.7.0`).
- [x] `--from <file>` reads the LinkML file via the format registry and pulls `SchemaDefinition.name` + `.version`; `--main` defaults to the path passed.
- [x] Refuses to overwrite an existing `panschema-publish.toml` unless `--force` is passed.
- [x] Post-write validation (informational): checks that `[files].main` exists and parses cleanly. Failures print a warning but don't undo the write.
- [x] Stable key order in the written file (schema → name → version → linkml → files → main). Round-trips through `PublishConfig::from_path`.
- [x] CLI integration tests for explicit args, `--from`, no-args defaults, refuse-clobber, `--force` overwrite, and missing-main-file warning.

**Notes:**
- No starter LinkML schema is created — schema authors usually already have one (use `--from`) or write one immediately after. Avoids panschema silently creating opinionated content the user didn't ask for.
- LinkML target version defaults to `1.7.0`; configurable via `--linkml`.

### Slice 4.6: `panschema release`

**Status:** Not Started

**User Value:** Producer-side counterpart to `cargo release` — one command to bump the schema's version in `panschema-publish.toml`, optionally commit + tag, optionally push. Schema authors stop hand-editing version strings and manually creating tags.

```
panschema release --level patch                  # bump publish.toml only
panschema release --level minor --git            # bump + git commit + git tag
panschema release --version 0.5.0-rc1 --git --push   # arbitrary version + push
panschema release --level patch --dry-run        # show plan, do nothing
```

**Acceptance Criteria:**

- [ ] `--level patch|minor|major` does literal semver bumps; pre-1.0 versions follow the same rule (`0.1.3 --level major` → `1.0.0`).
- [ ] `--version <x.y.z>` is an alternative to `--level` (mutually exclusive at the clap level); validates as semver before writing.
- [ ] Bump-only mode (no `--git`) edits publish.toml and prints copy-pastable git commands for the user to complete the release manually.
- [ ] `--git` runs `git add panschema-publish.toml && git commit -m 'release: v<ver>' && git tag v<ver>` after the bump.
- [ ] `--git` safety checks: refuses if git working tree is dirty, if the tag already exists, or if not in a git repo.
- [ ] `--push` (requires `--git`) additionally runs `git push --follow-tags`.
- [ ] `--dry-run` prints the plan without writing or running any git commands.
- [ ] Bump preserves comments and key order in publish.toml via `toml_edit`.
- [ ] Clear errors when `panschema-publish.toml` is missing in CWD, when the current version isn't valid semver, or when the version field is absent.
- [ ] Unit tests for `bump_version`/`set_version`; CLI integration tests for bump-only, dry-run, `--git` (with a temp git repo), dirty-tree refusal, tag-collision refusal.

**Notes:**
- Discovery is CWD-only (no walk-up) — publish.toml lives at the package root by convention.
- Commit message + tag format hardcoded for v0.3 (`release: v<ver>` and `v<ver>` respectively). Configurable later if a real need emerges.
- Pre-release suffixes (`--level rc`/`alpha`/`beta`) are out of scope; use `--version` for arbitrary versions including pre-releases.

### Slice 5: Documentation, polish, ship v0.3.0

**Status:** Not Started

**User Value:** The workflow is documented and reachable. README + dedicated guide cover authoring `panschema-publish.toml`, declaring `panschema.toml`, and running the four commands.

**Acceptance Criteria:**

- [ ] README updated with the manager workflow as the recommended path; `--input <file>` documented as a shorthand
- [ ] New section/guide in panschema's docs explaining the `panschema-publish.toml` standard so schema authors can publish their schemas
- [ ] CHANGELOG entries for each slice rolled up under `[0.3.0]`
- [ ] Release tag `v0.3.0` cuts after CI green

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Local-path manifest | Must Have | None | ✅ Completed |
| Slice 2: Lockfile + verify | Must Have | Slice 1 | ✅ Completed |
| Slice 3: `github:` source + cache | Must Have | Slice 2 | ✅ Completed |
| Slice 4: `panschema add` | Should Have | Slice 3 | ✅ Completed |
| Slice 4.5: `panschema init` | Should Have | Slice 1 | ✅ Completed |
| Slice 4.6: `panschema release` | Should Have | Slice 1 | Not Started |
| Slice 5: Documentation + ship v0.3.0 | Must Have | Slices 1–4.6 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All slice acceptance criteria are met
- [ ] All vertical slices marked as Completed
- [ ] All tests passing: `cargo nextest run`
- [ ] Integration tests cover at least: local-path-only consumer, github-source consumer, lockfile drift detection
- [ ] Library documentation complete with examples: `cargo doc`
- [ ] Code formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated under `[0.3.0]`

---

## Downstream Work (tracked separately when the time comes)

This milestone ships when the schema-manager works end-to-end against an
in-repo fixture schema. Several pieces of work in *other* repos need to
land before the dogfood loop is fully closed and t2t can adopt the
manager workflow. None of them are acceptance criteria for this milestone;
each will be handed off to another repo:

- **`scimantic-schema` adopts `panschema-publish.toml`** at the repo root.
  Without this, `panschema fetch --source github:padamson/scimantic-schema`
  errors as designed.
- **`t2t` Ch 2 Phase 3 onwards** consumes the manager workflow.
- **Writers referenced by `[generate.<name>]` blocks** — Rust types
  writer, deterministic TTL, SHACL, JSON Schema, etc. These land
  independently of the manager and don't block this milestone (slice 1
  uses HtmlWriter to prove the pipeline). Each is its own feature doc
  when picked up.

---

## Out of Scope (deferred past v0.3)

- Source protocols beyond `path:` and `github:` (`gitlab:`, `zenodo:`, `https:`, `pypi:` etc.)
- Transitive schema imports (a schema's `panschema-publish.toml` declaring `imports = [...]`)
- Cache eviction / GC (`panschema cache clean` is fine; auto-GC is not)
- `panschema update` (version bump command)
- Workspace-level manifests (one `panschema.toml` shared across Cargo workspace members) — current plan: single root manifest works, no special handling needed

---

## Open Questions (resolve during implementation)

1. **`[generate.<schema>]` location**: bundled in `panschema.toml` or split into a separate `panschema-codegen.toml`? Bundled for v0.3; revisit if it bloats.
2. **Path-source verification semantics**: track local-path file checksums in the lockfile? Yes — detects "edited but not regenerated."
3. **Rust types writer**: a parallel workstream feeds writers (Rust types, deterministic TTL, SHACL, JSON Schema) which are the things the `[generate.<name>]` blocks reference. Slice 1 wires HtmlWriter through the pipeline so the manager work isn't blocked on writer development; the writers land independently and the manifest config grows to reference them as they ship.

---

## References

- [cargo's manifest format](https://doc.rust-lang.org/cargo/reference/manifest.html) — closest prior art for TOML-driven dependency declaration

---

## Implementation Log

### 2026-05-10: Slice 1 Complete (Local-path manifest, walking skeleton)

**Completed:**
- `publish` module — parser for `panschema-publish.toml` (`PublishConfig`, `SchemaInfo`, `FileMapping`)
- `manifest` module — parser for `panschema.toml` (`Manifest`, `SchemaDep`, `GenerateConfig`) with `deny_unknown_fields` so unsupported keys fail-fast
- `manifest::discover_manifest` walks up from CWD, cargo-style
- `panschema generate` (no `--input`) discovers the manifest and runs HtmlWriter for each `[generate.<name>]` block; resolves paths relative to the manifest's location
- Clear error when a `path:` target doesn't exist
- Integration tests: happy path (manifest → fixture schema → HTML output) and the missing-path error

**Design decisions:**
- `panschema-publish.toml` validation deferred to Slice 3 — authoritative remote metadata only matters for `github:` sources where you can't trust the file path alone. `path:` sources are single-file friendly (no need to author a publish file alongside the schema).
- `[schemas].<name>` entries use `deny_unknown_fields` even though it'll need relaxing in Slice 3 (when `version` and `source` join `path`). The fail-fast cost in this slice is intentional — better to surface unsupported fields than silently ignore them.
- Implemented `FromStr` rather than a hand-rolled `from_str` method to play nicely with `clippy::should_implement_trait`.

**Next:** Slice 2 (Lockfile + verify).

### 2026-05-10: Slice 2 Complete (Lockfile + verify)

**Completed:**
- `lockfile` module — `Lockfile`/`LockEntry` types serializing as TOML with `[[schema]]` array entries, `checksum_file` helper computing `sha256:<hex>`, `path_source_spec` for stable lockfile source strings, `Lockfile::entry` lookup
- `panschema fetch` resolves every manifested schema, computes SHA-256, writes `panschema.lock` next to the manifest
- `panschema verify` re-checksums against the lockfile and errors with a per-schema diff on drift (also surfaces stale lockfile-only entries and manifest-only entries that haven't been fetched)
- Integration tests: happy path (fetch → verify succeeds), drift detection (edit schema after fetch, verify fails), and missing-lockfile error

**Design decisions:**
- `version` and `revision` lockfile fields are `Option<String>`; both are `None` for `path:` sources in this slice. They become populated for `github:` sources in slice 3.
- `panschema generate` does not read the lockfile — generate runs against the manifest, lockfile is verification metadata. Avoids double-resolution and keeps the slice 1 generate path unchanged.
- Source spec format is `path:<rel>` to mirror slice 3's `github:owner/repo` shape; one-line parser when slice 3 dispatches by prefix.
- Refactored `main.rs` to share `load_manifest()` and `resolve_path_source()` between the three manifest-driven commands.

**Next:** Slice 3 (`github:` source + cache).

### 2026-05-10: Slice 3 Complete (`github:` source + cache)

**Completed:**
- New `source` module — `SchemaSource` enum (`Path` / `Github`) with `from_dep` semantic validation; `TarballSource` trait + `CodeloadGithubSource` impl (ureq → `codeload.github.com`); `resolve_github` end-to-end (cache populate → publish-version verify → main-file path resolution); `Resolved { schema_path, revision }` shared with `path:` sources
- New `cache` module — `cache_root()` via `directories`, `github_version_dir`, `extract_tarball` (rejects absolute/`..`/multi-top-level entries), `populate_cache` with `fs2` exclusive lock on `<version>/.lock`, `validate_within` for symlink hygiene, `LocalTarballFixture` for test injection
- `Manifest::SchemaDep` extended with optional `source` and `version` fields (keeps `deny_unknown_fields`); semantic mutual-exclusion validation moved to `source` module
- `main.rs::resolve_source` dispatches on `SchemaSource` kind; `fetch_from_manifest` writes `version` + `revision` for `github:` entries in `panschema.lock`
- 12 new tests: 9 unit tests for source-spec validation + 4 end-to-end `resolve_github` tests + 4 cache module tests (extraction, locking, idempotency, parent-dir-traversal rejection via raw tar bytes)

**Design decisions:**
- Tarball via `codeload.github.com` rather than individual files via `raw.githubusercontent.com` — the latter doesn't carry the commit SHA, requiring a separate API call that would burn the 60/hr anonymous limit. The tarball's top-level dir name `<owner>-<repo>-<sha>` gives us the SHA for free.
- Cache layout `~/.cache/panschema/github/<owner>/<repo>/<version>/` — hierarchical, not `<source-hash>/`. The hash adds no value when owner/repo already uniquely namespaces github sources. Cargo uses a hash for crate-name disambiguation across registries; we don't have that problem.
- `TarballSource` trait for DI rather than env-var injection — naturally extends to future protocols (`gitlab:`, `https:`) as additional trait impls. Production uses `CodeloadGithubSource`; tests pass `LocalTarballFixture`.
- Re-fetch no-op: `populate_cache` short-circuits if `<owner>-<repo>-<sha>/` already exists in the version directory — no network call, no extraction. Test asserts this by mutating the fixture between calls and checking the SHA is unchanged.
- License-allow-list grew by `CDLA-Permissive-2.0` (used by webpki-roots, the Mozilla root CA store shipped with rustls/ureq).

**Next:** Slice 4 (`panschema add` command).

### 2026-05-11: Slice 4 Complete (`panschema add` + slice 1 unification)

While wiring `panschema add`, we found that the original slice 1
design (path sources are raw schema files; name comes from the
manifest key) created an unprincipled split with slice 3 (github
sources are full packages with `panschema-publish.toml`). The
asymmetry showed up in the CLI: github needed
`<name>@<version> --source <uri>`, path needed `<name> --path <file>`,
and the name was typed twice in the github case. We collapsed the
bifurcation while still pre-1.0 — every schema dependency is now a
"package" anchored by `panschema-publish.toml`, regardless of where
it lives.

**Completed:**
- **New types in `manifest`:**
  - `SchemaSpec` enum (`Source { uri, version } | Path(PathBuf)`)
    with a `FromStr` impl. Clap parses the positional CLI arg via
    `FromStr`, so malformed input (missing version, unknown
    protocol, empty spec) surfaces as a parse error before any
    handler runs.
  - `AddRequest` enum (`Path | Remote`) — validated post-CLI
    request, both variants carrying the *final* manifest key
    (inferred from publish.toml or supplied via `--name <alias>`).
  - `insert_schema(manifest_path, &AddRequest, with_generate_block)`
    — `toml_edit`-backed mutator that preserves comments and key
    order; returns `AddOutcome::{Inserted, AlreadyPresent}` for
    clean idempotency signalling.
  - `AddError::{VersionMismatch, SourceMismatch}` — surface
    conflicts loudly instead of overwriting.
- **Shared package opener in `source`:**
  - `open_package(name, pkg)` reads `panschema-publish.toml` from a
    package directory (or accepts the file path directly),
    canonicalizes for symlink hygiene, returns the parsed
    `PublishConfig`. Both `resolve_path` and `resolve_github` use it.
  - `Resolved.version` is always populated from publish.toml; both
    source types now write it into `LockEntry.version`.
- **CLI:**
  - `panschema add <spec> [--name <alias>] [--no-generate-config]`
    — positional spec, optional name override, optional
    suppression of the starter `[generate.<name>]` block.
  - Path-source input is canonicalized then re-relativized to the
    manifest's directory before storing — robust against the user
    typing the path from a different CWD than the manifest.
- **Drift detection upgrade:** `verify` now catches publish.toml
  version drift in addition to checksum drift (because version is
  always recorded in the lockfile).
- **Fixtures + tests:**
  - `tests/fixtures/local-pkg/` (publish.toml + sample yaml) is the
    new path-source fixture. Replaces direct references to
    `sample_schema.yaml` throughout the integration suite.
  - Lib-level: 7 new `SchemaSpec` parse tests + 6 `insert_schema`
    tests in `manifest.rs`.
  - CLI integration: path-source happy path, `--name` alias,
    idempotency, missing-version error, unknown-protocol error,
    `--no-generate-config`, missing-publish-toml error. Plus all
    nine pre-existing path-source tests rewritten to the new
    package shape.
  - Manual smoke: `panschema add ./local-pkg` → manifest entry +
    lockfile, `verify` succeeds, mutate schema file → `verify`
    fails with a clean drift error.

**Design decisions:**
- **Source-spec-as-positional** rather than `<name>@<version> --source <uri>`.
  Eliminates duplicate typing of the name; the name was always
  redundant on the CLI side once we accepted `panschema-publish.toml`
  as authoritative metadata.
- **Path-target stored as the package directory** (cargo / npm
  convention), not the publish-file path. CLI accepts either form
  on input; we normalize on the way to the manifest.
- **Single allow-list constant for protocols** (just `github:` today).
  Unknown-protocol specs like `gitlab:foo/bar@0.1.0` error fast at
  `SchemaSpec::from_str` — a typo in `github:` doesn't silently land
  as a filesystem path.
- **`--name <alias>` skips name verification** at resolve time —
  power-user aliasing is cheap, and we trust the user who explicitly
  chose to deviate from the declared name.
- **`toml_edit` rather than serde round-trip** — preserves user
  comments and whitespace. Adds one direct dep (already in our
  transitive tree via the `toml` crate); small cost for a big
  quality-of-life win on a user-facing config file.
- **"Same shape is no-op, mismatch is error"** rather than "always
  overwrite" — keeps `panschema add` safe to script in setup flows
  without clobbering manually-customized entries.
- **Lib-level `insert_schema()`** instead of inlining the toml_edit
  dance in `main.rs` — unit-testable without spawning the CLI
  binary, and reusable when `panschema update` lands.
- **Always re-fetch after add** (no `--no-fetch` flag) — the cargo
  `add`-then-`build` mental model. Idempotent `add` still re-verifies.

**Next:** Slice 4.5 (`panschema init`) + Slice 4.6 (`panschema release`).

### 2026-05-11: Slice 4.5 Complete (`panschema init`)

Producer-side scaffolding command. Symmetric to `panschema add` on
the consumer side — schema authors run one command instead of
hand-writing the publish file.

**Completed:**
- `publish::init_publish_file(dir, name, version, main, linkml, force)`
  writes a hand-formatted `panschema-publish.toml` with stable key
  order; refuses to clobber unless `force=true`. Round-trips
  through `PublishConfig::from_path`.
- New `PublishError::AlreadyExists { dir }` variant.
- `Init` clap subcommand with `--name`, `--version`, `--main`,
  `--linkml`, `--from`, `--force` flags.
- `main.rs::init_schema_package` resolves args by precedence
  (explicit > `--from` extracted > defaults) and runs post-write
  validation: warns if the main file doesn't exist or doesn't
  parse cleanly, but still writes the publish file.
- 4 new unit tests in `publish.rs` (round-trip, refuse-clobber,
  `--force` overwrite, stable key order) + 6 new CLI integration
  tests (explicit args, `--from`, no-args defaults, refuse-clobber,
  `--force`, missing-main-file warning).
- Test count: 260 (up from 250).

**Design decisions:**
- **No starter LinkML schema** when `init` runs with no `--from`.
  Schema authors usually have a schema already (use `--from`) or
  will write one immediately after; we don't want to silently
  create opinionated content the user didn't ask for. Asymmetric
  with `cargo init`'s `src/lib.rs`, but the LinkML metamodel is
  too rich to have one obvious skeleton.
- **Defaults are CWD-derived where possible** (name = basename of
  CWD) — matches `cargo init` ergonomics for the bare-args case.
- **Hand-formatted TOML** rather than serializing `PublishConfig`
  via serde. Stable key order matters for a user-facing config
  file, and the layout differences (e.g., blank line between
  `[schema]` and `[files]`) would be lost on a round-trip.
- **`--from <file>` uses the existing format registry**, so it
  works for any LinkML-readable file (YAML today, OWL/Turtle
  too). No special-case parsing.

**Next:** Slice 4.6 (`panschema release`).
