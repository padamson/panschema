# Feature 52: Book Editions

**Feature:** `mdbook-panschema editions` publishes a schema repo's book in
editions, one per schema minor line (a major line after 1.0), each rebuilt
from its git tag the way `panschema publish` rebuilds schema versions.

**User Story:** As the author of a schema whose book is edited in place, I
want every released minor line's book to stay published next to that
line's schema docs, so a consumer pinned to `v0.1.x` reads the book that
describes the schema they pin, while `main` carries the next edition.

**Related:** [Feature 11](11-versioned-docs-publish.md) (versioned schema
docs: the release list, `current`, `edge`, building from tags),
[Feature 21](21-book-to-schema-link.md) (the book→schema toolbar link),
[ADR 007](../adr/007-mdbook-panschema-plugin.md) (why this lives in
`mdbook-panschema`; see its 2026-10-12 addendum).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

The schema docs are versioned; the book is built from `main` only. A schema
whose v0.2 reverses a decision v0.1 made rewrites the book in place, so the
day `main`'s book diverges, the site shows an unreleased v0.2 book while
every consumer still pins `v0.1.x`, and the v0.1 book survives only as a git
tag. Editions keep each released line's book published at a stable path,
the way `schema/v0.1.1/` already keeps that version's schema docs.

## Design

**An edition is a minor line during 0.x, a major line after 1.0**, built
from the newest patch tag in the line. `v0.1.0` and `v0.1.1` form the
`v0.1` edition, built from `v0.1.1`. An edition spans exactly the releases
a consumer's pin moves within without breaking. By the same rule each
0.0.z release is its own edition, as a caret pin on `0.0.1` never moves
to `0.0.2`. A pin never resolves to a pre-release either, so a line
builds from its newest release, and from its newest pre-release only when
it has no release yet. The `edge` ref (typically `main`) builds an
unreleased edition.

**Editions read the release list `panschema publish` reads.** The tags are
`[publishing].versions`, the unreleased edition is `[publishing].edge`, and
the current edition is the one containing `[publishing].current`. A second
list could disagree with the first, and the two directions of the
book↔schema link have to agree on which edition a version belongs to.

**Layout mirrors the schema's:**

| Path | Serves |
|---|---|
| `/` | the current edition, as a copy |
| `book/v0.1/`, `book/v0.2/` | each released edition, at a stable path |
| `book/main/` | the unreleased edition |

The root is a copy, not a redirect: GitHub Pages has no server-side
redirects, so a root redirect keeps `index.html` working and breaks every
deep chapter link already shared. Every page carries
`<link rel="canonical">` to its edition's stable path, so the root copy and
the stable path do not compete as duplicates.

**Links agree in both directions.** The `v0.1` edition's toolbar link
points at `schema/v0.1.1/` (the tag the edition was built from), not
`schema/current/`. A page under `schema/v0.1.0/` or `schema/v0.1.1/`
links its header brand to `book/v0.1/`, not the site root.

**Old tags know nothing about editions.** `book.toml` at `v0.1.1` has no
switcher. The switcher, the canonical link and the edition's schema link
come from the `mdbook-panschema` running the build, never from the tagged
tree. The tagged tree supplies only the book's own content and
configuration.

**A tag's book builds with the tag's toolchain.** A patch release of mdbook
or a preprocessor can change rendered output, so a release records its
toolchain in `<book>/toolchain.toml`, in the tree, and the build installs
each edition's toolchain (cached by version). The unreleased edition may
name branch tips, so the dogfood loop survives. A tag cut before the file
existed gets its toolchain from an override in `panschema-publish.toml`.

**Idempotent, from tags.** Every edition is rebuilt from its tag on every
run, with the working tree untouched (the tag's tree is checked out
through a scratch index), and two runs produce byte-identical output.
Nothing built is stored on a release.

### Configuration

```toml
[publishing]
versions = ["v0.1.0", "v0.1.1"]
edge = "main"
current = "v0.1.1"
output_dir = "site/schema"

[editions]
book = "book"          # directory holding book.toml, relative to this file
output_dir = "site"    # the site root: editions land in <output_dir>/book/
```

`[editions]` requires `[publishing]`. The schema docs must land inside the
editions' `output_dir`, since the two link to each other by relative path.

The toolchain file, in the book directory at each tag:

```toml
mdbook = "0.5.2"

[[plugin]]
name = "mdbook-listings"
version = "0.2.2"
install = ["install"]          # run in the book directory before building

[[plugin]]
name = "mdbook-admonish"
git = "https://github.com/padamson/mdbook-admonish"
rev = "<sha>"                  # a release records an exact rev
install = ["install", "."]
```

