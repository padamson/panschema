# Feature: mdbook → Schema Cross-Link

**Feature:** An installable mdbook toolbar button (+ prose snippet) that links an mdbook book to its panschema-generated schema docs.

**User Story:** As a schema author who publishes both an mdbook book and panschema schema docs on one site, I want panschema to provide the book→schema navigation link as an installable, versioned asset — the way `mdbook-admonish install` drops its CSS/JS — so I get a maintained toolbar button without hand-writing per-book JavaScript that breaks on every mdbook release.

**Related ADR:** [007-mdbook-panschema-plugin.md](../adr/007-mdbook-panschema-plugin.md) — the command lives in the `mdbook-panschema` binary, not a `panschema` subcommand. Originally a dedicated workspace crate; since 2026-08-13 the binary ships as a second `[[bin]]` of the `panschema` crate (see the ADR's addendum).

**Approach:** Vertical Slicing with Outside-In TDD.

> Throughout this doc, "book" means an **mdbook book** — a directory with a `book.toml`, built by mdbook. panschema is not part of that build; it generates the schema docs the book links out to.

---

## Context

panschema already owns **one** direction of the mdbook↔schema cross-link: the generated schema docs link *back* to the mdbook book via `site_root_url` in `panschema-publish.toml` (the header brand/home link — see [11-versioned-docs-publish.md](11-versioned-docs-publish.md)). The **reverse** direction — from the mdbook book *to* the schema docs — has no owner, so every consumer hand-rolls it.

A consumer built a working prototype (toolbar button injected via a small `schema-link.js` + `schema-link.css`, wired through the mdbook `book.toml`'s `additional-js`/`additional-css`) and hit the predictable smell: it's per-book custom JS+CSS that every consumer copies and that drifts with every mdbook release — the mdbook 0.5 toolbar-id rename alone silently broke the first cut. This belongs in a maintained tool, written once.

---

## Design Decision (settled — ADR 007)

The command lives in an **`mdbook-panschema` binary** whose `install [dir]` subcommand (`dir` defaults to `.`) copies the asset and auto-edits `book.toml`, exactly the `mdbook-admonish` idiom. Rejected: a `panschema install-book-link` subcommand (off-idiom — panschema isn't an mdbook plugin) and folding into `panschema publish`. Originally the binary was a dedicated workspace crate with a path dependency on the `panschema` library; it now ships as a second `[[bin]]` of the `panschema` crate itself. Full rationale, alternatives, and the fold are in [ADR 007](../adr/007-mdbook-panschema-plugin.md).

---

## Manifest extension: `[book_link]`

A new section in `panschema-publish.toml`, symmetric with the existing `[publishing].site_root_url` (the schema→book direction):

```toml
[book_link]
enabled = true
schema_path = "schema/current/"   # book-relative path to the schema docs
label = "Schema reference"         # button aria-label / tooltip / prose text
```

A book fronting several schemas writes the same key as an array instead, one entry per schema (see Slice 5):

```toml
[[book_link]]
schema_path = "schema/current/"
label = "Wine schema"

[[book_link]]
schema_path = "schema/cqa/current/"
label = "CQ&A contract"
```

`install` reads this section and bakes the entries into the emitted asset. The consumer writes **zero** JavaScript — one config block, and the command handles the asset and the `book.toml` wiring. Improvements to the button then flow from a tool upgrade + re-`install`, not a manual edit in every book.

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
- Implementation anchors (not ACs): the `mdbook-panschema` binary in the `panschema` crate (originally a dedicated workspace crate — see [ADR 007](../adr/007-mdbook-panschema-plugin.md) and its addendum), assets embedded like the viz/wasm bundle. The rendering / selector / href pitfalls live in "Things to watch".

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

### Slice 5: One book, several schemas — link for one, selector for N

**Status:** Completed

**Priority:** Should Have

**User Value:** A book fronting more than one schema can reach all of them
from the toolbar. The motivating case is a domain book that publishes both
its own schema docs and a *dependency* schema's docs rendered with local
instance data: an author cross-checking that each eval item's expected
anchors resolve to real catalogue nodes wants both pages open side by side,
which means both have to be reachable and separately addressable.

The wider driver is an ecosystem docs site fronting several schemas at once;
two is the first case, three-plus is what the selector is really for.

**Design:** `[book_link]` becomes a list. This is the **selector-when-N**
pattern the in-page instance-graph selector already established (feature
37): one renders bare, many render behind a picker. Using it again here
makes the book-navigation version read as the same idea rather than a
second, differently-shaped affordance.

Both TOML spellings parse, because every book that exists today writes the
table form:

```toml
# today — unchanged, still renders one plain link
[book_link]
enabled = true
schema_path = "schema/current/"
label = "Schema reference"

# new — two or more entries render a drop-down in the same toolbar slot
[[book_link]]
schema_path = "schema/current/"
label = "Wine schema"

[[book_link]]
schema_path = "schema/cqa/current/"
label = "CQ&A contract"
```

**Acceptance Criteria:**
- [x] The table form parses exactly as it does today, with the same
  defaults, and installs the same single link. No existing book changes
  behaviour or needs an edit.
- [x] The array-of-tables form parses, and each entry carries its own
  `schema_path` and `label`.
- [x] A list of one installs a plain link, identical to what the table form
  produces — degrading to today's behaviour is what makes the list form
  safe to adopt early.
- [x] A list of two or more installs a drop-down in the same toolbar slot,
  listing every entry in declaration order.
- [x] `enabled` still turns the whole feature off, in either form, and an
  empty list is the same as absent.
- [x] A malformed entry — unknown key, or a missing `schema_path` — is a
  parse error naming the problem, not a silently dropped button.
- [x] Every link resolves under a project-path prefix, as the single link
  already does (`path_to_root`-relative, never absolute).

**Notes:**
- The note that filed this holds a **demand-driven rule**: build when a book
  actually needs to link out to a second page, not ahead of it. That
  condition is not met yet — the dependency-schema page is several steps out
  in another repo — so the tests here stand on this repo's own fixtures
  rather than waiting on a consumer.
- **The `untagged` derive was wrong twice, and both showed up as tests.**
  Serde can build a struct from a sequence, and every `[book_link]` field
  has a default, so `book_link = []` matched the table form and produced
  one link from an empty list. And an untagged derive reports only "data
  did not match any variant," so a typo'd key named nothing — losing the
  one thing the table form already did well. A hand-written `Deserialize`
  dispatching on array-versus-table fixes both and keeps the inner error.
- **The page a second entry points at can already be produced.** Rendering
  local instance data against a schema the repo does not own is
  [feature 36 slice 5](36-instance-graph-publishing-and-exports.md), which
  is complete — a manifest naming an external schema plus an `instances`
  list emits the full page, instance graph included. So a book can point a
  second `[[book_link]]` entry at that page today. What is missing is only
  a *versioned* history for it, which is feature 36 slice 6 — and note that
  what gets versioned there is the **instance graph**, on the consuming
  repo's own tags. The consumed schema stays a pinned dependency whose own
  docs its owner publishes.
- Capability B from the same note — *publishing* a dependency schema's docs
  with local instances — is the other half and is **not** this slice. It
  lands in the publish path, not the toolbar, and is specced separately.
  This slice only makes such a page reachable once it exists.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| ADR 007 — command home (`mdbook-panschema` plugin) | Must Have | None | Accepted (2026-06-30) |
| Slice 1 — `[book_link]` parse + validate | Must Have | None | Completed |
| Slice 2 — `mdbook-panschema` crate + `install [dir]` | Must Have | Slice 1 | Completed |
| Slice 3 — shared authoring-template adoption | Should Have | Slice 2 | Not Started |
| Slice 4 — reference-consumer swap + dogfood | Should Have | Slice 2 | Not Started |
| Slice 5 — link for one, selector for N | Should Have | Slice 2 | Completed |

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
