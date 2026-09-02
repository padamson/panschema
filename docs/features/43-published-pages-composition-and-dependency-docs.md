# Feature 43: Published Pages — Composition and Dependency Docs

**Feature:** Let a repo publish more than one schema-docs page — its own
schema, and one per dependency schema it holds local instance data for —
and let each page choose its composition: schema-docs first (today's
page, the default) or instance-graph first, with the schema reference
sections optional.

**User Story:** As a domain repo that curates instance data — my own
catalog, and records conforming to a contract schema I consume as a
dependency — I want each schema I hold data for documented as its own
versioned page, with the page leading with whichever half matters to its
readers, so a reader can open my catalog beside the contract records
that point into it and follow a stable link to either.

**Related ADR:** ADR-009 (instance graphs share the schema-docs
renderer).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Design

Two orthogonal halves.

**Which pages exist.** A published site is a set of schema-docs pages.
The repo's own schema page is implicit, exactly as today. A
`[[instances]]` entry may name a dependency (`schema = "<dep>"` matching
a `[schemas.<dep>]` entry); entries naming the same dependency group
onto one additional page for that dependency's schema, with those
datasets embedded. Every page renders with the same renderer and the
same per-version + `edge` + `current/` layout, at an explicitly
configured directory **inside** the publish output tree — never a
location inferred from the output path's parent.

**What each page looks like.** A page has a layout:

- `layout = "schema-first"` (default) — today's page, byte-identical
  when nothing is configured.
- `layout = "instances-first"` — the instance graph section leads; the
  schema reference (schema graph and class/slot/enumeration/type cards)
  follows.
- `schema_sections = false` — omit the schema reference entirely,
  leaving the instance graph and its cards. Valid with either layout;
  the default is `true`.

The own page's layout is configured under `[publishing]`; a dependency
page's under its own table. A data-first page is a composition of the
one page kind, not a different kind of page.

## Design requirements (from review of a discarded first cut)

A sibling `/data/`-space design was implemented and reverted; its review
produced constraints this design must satisfy by construction:

1. **Explicit locations only.** Every page's directory is configured or
   derived *within* the configured output tree. Nothing is written — and
   especially nothing is deleted — outside it.
2. **One segment validator.** Page directory names, dataset names used
   in paths, and version/edge labels are validated as single, unique
   path segments: no `.`/`..`, no empty names, no collision with the
   reserved `current`, uniqueness enforced at parse time with errors
   naming the entries.
3. **Per-page version context.** A page's version dropdown lists only
   the refs at which that page exists; its `current/` alias follows a
   per-page notion of current (the configured current when present
   there, else absent *with a note*); root-links and version URL
   patterns are computed for the page's own depth and space.
4. **Load once per version.** The schema load, instance parsing,
   validation, diagnostics, and label-store opening happen once per
   published ref; every page of that ref renders from the shared state.
   Validation findings print once per ref, not once per page.
5. **Aliases are refreshed safely.** One shared helper maintains every
   `current/` copy: source checked before the old alias is removed, and
   an alias that ends up absent is said out loud.

## Vertical Slices

### Slice 1: Page composition options on the own-schema page

**Status:** Completed

**User Value:** A repo can lead its published schema page with the
instance graph, or publish it without the schema reference sections.

**Acceptance Criteria:**
- [x] `[publishing] layout = "instances-first"` renders the instance
      graph section before the schema reference sections.
- [x] `[publishing] schema_sections = false` omits the schema graph and
      the class/slot/enumeration/type sections; the instance section and
      its cards render as before.
- [x] With neither key set, output is byte-identical to today's.
- [x] An unknown `layout` value is a configuration error naming the
      accepted values.
- [x] `generate --format html` accepts the same composition
      (`html_page_layout` / `html_schema_sections` in the manifest), so a
      page can be previewed without publishing.

### Slice 2: A page per dependency schema with local instances

**Status:** Completed

**User Value:** A repo can publish a dependency schema's docs with its
own local datasets embedded — the contract-plus-local-records page.

**Acceptance Criteria:**
- [x] An `[[instances]]` entry with `schema = "<dep>"` places its dataset
      on a second published page rendering that dependency's schema;
      several entries naming one dependency share that page.