`install` is the argument list that generates the plugin's gitignored book
assets. `mdbook-panschema` is never listed: the build always installs its
own toolbar asset, from the running binary.

---

## Vertical Slices

### Slice 1: Edition planning

**Status:** Completed

**User Value:** An author can declare `[editions]`, and the release list is
grouped into editions and checked when the publish spec loads, before
anything is built.

**Acceptance Criteria:**
- [x] A publish spec with `[editions]` and `[publishing]` parses; one with
  `[editions]` but no `[publishing]` is refused with an error saying
  editions read the release list from `[publishing]`.
- [x] `v0.1.0`, `v0.1.1` and `v0.2.0` plan as two editions: `v0.1` built
  from `v0.1.1`, and `v0.2` built from `v0.2.0`.
- [x] After 1.0, an edition is a major line: `v1.0.0`, `v1.2.3` and
  `v2.0.0` plan as `v1` (from `v1.2.3`) and `v2` (from `v2.0.0`).
- [x] "Newest" is semver order, not list order: a list written
  `["v0.1.1", "v0.1.0"]` still builds `v0.1` from `v0.1.1`.
- [x] `edge` plans an unreleased edition under its own label (`main`); an
  edge label equal to a released edition's label is refused, naming it.
- [x] The current edition is the one containing `current`; when `current`
  equals `edge`, the unreleased edition is current.
- [x] Each 0.0.z release is its own edition (`v0.0.1`, `v0.0.2`).
- [x] A line builds from its newest release, not a later pre-release
  (`v0.2.0` over `v0.2.1-rc.1`); a line of only pre-releases builds from
  the newest one.
- [x] A version that does not parse as semver (with or without a leading
  `v` or `V`) is refused with an error naming it, since its line cannot be
  known.
- [x] Tags spelled two ways (`0.1.0` beside `v0.1.1`) are refused, naming
  one of each, since edition directories take the tags' spelling; a
  version listed twice is refused, naming it.
- [x] A schema `output_dir` outside the editions' `output_dir`, or inside
  its `book/` directory in any letter case, is refused, naming both; `.`
  and `..` are resolved before the comparison, so `site/../schema` is
  outside `site`.
- [x] Unknown keys in `[editions]` are refused, naming the key.

---

### Slice 2: `mdbook-panschema editions` builds every edition

**Status:** Completed

**User Value:** One command publishes every edition at its stable path and
the current edition at the site root, from tags, without touching the
working tree.

**Acceptance Criteria:**
- [x] `mdbook-panschema editions` writes each released edition to
  `<output_dir>/book/<edition>/`, built from its tag's tree, and the
  unreleased edition to `<output_dir>/book/<edge>/`.
- [x] The site root holds a copy of the current edition, and anything else
  already in `<output_dir>` (the schema docs) survives.
- [x] Re-running with a different `current` leaves no page of the previous
  current edition at the root.
- [x] A built book whose top level would collide with `book/` or the
  schema docs' directory is refused before anything is written.
- [x] The working tree is untouched: no file changes, no file appears, and
  no worktree is registered.
- [x] Each plugin the tag's `toolchain.toml` lists has its `install`
  arguments run in that edition's book directory before the build; a tag
  without the file builds with no plugin installs.
- [x] A failed edition build names the edition and the tag, and carries
  the tool's output.
- [x] Two runs over the same tags produce byte-identical output.

**Notes:**
- Tools come from `PATH` in this slice; Slice 5 installs pinned versions.
- Tests drive the orchestration through an in-process toolchain, so they
  need no mdbook install; one ignored test builds with the real `mdbook`
  (`cargo nextest run --run-ignored only -E 'test(real_mdbook)'`).
