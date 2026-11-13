# Feature 54: Book Editions as Declared Spans

**Feature:** `mdbook-panschema editions` publishes the editions a schema's
author declares. Each edition is a book directory on `main` covering a span
of schema versions, and the spans partition the releases.

**User Story:** As the author of a schema's book, I want to decide when the
book needs a new edition, so a release that changes the schema's story gets
a book that tells it from the start, while releases that only extend it
stay in the edition that already describes them.

**Related:** [Feature 52](52-book-editions.md) (one edition per release
line, built from tags; this feature replaces its model and keeps its
site layout), [Feature 11](11-versioned-docs-publish.md) (the release list),
[Feature 21](21-book-to-schema-link.md) (the book→schema toolbar link),
[ADR 007](../adr/007-mdbook-panschema-plugin.md) (see its 2026-11-13
addendum).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

Feature 52 ties an edition to a release line and builds it from the line's
newest tag. An author's editions do not line up with release lines. Three
minors can sit in one edition, and a rewrite can start at any release: a
release that adds a class and moves slots off existing classes reads better
as a new edition that derives the slots as they are now than as a chapter
followed by a refinement section that undoes it. Under feature 52 an
edition the author thinks of as one book covering v0.1 to v0.3 is published
as three (`book/v0.1/`, `book/v0.2/`, `book/v0.3/`), each a prefix of the
next, and the switcher offers editions nobody wrote.

## Design

**An edition is a book directory on `main`.** Each one is an ordinary
mdbook root with its own `book.toml`, `src/`, and whatever its plugins keep
beside them (a `listings.toml` and its frozen listings, for instance). An
old edition is fixed by an ordinary commit, and every edition builds with
the repo's current tools, so a tool upgrade re-renders all of them.

**An edition names where it starts.** `from` is the first release the
edition covers, and the next edition's `from` closes it. Every release
belongs to exactly one edition: the one with the latest `from` at or below
it. A pre-release belongs to the edition of the release it precedes
(`v0.3.0-rc.1` to the edition starting at `v0.3.0`), and the edge belongs
to the last edition. A `from` need not be a cut tag yet: an edition whose
span holds no release is unreleased, and only the last edition may be.

**The URL is the author's.** An edition is served at `book/<id>/`, from its
first unreleased build through every release in its span, so nothing moves
when a tag is cut. `name` is what the switcher and the banner show.

**One edition is the authoring edition**, the one new chapters and listings
go into. It is marked with `authoring = true`, or is the last edition when
none is marked, and `mdbook-panschema editions --authoring` prints its book
directory so a consumer's scripts need not parse the publish spec.

**The schema stays loosely coupled to the book.** Every schema version is
reachable from every edition: the toolbar's schema button lists every
version grouped by edition. A release cut while an edition is edited on
`main` republishes that edition for its whole span, which is the loose
coupling working as intended.

**Every edition builds from the edge ref**, through the same scratch-index
checkout feature 52 uses for tags, so the working tree is never touched and
two runs over the same commit write the same bytes. Tags still build the
schema docs (`panschema publish` is unchanged); they no longer build books.

**What stays from feature 52:** the site layout (`book/<edition>/`, the
current edition copied to the site root, the root ledger), the canonical
links, the switcher and banner, the schema pages' brand linking back to
their edition, and versions and `current` derived from the tags. What goes:
building a book from a tag, `<book>/toolchain.toml` per tag, the per-tag
overrides, and the rule that a released edition's pins be exact.

### Configuration

```toml
[publishing]
edge = "main"
output_dir = "site/schema"     # must lie inside [editions].output_dir

[editions]
output_dir = "site"            # the site root; editions land in site/book/

[[editions.edition]]
id = "1"                       # served at site/book/1/
name = "First edition"         # defaults to the id
book = "book/editions/1"       # holds book.toml, relative to this file
from = "v0.1.0"

[[editions.edition]]
id = "2"
name = "Second edition"
book = "book/editions/2"
from = "v0.3.0"
authoring = true               # defaults to the last edition

[editions.toolchain]           # optional; tools come from PATH without it
mdbook = "0.5.2"

[[editions.toolchain.plugin]]
name = "mdbook-listings"
version = "0.2.2"
install = ["install"]          # run in each edition's book directory
```

`[editions.toolchain]` has the shape of feature 52's `toolchain.toml`. A
plugin may be pinned to a crates.io version, a git commit, or a git branch
(resolved to its tip once per run).

---

## Vertical Slices

### Slice 1: Declared editions, planned and built

**Status:** Completed

**User Value:** An author declares editions and their spans, and one
command publishes each at `book/<id>/` with the schema pages linking back
to the edition that covers them.

**Acceptance Criteria:**
- [x] `[editions]` with `[[editions.edition]]` entries parses, each with
  `id`, `book`, `from`, and optional `name` (the id when absent) and
  `authoring`; unknown keys, feature 52's `book` among them, are refused
  by name.
- [x] `[editions]` without an `[[editions.edition]]` is refused, naming
  the table to add, before any other check; without `[publishing].edge`
  it is refused, saying the editions build from the edge.
- [x] Releases `v0.1.0`, `v0.1.1`, `v0.2.0`, `v0.3.0`, `v0.3.1` and
  editions from `v0.1.0` and `v0.3.0` plan as the first edition holding
  `v0.1.0` to `v0.2.0` and the second holding `v0.3.0`, `v0.3.1`, and the
  edge, whichever order the editions are declared in.
