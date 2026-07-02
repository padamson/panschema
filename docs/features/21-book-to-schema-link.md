# Feature: mdbook → Schema Cross-Link

**Feature:** An installable mdbook toolbar button (+ prose snippet) that links an mdbook book to its panschema-generated schema docs.

**User Story:** As a schema author who publishes both an mdbook book and panschema schema docs on one site, I want panschema to provide the book→schema navigation link as an installable, versioned asset — the way `mdbook-admonish install` drops its CSS/JS — so I get a maintained toolbar button without hand-writing per-book JavaScript that breaks on every mdbook release.

**Related ADR:** [007-mdbook-panschema-plugin.md](../adr/007-mdbook-panschema-plugin.md) — the command lives in a dedicated `mdbook-panschema` plugin (a new workspace member), not a `panschema` subcommand.

**Approach:** Vertical Slicing with Outside-In TDD.

> Throughout this doc, "book" means an **mdbook book** — a directory with a `book.toml`, built by mdbook. panschema is not part of that build; it generates the schema docs the book links out to.

---

## Context

panschema already owns **one** direction of the mdbook↔schema cross-link: the generated schema docs link *back* to the mdbook book via `site_root_url` in `panschema-publish.toml` (the header brand/home link — see [11-versioned-docs-publish.md](11-versioned-docs-publish.md)). The **reverse** direction — from the mdbook book *to* the schema docs — has no owner, so every consumer hand-rolls it.