- A tag's tree is checked out with `git read-tree` and `git
  checkout-index` against a scratch index file, from the repo's top level
  so a book can include files from outside its own directory. `git
  archive` was the first choice and was dropped: it drops every path
  `.gitattributes` marks `export-ignore`, which repos do to keep their
  docs out of source tarballs, and it holds the whole tree in memory
  before unpacking.
- A book that declares more than one `[output.*]` backend renders its
  HTML into `html/` under mdbook's destination, so the HTML root is read
  from the tag's `book.toml`, not assumed.
- `[editions].book` may not lie inside `[editions].output_dir`: the
  build replaces `<output_dir>/book/` and writes the current edition over
  `<output_dir>/`, so a book source in there (the GitHub Pages `docs/`
  layout with the source in `docs/book`) would be deleted.
- The site root keeps a ledger, `.panschema-editions-root`, of the entries
  the root copy wrote, so the next run removes exactly those and nothing
  else at the root. It is written with the old and the new entries before
  the copy starts, so an interrupted copy is still recorded, and a ledger
  line that is not a fresh single name at the root (`..`, `book`, the
  schema docs' directory) is refused rather than removed, since a ledger
  something else wrote is not a record of this tool's work.
- With tools from `PATH`, a tag whose preprocessors predate the
  installed ones fails naming the edition and the tag, with mdbook's
  error about the missing preprocessor asset; Slice 5's toolchain
  override is what builds such a tag.

---

### Slice 3: The edition-aware toolbar

**Status:** Completed

**User Value:** A reader can see which edition they are reading, switch
editions, and reach the schema docs for exactly that edition.

**Acceptance Criteria:**
- [x] Every page of every edition, and of the root copy, has a switcher
  listing the released editions newest first and then the unreleased one,
  with the page's own edition selected.
- [x] Choosing an edition opens that edition's first page, at any page
  depth and under a project-path prefix.
- [x] A page of a non-current edition says which edition it is and links
  to the current one; a page of the unreleased edition says it is
  unreleased; the current edition's pages say neither.
- [x] Every page carries `<link rel="canonical">` to the same page at its
  edition's stable path, including the root copy's pages.
- [x] A `[book_link]` entry whose `schema_path` ends in `current/` links
  to the version the edition was built from (`schema/v0.1.1/`), or to the
  edge label for the unreleased edition; other entries keep their path.
- [x] All of the above holds for a tag whose `book.toml` predates editions
  and does not wire the toolbar asset.

---

**Notes:**
- One asset, `schema-link.js`, carries the schema button, the switcher
  and the banner, so they agree on which edition a page is. The editions
  build bakes the edition context in; `mdbook-panschema install`, run in
  a working tree with one edition, bakes `null` and renders no switcher.
- The canonical link is injected into each built page after its `<head>`
  tag, relative to the page so it resolves under a project-path prefix,
  with every path segment percent-encoded the way `encodeURIComponent`
  spells it (`#` is legal in a git ref and starts a URL fragment). The
  copy at the site root is two directories shallower, so it is taken
  from the build before the stable copy gets its canonicals and given
  its own; the canonicals are the one place the two copies differ. The
  top-level `404.html` gets none: GitHub Pages serves it at whatever
  depth the missing URL had, where a page-relative link points somewhere
  different on every miss.
- The asset reads the canonical to find the site root, and under editions
  resolves the schema links against it too: the book sits at
  `book/<edition>/` and the schema docs beside it, so `path_to_root`,
  which reaches only the edition root, would put the schema button
  inside the edition. A page without a `<head>` (or the 404 page) gets no
  switcher and keeps `path_to_root` links.
- Only a `[book_link]` path that is `current/` or ends in `/current/` is
  an alias and rewritten; `docs/concurrent/` is kept.
- Browser tests load a stand-in page over `file://` carrying mdbook
  0.5's toolbar and content markup and the injected canonical, the way
  the toolbar-link tests already do; the real markup was checked against
  a book built by mdbook 0.5.2.

---

### Slice 4: Schema pages link back to their edition

**Status:** Not Started

**User Value:** A reader who arrives at a version's schema docs gets back
to the book that describes that version, not the newest one.

**Acceptance Criteria:**
- [ ] With `[editions]` configured, `panschema publish` points each
  version page's header brand at its edition (`schema/v0.1.0/` and
  `schema/v0.1.1/` at `book/v0.1/`; the edge page at `book/main/`).
- [ ] `schema/current/` stays a byte copy of its version, so its brand
  points at that version's edition.
- [ ] Without `[editions]`, `site_root_url` behaves exactly as before.
- [ ] Dependency pages, one directory deeper, reach the same edition.

---

### Slice 5: Each edition builds with its own toolchain

**Status:** Completed

**User Value:** An old edition renders the way it did when it was
released, even after the repo's toolchain moves on.

**Acceptance Criteria:**
- [x] Each edition builds with the mdbook and plugin versions its tag's
  `toolchain.toml` names, whatever is on `PATH`.
- [x] A toolchain is installed once and reused by every later edition and
  run that names the same versions.
- [x] `[editions.toolchain."<tag>"]` in `panschema-publish.toml` supplies
  the toolchain for a tag cut before `toolchain.toml` existed, and wins
  over the tag's own file.
- [x] A released edition whose toolchain names a branch instead of a
  version or an exact rev is refused, naming the tag and the plugin; the
  unreleased edition may name a branch.
- [x] A released edition with no toolchain from either source is refused,
  naming the tag and both places a toolchain can come from.

---