- [x] The dependency page lives at a configured directory inside the
      output tree, versioned and aliased like the own page, and its
      version dropdown offers only refs where the page exists.
- [x] At each published ref, the page renders the data as of that ref
      against the dependency as pinned at that ref.
- [x] Naming a dependency the manifest does not declare is a
      configuration error naming the entry and the missing dependency.
- [x] The dependency page takes the same composition options as the own
      page, configured per page.
- [x] The dataset's external references render as the external links the
      instance graph draws elsewhere.

### Slice 3: The pages link to each other

**Status:** Completed

**User Value:** A reader on any published page can reach the others
without knowing the URL scheme.

**Acceptance Criteria:**
- [x] Each published page links to the site's other pages by name.
- [x] README documents the composition keys, the dependency-page
      pattern, and how a book's `[[book_link]]` selector fronts the
      pages — in the same change that ships each key it names.

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Completed |
| Slice 2 | Must Have | Slice 1 | Completed |
| Slice 3 | Should Have | Slice 2 | Completed |

---

## Definition of Done

- [x] All acceptance criteria met, slices Completed
- [x] All tests passing: `cargo nextest run`
- [x] CHANGELOG.md and README.md updated
- [x] Docs build cleanly: `cargo doc`

## Notes / Things to Watch

- Dependency resolution at publish time goes through the local cache
  only — never a live network fetch — so publishes are reproducible
  from a clean checkout with a warm cache (`panschema fetch` first).
  Old refs pin the dependency by the version that ref's own
  `panschema.toml` declares (exact versions, so the manifest is the
  pin), and historical pages show the contract as it was. `path:`
  dependencies carry no pin at all and resolve from the working tree
  at every ref — the local-development shape, documented as such.
- A dependency page that resolves at no ref publishes nothing and says
  so with a warning; per-ref skips (data or dependency absent there)
  are notes, matching the own page's missing-data behavior. A declared
  dependency that *fails* to resolve (cold cache, corrupt package,
  unparseable manifest at the ref) is distinguished from one not yet
  declared: its note carries the resolver's message, including the
  `panschema fetch` remedy for a cold cache.
- `url_pattern` and `site_root_url` are site-level with depth-correct
  parent-relative defaults. A relative `site_root_url` override is
  re-based per page (a dependency page prepends the `../` its extra
  depth needs; a relative override must climb, or it is refused at
  parse), and absolute values pass through. An overridden
  `url_pattern` still targets the own page's depth — a dependency
  page's version dropdown navigates its own version tree, so a
  re-base would point it at the wrong page's versions. Per-page URL
  overrides wait for a consumer who needs them.
- Publish resolves the dependency by the version the ref's manifest
  declares, and checks the cached content against the ref's committed
  lockfile checksum. Whether a pin exists to honor is decided by the
  manifest's parsed source, never the lock entry's spelling; an entry
  disagreeing with its own ref's manifest (version or source) is
  reported as a stale lock, since the cache may be pristine and no
  fetch repairs committed history; a lockfile that is present but
  unparseable refuses the page rather than failing open. Refs without
  a lockfile entry publish ungated, and `path:` dependencies are never
  gated. The checksum covers the schema's main file — the content
  `fetch` locks and `verify` checks, through the same shared
  comparison — so imported sibling files in the cached package are
  outside the gate until the lockfile format records a package-level
  digest (tracked as a follow-up).
- A dataset's version identity is the publish cohort's ref; there is no
  per-dataset semver.
- Composition presets were chosen over independent per-section toggles:
  two knobs (`layout`, `schema_sections`) cover the demanded shapes, and
  an instance-cards toggle waits for a consumer who wants it.
- Withdrawn or renamed pages leave stale directories in persistent
  output trees (CI deploys); pruning the output tree is the deploy job's
  concern today, but worth revisiting if it bites.
- `panschema serve` renders through the default writer registry and does
  not yet apply composition keys — a known preview gap (it already
  diverges on the graph knobs); `generate` is the preview path.
- A data-only page keeps the namespace table (its instance cards' CURIEs
  expand through it) and carries the shared graph-shell script itself,
  since the schema sections that normally include it are omitted.