- [x] A pre-release belongs to the edition of the release it precedes.
- [x] A release older than every edition's `from` is refused, naming the
  release and the first edition; when the release list was read from the
  tags, the refusal also names `[publishing].versions` as the way to
  leave the tag out.
- [x] An edition other than the last that holds no release is refused,
  naming it; the last edition with no release plans as unreleased. A
  pre-release is not a release, so an edition holding only pre-releases
  counts as holding none.
- [x] A version listed twice, under one spelling or two (`v0.1.0` and
  `0.1.0`), is refused, naming both.
- [x] Two editions sharing an `id` or a `book` (in any letter case) or a
  `from` are refused, naming both; an `id` that is not one path segment,
  a `from` that is not semver, and a `book` that is absolute or lies
  outside the repository are refused, naming the edition.
- [x] Two editions marked `authoring` are refused, naming both; with none
  marked, the last edition is the authoring one.
- [x] The site root serves the edition holding `current`; the last
  edition when `current` is the edge.
- [x] `mdbook-panschema editions` builds each edition's `book` from the
  edge ref's tree into `<output_dir>/book/<id>/`, leaves the working tree
  untouched, and keeps feature 52's guarantees for the root copy, the
  ledger, the canonical links, and byte-identical reruns.
- [x] Every edition builds with `[editions.toolchain]`, running each
  plugin's `install` in that edition's book directory, or with the tools
  on `PATH` when there is none; a pin that says two things, or a git
  repository with neither a `rev` nor a `branch`, is refused when the spec
  is read, naming the tool.
- [x] A `toolchain.toml` in an edition's book directory, where feature 52
  kept the pins, is refused before any edition builds, naming the edition
  and `[editions.toolchain]`.
- [x] A dependency whose released publish spec carries an `[editions]`
  this build would refuse still resolves.
- [x] A `[book_link]` entry aliasing `current/` links to the newest
  release in the edition's span, or to the edge for an unreleased edition.
- [x] `panschema publish` points each version page's brand at the edition
  holding it (`v0.1.0` to `v0.2.0` at `book/1/`, `v0.3.x` and the edge at
  `book/2/`).
- [x] README describes declared editions in place of feature 52's
  section (configuration, starting a new edition, an edition edited on
  `main` republishing for its whole span, the toolchain), and CHANGELOG
  records the change and that feature 52's `[editions]` form is refused.

---

### Slice 2: The toolbar names editions and lists every version

**Status:** Not Started

**User Value:** A reader sees which edition and which versions they are
reading about, and reaches any version's schema docs from any edition.

**Acceptance Criteria:**
- [ ] The switcher lists the editions newest first by `name`, the page's
  own selected and the unreleased one marked as such.
- [ ] A page of an edition that is not current names its edition and the
  span of releases it covers, and links to the current edition; a page of
  the unreleased edition says it is unreleased.
- [ ] Under editions, the schema button of a `[book_link]` entry aliasing
  `current/` opens a list of every version, the edge included, grouped by
  edition newest first; the page's own edition and its newest version are
  emphasized and the other editions' versions de-emphasized; each links to
  that version's schema docs.
- [ ] The list is reachable and closable by keyboard, as the multi-schema
  menu is.
- [ ] Without editions, the schema button renders as it does today.

---

### Slice 3: The authoring edition for scripts

**Status:** Not Started

**User Value:** A consumer's freeze and preview scripts find the book that
new listings go into without parsing the publish spec.

**Acceptance Criteria:**
- [ ] `mdbook-panschema editions --authoring` prints the authoring
  edition's book directory and builds nothing.
- [ ] It fails, exiting 2, when the publish spec cannot be read or has no
  `[editions]`.

---

### Slice 4: Integration tests and docs

**Status:** Not Started

**User Value:** A schema repo with two declared editions publishes both,
verified end to end in a browser, and the README tells an author how to
start a new edition.

**Acceptance Criteria:**
- [ ] A repo with two editions, three releases, and an edge publishes
  `/`, `book/1/`, `book/2/`, and the schema docs, with the switcher, the
  banners, the version list, and each schema version's brand correct in a
  browser.
- [ ] README covers what slices 2 and 3 add: the edition names and the
  version list in the toolbar, and the authoring edition with
  `mdbook-panschema editions --authoring`.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Declared editions | Must Have | None | Completed |
| Slice 2: Toolbar | Must Have | Slice 1 | Not Started |
| Slice 3: Authoring edition | Should Have | Slice 1 | Not Started |
| Slice 4: Integration and docs | Must Have | Slices 1, 2 | Not Started |

---

## Things to watch

- **A prose-only rewrite starts at the next release.** Spans partition
  releases, so a new edition cannot begin between two of them.
- **Migrating from feature 52.** Its `[editions]` form fails the parse by
  key, and a `toolchain.toml` left beside a book stops the build. No
  consumer published with it, so there is no migration path beyond the
  errors and the README. A dependency's `[editions]` is never read, so a
  package released with the old form still resolves.
- **A sync gate.** Nothing checks that an edition's last full-schema
  listing matches the schema at the newest release in its span. A gate
  would have to define "last full-schema listing" and read
  mdbook-listings' manifest, and every release's `version:` bump would
  force a re-freeze, so it stays out of this feature.
- **Switching editions lands on the first page**, as in feature 52.

---

## Definition of Done

- [ ] All acceptance criteria met; all slices "Completed".
- [ ] All tests passing: `cargo nextest run`.
- [ ] Docs build cleanly: `cargo doc`.
- [ ] Formatted: `cargo fmt --check`; no clippy warnings.
- [ ] README.md and CHANGELOG.md updated.