**Notes:**
- Each pin installs with `cargo install --locked` into
  `<cache>/toolchains/<tool>-<pin>/` (the cache root is panschema's,
  so `PANSCHEMA_CACHE_ROOT` moves it) and is reused whenever its binary
  is already there. A crates.io pin's directory is named by version, a
  git pin's by commit; a branch is resolved to its tip with `git
  ls-remote` once per run, so the dogfood loop builds today's tip and a
  tip already installed is not built again.
- mdbook finds each preprocessor by name, so the pinned plugins' `bin/`
  directories lead `PATH` for the `mdbook build` process only; nothing
  else in the environment changes.
- A plugin pinned to neither a version nor a repository, and an mdbook
  left unpinned, come from `PATH`; a released edition refuses both, since
  it has to render the way it did when it shipped.
- A consumer's CI will want to cache the toolchains directory, keyed on
  the pins, or every run rebuilds mdbook from source; that is the
  scaffold's call (`ontology-authoring-template`).
- A version pin is one version (`0.5.2`, not `0.5` or `^0.5`), and a
  released edition's git pin is a full commit id: a range installs
  whatever is newest on the day of the first install, and `rev =
  "main"` is a branch by another name. The unreleased edition may name
  a branch, or a rev that is not a commit id, and both are resolved to
  the commit at their tip. A pin that says two things (`version` with
  `rev`, `rev` without `git`, `rev` with `branch`) is refused for every
  edition. An override keyed by a ref no edition builds from (the wrong
  tag in a line, an edition label) is refused rather than ignored.
- A tag cut before `toolchain.toml` existed builds from its override, and
  a tag that names the same pin as another installs nothing new.

---

### Slice 6: Versions derived from tags

**Status:** Not Started

**User Value:** A release is a tag push. A patch tag cut on an old line,
from a commit no branch holds, publishes without a separate commit on
`main` to list it.

**Acceptance Criteria:**
- [ ] With `versions` omitted, every `v*` tag that parses as semver is a
  version, for schema docs and editions alike.
- [ ] With `current` omitted, the highest release tag is current.
- [ ] Pre-release tags (`v0.2.0-rc.1`) are versions but never the
  default `current`.
- [ ] An explicit `versions` or `current` still wins, so a repo can lag
  `current` or leave a tag out.

**Notes:**
- Feature 11 chose explicit fields ("don't silently default to latest
  semver"). The tag-only patch line changes the cost: under an explicit
  list, a tag cannot publish itself.

---

### Slice 7: Dogfood and docs

**Status:** Not Started

**User Value:** A schema repo with a released line and a diverged `main`
publishes both books, verified on a real site.

**Acceptance Criteria:**
- [ ] A real schema repo with a released `v0.1.1` (no `toolchain.toml`)
  and a `main` on v0.2 publishes `/`, `book/v0.1/` and `book/main/`, with
  the switcher, the banners and the schema links correct in a browser.
- [ ] README documents `[editions]`, `toolchain.toml`, the override and
  the command; CHANGELOG records the feature.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Edition planning | Must Have | None | Completed |
| Slice 2: Build every edition | Must Have | Slice 1 | Completed |
| Slice 3: Edition-aware toolbar | Must Have | Slice 2 | Completed |
| Slice 4: Schema pages link back | Must Have | Slice 1 | Not Started |
| Slice 5: Per-edition toolchain | Must Have | Slice 2 | Completed |
| Slice 6: Versions derived from tags | Should Have | None | Not Started |
| Slice 7: Dogfood and docs | Must Have | Slices 3, 4, 5 | Not Started |

---

## Things to watch

- **A tag push checks out the tag.** A docs workflow triggered by a tag
  push reads `panschema-publish.toml` from the tag, whose release list may
  predate the tag itself. Slice 6 makes that harmless for `versions`;
  `[editions]` and the toolchain overrides are still read from the
  checkout, so a consumer's workflow should publish from `main` with tags
  fetched.
- **Submodules.** The scratch-index checkout leaves a submodule's
  directory empty; a book that includes files from a submodule is out of
  scope.
- **Switching editions lands on the first page.** As with the schema
  version dropdown, chapters are not assumed to exist under the same path
  in every edition.
- **Canonical links are relative.** The site's absolute URL is not known
  to panschema, and a relative canonical resolves per the HTML spec. If a
  search engine turns out to want absolute ones, an optional site URL in
  `[editions]` is the place for it.
- **A dependency page absent at a tag.** A `book_link` entry rewritten to
  a version whose dependency page was never published links to nothing;
  publish already skips such pages, and the toolbar should agree with it.

---

## Definition of Done

- [ ] All acceptance criteria met; all slices "Completed".
- [ ] All tests passing: `cargo nextest run`.
- [ ] Docs build cleanly: `cargo doc`.
- [ ] Formatted: `cargo fmt --check`; no clippy warnings.
- [ ] README.md and CHANGELOG.md updated.
