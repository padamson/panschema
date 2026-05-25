# Feature: Versioned Docs (`panschema publish`)

**Feature:** Multi-version HTML doc orchestration + in-page version UX

**User Story:** As a LinkML schema author whose schema evolves through multiple released versions, I want `panschema` to render a single deployable site that hosts every released version's docs side-by-side (`/schema/v0.1.0/`, `/schema/v0.2.0/`, `/schema/main/`, `/schema/current/`), with a version dropdown and a "you're viewing X; current is Y" banner inside each rendered page, so my consumers can navigate across versions without my CI carrying the orchestration logic.

**Related ADR:** None yet (likely needs one if alternative orchestration models surface — "ADR: Where versioned-docs orchestration lives (panschema vs consumer CI)")

**Approach:** Vertical Slicing with Outside-In TDD. Slice 1 lands the manifest section + validation alone — no orchestration, no template work — so the wire format stabilizes before downstream slices commit to it. Subsequent slices add extraction, template injection, the `current/` alias, and the consumer dogfood.

---

## Context

scimantic-schema is starting a v0.2.0 → v0.3.0 ground-up rebuild (N&M-101 adapted to LinkML, documented as an mdbook). For the rebuild to be usable to readers and collaborators, the rendered HTML schema docs need to be **versioned** — readers landing on the site need to be able to see how `Question` was defined in v0.1.0 vs. v0.2.0 vs. `main`, with a UI affordance to switch versions inside the page.

