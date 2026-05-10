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

**Commands:** `add`, `fetch`, `generate`, `verify`. Existing `panschema generate --input <file>` becomes a no-manifest shorthand.

**Cache:** `~/.cache/panschema/<source-hash>/<version>/` — XDG-compliant, shared across projects (cargo-style), no auto-eviction in v0.3.

---

## Vertical Slices

Each slice delivers end-to-end user value: a complete `manifest → fetch → generate` flow against a real schema, broadening one capability axis per slice.

### Slice 1: Local-path manifest (walking skeleton)

**Status:** Not Started

**User Value:** A consumer can declare a local schema in a manifest and run codegen against it through panschema's manager workflow — no more `--input <file>` for the manifest-aware path.

**Acceptance Criteria:**

- [ ] `panschema-publish.toml` parser (serde) covering `[schema]` (name, version, linkml) and `[files]` (main)
- [ ] `panschema.toml` parser covering `[schemas]` (with `path:` source) and `[generate.<name>]` (writer-output mapping)
- [ ] `panschema.toml` placement: discovered by walking up from CWD (cargo-style)
- [ ] `panschema generate` walks the manifest, reads each schema's `panschema-publish.toml`, resolves the `path:` source, runs the configured writers, errors clearly when:
  - manifest references a schema that has no `panschema-publish.toml`
  - `panschema-publish.toml` `version` disagrees with the manifest's pinned version
  - `path:` target doesn't exist
- [ ] Existing `panschema generate --input <file>` continues to work as a no-manifest shorthand (backward compatibility)
- [ ] At least one writer wired through the new pipeline (start with HtmlWriter — already exists, no new generation code on the critical path)
- [ ] Integration test: a fixture consumer project with a `panschema.toml` pointing at a fixture schema, full `generate` produces expected output

**Notes:**
- No lockfile in this slice — slice 2 adds it
- No remote sources — slice 3 adds them
- No caching — manifest sources resolve directly via filesystem
- The point of this slice is to prove the manifest → resolver → writer pipeline end-to-end with the lowest-friction source (local path)

### Slice 2: Lockfile + verify

**Status:** Not Started

**User Value:** Builds become reproducible. `panschema fetch` records exact revisions and checksums in `panschema.lock`; `panschema verify` errors on drift; CI can guarantee the schemas it built against haven't changed.

**Acceptance Criteria:**

- [ ] `panschema fetch` resolves all manifested schemas, computes SHA-256 of each schema's main file, writes `panschema.lock` with one entry per schema
- [ ] `panschema verify` reads the lockfile and re-checksums each schema; errors with a clear diff when checksums disagree
- [ ] `panschema generate` in this slice can run after `fetch` (consumes the lockfile) or independently against the manifest (resolves fresh)
- [ ] Lockfile format includes: name, version, source spec, checksum (revision field present but `null` for path: sources in this slice)
- [ ] Local-path schemas are checksummed too — detects "schema edited but generate not re-run"
- [ ] Integration test: edit a fixture schema's content after `fetch`, expect `verify` to fail

**Notes:**
- Reproducibility ratchet: once this slice ships, every consumer can pin and verify
- File-locking on the cache deferred (no cache yet — slice 3 adds caching and concurrency concerns together)

### Slice 3: `github:` source + cache

**Status:** Not Started

**User Value:** Schemas can live in their own repos and be consumed by tag. Cross-project consumers share a global cache, cargo-style.

**Acceptance Criteria:**

- [ ] `github:owner/repo` source protocol implemented
- [ ] Fetch downloads the tagged commit's tarball from `raw.githubusercontent.com` (anonymous; avoids 60/hour API rate limit)
- [ ] Tag resolution: `version = "0.1.3"` → fetches `v0.1.3` tag
- [ ] `panschema-publish.toml` is read from the tagged commit; verifies its declared `version` matches the manifest
- [ ] Cache populated at `~/.cache/panschema/<source-hash>/<version>/` via the `directories` crate
- [ ] Re-fetch is a no-op when the cache + lockfile checksums agree
- [ ] Lockfile entries for `github:` sources record the resolved revision (commit SHA from the tag)
- [ ] File locking on the cache (e.g. `fs2`) so two `fetch` invocations against the same cache don't race
- [ ] Symlink hygiene: don't follow symlinks out of the cache when reading schema files
- [ ] Error fast and clearly when:
  - Tag doesn't exist
  - `panschema-publish.toml` is absent at the tagged commit
  - `version` in the publish file disagrees with the tag
- [ ] Integration test: a fixture consumer with a `github:` source against a real (or fixture-replayed) public schema repo

**Notes:**
- Other protocols (`gitlab:`, `zenodo:`, `https:`, `pypi:`) deferred to later releases
- `GITHUB_TOKEN` env var honored if set, but unauthenticated raw URLs are the default path

### Slice 4: `panschema add`

**Status:** Not Started

**User Value:** UX shorthand for the manifest. `panschema add scimantic-schema@0.1.3 --source github:padamson/scimantic-schema` is one command instead of three (edit manifest, fetch, optionally edit generate config).

**Acceptance Criteria:**

- [ ] `panschema add <name>@<version> --source <source>` appends a new entry to `[schemas]` in `panschema.toml`
- [ ] Fetches the new schema (delegates to slice 2's `fetch`) and updates the lockfile
- [ ] Optionally writes a starter `[generate.<name>]` block (configurable, e.g. `--no-generate-config`)
- [ ] Idempotent: running `add` for a schema already in the manifest with the same version is a no-op; with a different version, errors with a clear message (use a separate `update` command — out of scope for v0.3)
- [ ] Errors fast on invalid source spec, missing tag, etc.
- [ ] CLI tests covering happy path + the error cases

**Notes:**
- Largely a TOML editor backed by the slice 1–3 machinery; small surface area
- A `panschema update` (version bump) is out of scope for v0.3

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
| Slice 1: Local-path manifest | Must Have | None | Not Started |
| Slice 2: Lockfile + verify | Must Have | Slice 1 | Not Started |
| Slice 3: `github:` source + cache | Must Have | Slice 2 | Not Started |
| Slice 4: `panschema add` | Should Have | Slice 3 | Not Started |
| Slice 5: Documentation + ship v0.3.0 | Must Have | Slices 1–4 | Not Started |

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

(Filled in as slices land.)