A consumer built a working prototype (toolbar button injected via a small `schema-link.js` + `schema-link.css`, wired through the mdbook `book.toml`'s `additional-js`/`additional-css`) and hit the predictable smell: it's per-book custom JS+CSS that every consumer copies and that drifts with every mdbook release — the mdbook 0.5 toolbar-id rename alone silently broke the first cut. This belongs in a maintained tool, written once.

---

## Design Decision (settled — ADR 007)

The command lives in a **dedicated `mdbook-panschema` plugin** — a new workspace member crate producing an `mdbook-panschema` binary whose `install [dir]` subcommand (`dir` defaults to `.`) copies the asset and auto-edits `book.toml`, exactly the `mdbook-admonish` idiom. It takes a path dependency on the `panschema` library crate to reuse `[book_link]` parsing and asset bundling. Rejected: a `panschema install-book-link` subcommand (off-idiom — panschema isn't an mdbook plugin) and folding into `panschema publish`. Full rationale and alternatives in [ADR 007](../adr/007-mdbook-panschema-plugin.md).

---

## Manifest extension: `[book_link]`

A new section in `panschema-publish.toml`, symmetric with the existing `[publishing].site_root_url` (the schema→book direction):

```toml
[book_link]
enabled = true
schema_path = "schema/current/"   # book-relative path to the schema docs
label = "Schema reference"         # button aria-label / tooltip / prose text
```

`install` reads this section and bakes `schema_path` + `label` into the emitted asset. The consumer writes **zero** JavaScript — one config block, and the command handles the asset and the `book.toml` wiring. Improvements to the button then flow from a tool upgrade + re-`install`, not a manual edit in every book.

---

## Vertical Slices

### Slice 1: `[book_link]` section parsing + validation (no asset, no command)

**Status:** Completed

**User Value:** A consumer can declare `[book_link]` in `panschema-publish.toml` and get clear validation errors, so the wire format stabilizes before any asset or command depends on it. (Lives in the `panschema` library crate, independent of the `mdbook-panschema` crate work — so this slice can land first.)

**Acceptance Criteria:**
- [x] A `panschema-publish.toml` with a well-formed `[book_link]` section loads without error, and its `enabled` / `schema_path` / `label` values are available to the rest of the tool.
- [x] Omitted fields fall back to documented defaults: `enabled = false`, `schema_path = "schema/current/"`, `label = "Schema reference"`.
- [x] A malformed `[book_link]` (wrong value types, unknown keys) fails to load with an actionable error, consistent with existing manifest validation.
- [x] A manifest with no `[book_link]` section loads successfully (the feature is opt-in).

**Notes:**
- Wire format first, mirroring 11's Slice 1 — no command surface yet, so downstream slices commit to a stable shape.

---

### Slice 2: `mdbook-panschema` crate + `install [dir]` command

**Status:** Completed (core mechanics; rendered-button ACs verified in Slice 4 dogfood)

**User Value:** A consumer runs `mdbook-panschema install` and gets a working, correctly-aligned mdbook→schema toolbar button with no hand-written JS.

**Acceptance Criteria:**
- [ ] An `mdbook-panschema` binary exists and exposes an `install` subcommand.
- [ ] Running `install` in a book directory adds a toolbar button linking to the schema docs and wires it into `book.toml`; re-running is idempotent (no duplicate entries).
- [ ] `install` with no path argument operates on the current directory.
- [ ] In the built book, the button appears in the toolbar and navigates to the schema docs correctly from any page depth and under a GitHub Pages project-path prefix.
- [ ] The button's link target and label reflect the `[book_link]` `schema_path` and `label`.
- [ ] The button's icon renders legibly (fill and alignment) against the default mdbook theme.
- [ ] With `[book_link]` absent or `enabled = false`, `install` makes no changes and reports that it did nothing.

**Notes:**
- Implementation anchors (not ACs): a workspace-member crate with a path dependency on the `panschema` library, assets embedded like the viz/wasm bundle — see [ADR 007](../adr/007-mdbook-panschema-plugin.md). The rendering / selector / href pitfalls live in "Things to watch".

---

### Slice 3: Shared authoring-template adoption

**Status:** Not Started

**User Value:** Every downstream mdbook+schema site inherits the maintained link by default instead of copying a prototype.

**Acceptance Criteria:**
- [ ] A book scaffolded from the shared authoring template ships the working mdbook→schema button out of the box, with no hand-written JS.
- [ ] Editing the installed asset during local template dev is reflected on reload, or the required nudge is documented.

---

### Slice 4: Reference-consumer swap + dogfood

**Status:** Not Started

**User Value:** The prototype is retired in favor of the maintained mechanism, proving the feature end-to-end on a live site.

**Acceptance Criteria:**
- [ ] The reference consumer's button comes entirely from the installed asset — no hand-written `schema-link.*` or manual `additional-*` wiring remains.
- [ ] On the live dogfood site, the button renders and navigates correctly (alignment; href under the Pages path prefix).
- [ ] Existing prose links to the schema docs still work.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| ADR 007 — command home (`mdbook-panschema` plugin) | Must Have | None | Accepted (2026-06-30) |
| Slice 1 — `[book_link]` parse + validate | Must Have | None | Completed |
| Slice 2 — `mdbook-panschema` crate + `install [dir]` | Must Have | Slice 1 | Completed |
| Slice 3 — shared authoring-template adoption | Should Have | Slice 2 | Not Started |
| Slice 4 — reference-consumer swap + dogfood | Should Have | Slice 2 | Not Started |

---

## Things to watch (baked-in from the prototype)

- **Select by class, not id** — mdbook 0.5 prefixed the toolbar ids; `.menu-bar` / `.left-buttons` survived, `#menu-bar` did not.
- **Icon fill** — mdbook fills `.fa-svg svg` with `currentColor`; a stroke-based glyph needs `fill: none` or must be a fill-based icon.
- **No custom flex/vertical-align** — mdbook's `.icon-button` + `.fa-svg svg` already center the glyph; adding custom alignment misaligned the prototype.
- **`path_to_root`-relative href** — absolute paths break under GitHub Pages project prefixes.
- **Dev-loop watch gap** — installed `*.js`/`*.css` aren't watched by a typical mdbook dev script by default.

---

## Definition of Done

- [x] Command-home decided ([ADR 007](../adr/007-mdbook-panschema-plugin.md)).
- [ ] All slice acceptance criteria met; all slices "Completed".
- [ ] All tests passing: `cargo nextest run`.
- [ ] Docs build cleanly: `cargo doc`.
- [ ] Formatted: `cargo fmt --check`; no clippy warnings: `cargo clippy --all-targets --all-features -- -D warnings`.
- [x] README.md updated (the `mdbook-panschema install` command + `[book_link]` config).
- [ ] CHANGELOG.md updated.