The user explicitly chose to put both the orchestration and the in-page UX in panschema, not in consumer CI. The pattern matches every mature multi-version doc tool: [mike](https://github.com/jimporter/mike) for MkDocs, Docusaurus versioning, sphinx-multiversion, Read the Docs. The doc tool (or its plugin) owns BOTH the orchestration AND the cross-version theme integration; consumer CI configures what to publish, the tool does the rest. Pushing orchestration into consumer CI scatters the same `for tag in ...` loop across every LinkML+panschema schema repo.

A per-call `--version-context` flag (Option A in the original design conversation) was considered and rejected as wrong-abstraction-layer — it makes consumer CI carry orchestration logic that belongs in panschema.

---

## Manifest extension: `[publishing]` section

Existing `panschema-publish.toml` (scimantic-schema v0.2.0 example):

```toml
[schema]
name = "scimantic"
version = "0.2.0"
linkml = "1.7.0"

[files]
main = "schema/scimantic.yaml"
```

Proposed addition:

```toml
[publishing]
versions = ["v0.1.0", "v0.2.0"]    # build per-version docs from these git tags
edge = "main"                       # also build HEAD of this ref (optional; null = skip)
current = "v0.2.0"                  # alias `current/` to this version (must be in `versions` or equal to `edge`)
url_pattern = "/schema/{version}/"  # for cross-version links in the dropdown
output_dir = "site/schema"          # relative to repo root; per-version subdirs land here
format = "html"                     # what to generate per version
```

All fields under `[publishing]` are optional from a parsing perspective — presence of the section enables the versioned-publish path. If `[publishing]` is absent, `panschema generate` continues to work as today (single-version generation).

Field semantics:
- `versions` — list of git tag names. `panschema` resolves each via `git rev-parse`. Failing resolution is an error (caller bug — they listed a tag that doesn't exist).
- `edge` — a ref (branch or commit-ish); skipped when null/omitted. Typical value `"main"`. Built from a fresh extraction, not from the working tree (so the publish is reproducible regardless of dirty working state).
- `current` — the alias target. Validate it's in `versions` OR equals `edge`. Don't silently default to "latest semver" — explicit is better; users may want `current` to lag the latest tag during an in-progress release.
- `url_pattern` — uses `{version}` as the placeholder. Used in template rendering to build cross-version links.
- `output_dir` — defaults to `site/schema/` if omitted.
- `format` — defaults to `html`. Reserved for future writer fan-out.

---

## Vertical Slices

### Slice 1: `[publishing]` section parsing + validation (no orchestration)

**Status:** Completed

**User Value:** The wire format is locked in. Consumers can land the `[publishing]` section in their `panschema-publish.toml` and have it parsed cleanly; manifest tooling, schema-package-manager, and downstream tests can rely on a stable shape before the rest of the feature ships. No behavioral change to `panschema generate` yet.

**Acceptance Criteria:**
- [x] `PublishingConfig` struct in [`panschema/src/publish.rs`](../../panschema/src/publish.rs), with serde derives, parsing the fields listed above. Optional fields use `Option<T>` with `#[serde(default)]`; `url_pattern`, `output_dir`, and `format` carry custom default-fn producers so a minimal block parses with sensible defaults.
- [x] `PublishConfig::from_str` accepts a publish file with `[publishing]` present and round-trips it through serialize/deserialize.
- [x] Parse-time cross-field validation: `current` must appear in `versions` or equal `edge`. Reject with the new `PublishError::InvalidCurrent` variant whose error message names the offending value and the legal alternatives.
- [x] Tag-resolution validation deferred to slice 2 (it requires a git repo at hand, which manifest parsing doesn't).
- [x] Unit tests cover: parses absent section (returns `None`), parses minimal block (defaults populated), parses full block, accepts `current == edge` when `versions` is empty, rejects `current` that's neither in `versions` nor `== edge`, rejects empty `versions` + no `edge`, rejects missing `current`.

**Notes:**
- Naming parallels the existing `GenerateConfig` for the `[generate.<name>]` blocks in `panschema.toml`. Keeps the manifest model consistent across the two manifest files.
- Defer the `panschema publish` subcommand itself to slice 3 — slices 1–2 set up the data + extraction primitives without exposing a user-facing command.

---

### Slice 2: Per-version git extraction (no command surface yet)

**Status:** Not Started

**User Value:** The plumbing that turns a tag list into per-version schema files. Used internally by slice 3's `publish` command; lands separately so its failure modes (missing tag, missing main file at tag, extraction race) are isolated.

**Acceptance Criteria:**
- [ ] Internal helper `extract_main_at_ref(ref: &str, manifest_main: &Path) -> Result<TempFile>` that runs `git show <ref>:<path>` and returns the extracted content as a temp file path. No working-tree mutation.
- [ ] Resolve each tag in `versions` via `git rev-parse` and surface a single combined error if any fails to resolve.
- [ ] Per-tag extraction tolerates the case where `files.main` *moved* between versions — the older tag's manifest may have listed a different path; v1 of this feature reads only the current manifest's `files.main` and documents that as the contract (consumers who rename their main file need a follow-up slice).
- [ ] Tests over a fixture repo with two committed tags + a main branch, asserting extraction succeeds for both tags and surfaces a clean error for a non-existent tag.

**Notes:**
- `git show <ref>:<path>` is the right primitive: works on bare and non-bare repos, doesn't mutate the working tree, fails loudly when the path doesn't exist at that ref.
- Test fixture lives under `panschema/tests/fixtures/` as a small init-script that builds a synthetic git repo at test time; checked-in `.git` directories are awkward.

---

### Slice 3: `panschema publish` command — per-version build + `current` alias

**Status:** Not Started

**User Value:** First user-facing milestone. `panschema publish` reads the manifest, builds each version's HTML output into `output_dir/<tag>/`, copies the configured `current` version to `output_dir/current/`, and exits cleanly. No dropdown/banner in the rendered output yet — that's slice 4.

**Acceptance Criteria:**
- [ ] New subcommand `panschema publish [--manifest <path>] [--output <dir>]` wired into `main.rs`.
- [ ] Reads manifest, errors if `[publishing]` is absent.
- [ ] For each entry in `versions`: extracts via slice 2, runs the existing HTML generator against the extracted file, output to `<output_dir>/<tag>/`.
- [ ] If `edge` is set: extracts and builds to `<output_dir>/<edge-name>/` (typically `main`).
- [ ] `current/` is a `cp -r` of the `current` version's output (not a symlink — static hosts handle directories reliably; not a re-render — risk of byte divergence).
- [ ] Exit non-zero on: missing `[publishing]`, any tag failing to resolve, `current` not in `versions` and not equal to `edge`, any individual generate failing (don't continue producing partial state).
- [ ] Integration test against a fixture repo verifies: all configured outputs exist, `current/` is byte-equal to the configured version's output.

**Notes:**
- The existing HTML generator API is the right reuse target; this slice doesn't change `HtmlWriter` itself, just orchestrates it.
- The optional `<output_dir>/index.html` redirect stub deferred to slice 5 (consumer overlay).

---

### Slice 4: Template integration — version dropdown + banner

**Status:** Not Started

**User Value:** Each generated page knows which version it is and can offer in-page navigation to other versions. The "you're viewing X; current is Y" banner makes version drift visible to consumers.

**Acceptance Criteria:**
- [ ] `VersionContext` struct passed into the Askama render context, containing: `all_versions`, `viewing`, `current`, `url_pattern`.
- [ ] HTML template gets a `<select>` (or styled dropdown) in the header populated from `all_versions`, default-selected to `viewing`, with a JS handler that navigates to the url_pattern with `{version}` substituted.
- [ ] Conditional banner above main content: when `viewing != current`, render `"You're viewing <viewing>; current is <a href=...>{current}</a>"`. When `viewing == current`, render no banner (or a subtle "current" badge — picker's choice).
- [ ] Edge banner: when `viewing == edge`, render `"You're viewing the edge build from HEAD; not a released version"`.
- [ ] E2E test (playwright-rs) confirms the dropdown is present, the banner renders correctly on a non-current page, and the JS navigation produces the expected URL.

**Notes:**
- Plain `<select>` is fine for v1. Styling polish defer.
- The render context is passed by `panschema publish` only; `panschema generate` (single-version) renders without a `VersionContext` and the template branches accordingly.

---

### Slice 5: scimantic-schema integration dogfood + release

**Status:** Not Started

**User Value:** The feature is real. scimantic-schema's `.github/workflows/docs.yml` is rewritten to call `panschema publish`, and the deployed Pages site shows the full multi-version UX. This is also the release vehicle for whatever panschema version ships the feature — the [panschema release-command-gaps note](https://example.invalid) flagged that the version-bump path needs real exercise.

**Acceptance Criteria:**
- [ ] scimantic-schema's `panschema-publish.toml` gains a `[publishing]` block listing `["v0.1.0", "v0.2.0"]`, `edge = "main"`, `current = "v0.2.0"`, `url_pattern = "/schema/{version}/"`.
- [ ] scimantic-schema's `.github/workflows/docs.yml` replaces the multi-step generate with a single `panschema publish --output site/schema/`.
- [ ] scimantic-schema gets a `site/index.html` stub redirecting to `/schema/current/` (the mdbook landing-page work is a separate workstream).
- [ ] Deployed Pages site shows `/schema/v0.1.0/`, `/schema/v0.2.0/`, `/schema/main/`, `/schema/current/` rendering with a working dropdown.
- [ ] Cut a panschema release (cross-repo note 05-12 `release-command-gaps` flagged this path needs exercise; the feature's release is a fine vehicle).

**Notes:**
- This slice is the "close-out trigger" condition from the cross-repo note. Once it lands, the source note can move to the archive with `affects_repos: [scimantic-schema]` so a future scimantic-schema session sees the follow-up.

---

## Cross-version URL stability — known limitation

For the dropdown to function as cross-version navigation, per-class and per-slot URLs should ideally be stable across versions when the class/slot exists in both. e.g., `/schema/v0.2.0/classes/Question.html` should resolve the same way as `/schema/v0.3.0/classes/Question.html`.

panschema currently writes a single `index.html` with everything inline. The dropdown can only switch the whole-page URL; intra-page sections don't survive version switches. Not blocking, but a UX limitation worth documenting.

If/when panschema gains a "split per class" output mode, stable URLs become important. For now, the dropdown switches to the equivalent version's `index.html` and the user re-finds their class via the graph or class list.

---

## Things to watch (deferred design considerations)

- **Idempotence.** Running `panschema publish` twice in a row produces byte-identical output. No timestamps in generated HTML except where mandatory (e.g., build-timestamp footer). The consumer's GH Pages workflow re-runs on every push; non-determinism inflates the diff.
- **No working-tree mutation.** `git show <tag>:<path>` is the extraction primitive; the working tree stays exactly as the user left it.
- **What "current" means is consumer-chosen, not auto-latest-tag.** Validate the field but don't try to be clever.
- **Cache friendliness.** GH Pages caches aggressively; the JS dropdown navigation uses plain `/schema/<version>/` paths without cache-busting query params.
- **`linkml` field interaction.** Each tag's extracted schema may declare a different LinkML metamodel version. The build should honor the tag's declared version, not the manifest's current `[schema].linkml` value. If panschema enforces a particular range, document + emit a clear error per version — silent mismatches are not acceptable.
- **`panschema publish` vs `panschema release`.** Orthogonal. `release` cuts a new tag (bumps `[schema].version`, commits, tags, pushes). `publish` builds versioned docs from the current set of tags. Release happens at release time; publish happens at deploy time (typically every push to main, plus on tag push).

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `[publishing]` parsing + validation | Must Have | None | Completed |
| Slice 2: Per-version git extraction | Must Have | Slice 1 | Not Started |
| Slice 3: `panschema publish` command | Must Have | Slices 1, 2 | Not Started |
| Slice 4: Template integration (dropdown + banner) | Must Have | Slice 3 | Not Started |
| Slice 5: scimantic-schema dogfood + panschema release | Must Have | Slice 4 | Not Started |
