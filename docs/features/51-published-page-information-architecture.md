# Feature 51: Published Page Information Architecture

**Feature:** Group the schema-docs page into three parts a reader can see
in the structure, not infer from vocabulary: an overview of the page
itself, the schema (T-box), and the instances (A-box). The sidebar shows
the three groups; the body renders them as blocks; the layout preset
swaps the two content blocks whole; and the instances get the metadata
card the schema has always had.

**User Story:** As a reader of a published schema page, whether the
repo's own schema or a dependency page carrying the repo's records, I
want to see at a glance which parts describe the schema and which
describe the data, and to read where the data came from, what it
conforms to and how much of it there is, so a page that leads with its
instances reads as that and not as a reordered list.

**Related:** Feature 43 (page composition and dependency pages), ADR-009
(instance graphs share the schema-docs renderer), Feature 45 (one graph
component, two surfaces).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

The page's sidebar is a flat list of seven peers: five describe the
schema, one the data, one the page. The `instances-first` layout moves
one entry up that list, so a dependency page reads *Instance Graph ·
Schema Graph · Namespaces · Classes …*: two peer graphs and some
orphaned sections, with the only cue to the page's halves the accidental
adjacency of the two "Graph" entries.

Namespaces already refuses to belong to either half. The template hoists
the prefix table out of the schema sections when those are omitted,
because the instance cards expand their CURIEs through it; the table is
schema-declared and page-scoped, and that fact lives in a conditional.

The metadata card at the top describes the schema: its IRI, version and
description. On a dependency page that is the dependency's identity,
while the records below are the repo's own. Nothing on the page says
which dataset this is, what it conforms to, what it targets, or how many
records it holds; the dataset gets a `Source:` line inside its section.

## Design

Three groups. The page-level one exists because the prefix map has to
live somewhere that survives every composition.

    Overview        the page: identity and the prefix map
        Metadata        IRI, version, description
        Namespaces      the prefix table both halves resolve against

    Schema          the T-box; absent when `schema_sections = false`
        Graph
        Classes
        Slots
        Enumerations
        Types

    Instances       the A-box; absent when no dataset is on the page
        [dataset switcher, when more than one]
        Metadata        source, what the records conform to, how many,
                        and the container's own declared fields
        Graph
        Individuals

The sidebar renders each group as a label with its links beneath. The
body renders Overview at the top, then the Schema and Instances blocks
in the configured order: `schema-first` (the default) or
`instances-first`, which is now a swap of two blocks rather than a move
of one entry. Omitting the schema sections removes the Schema block; the
hoisted includes that today carry the graph shell and the namespace
table around that case retire, since Overview always renders them.

**Deep links are kept.** Every section id that exists today keeps its
target: `#metadata`, `#namespaces`, `#graph-visualization`, `#classes`,
`#slots`, `#enums`, `#types`, and `#individuals` on the Instances block
as a whole. The Instances group's children get new ids:
`#instance-metadata`, `#instance-graph`, `#instance-individuals`.

**Counts stay where the reader already finds them.** The graph entries
keep their `nodes / edges` badge with its tooltip; the Individuals entry
carries the record count. A visible legend for the badges is deferred
with the open questions below.

## Decisions

- **Group labels are labels, not links.** The cheapest structure that
  reads as grouping: a label with its links indented beneath. A label
  that linked somewhere would also be a scroll-spy target, and the two
  highlights would compete.
- **Namespaces moves above the schema graph.** It sits under Overview,
  which puts it before the Schema block on a schema-first page. That is
  a visible change on every page and is the point: the table is the
  page's, not the schema reference's.
- **The instances metadata card is per dataset** and swaps with the
  selector, like the graph and the cards. "Conforms to" names the
  page's schema and version, which is the one fact the dependency page
  had nowhere to say.

## Open questions (not in scope here)

- **Datasets in the nav.** A page with several datasets lists them only
  in the body's switcher; the second one is not discoverable from the
  sidebar. Listing each under Instances needs the selector to follow a
  nav click.
- **A legend for the badges.** `37 / 61` has a tooltip but no visible
  key.

## Vertical Slices

### Slice 1: Three groups in the sidebar and the body ✅ Complete

**User story:** As a reader, I want the sidebar to show which entries
describe the page, the schema and the data, and the body to be laid out
in those same blocks, so a page that leads with its instances reads as
two halves trading places.

**Acceptance criteria:**
- [x] The sidebar shows three labeled groups, Overview, Schema and
  Instances, each with its entries beneath it; a group with nothing to
  list is absent (Schema when the schema sections are off, Instances
  when no dataset is on the page).
- [x] Namespaces is an Overview entry and renders in the Overview block,
  before the Schema block, on every composition; the data-only page
  reaches it with no special case.
- [x] The `instances-first` layout swaps the Schema and Instances blocks
  and their sidebar groups whole; Overview stays first either way.
- [x] Every existing section id resolves to the same part of the page as
  before; the Instances group's entries link to their own sections.
- [x] Clicking a grouped entry navigates to its section and the scroll
  spy marks it active, in every browser the e2e tier runs.

### Slice 2: The instances metadata card

**User story:** As a reader of a dependency page, I want to read which
dataset I am looking at, what it conforms to and how many records it
holds, where today I read the dependency's IRI and a source line.

**Acceptance criteria:**
- [ ] Each dataset has a metadata card under Instances listing its
  source file, the schema and version its records conform to, its
  record count, and the container's own declared scalar fields (a
  benchmark's target schema, dataset and version, when authored); the
  card swaps with the dataset selector.
- [ ] The Instances group's Graph entry carries the dataset's
  `nodes / edges` badge and its Individuals entry the record count; the
  group label itself carries no count.
- [ ] A dataset embedded in the schema (OWL individuals) reads "embedded
  in the schema" as its source and still gets the card.

### Slice 3: Datasets in the navigation — proposed

Each dataset listed under Instances; a nav click selects it in the
body. Not scheduled; see the open questions.

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: three groups | Must Have | — | Complete |
| Slice 2: instances metadata card | Should Have | Slice 1 | Not Started |
| Slice 3: datasets in the navigation | Could Have | Slice 1 | Proposed |

## Definition of Done

- [ ] Slices 1 and 2 acceptance criteria met
- [ ] `cargo nextest run` green, including the e2e tier; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`
- [x] Screenshots of the reference page and a data-first page, before and after, reviewed (slice 1, 2026-10-09)
- [ ] CHANGELOG.md and README.md updated; the changelog names the new section ids and states that existing ones are kept

## Notes / Things to Watch

- The scroll spy observes `section[id]` and the metadata card; the new
  Instances children must be sections with ids, or the spy skips them.
- The e2e tests select sidebar entries by `href`; the Instances link
  moves from `#individuals` to the group's children.
- The mdbook toolbar links target class and slot anchors, not section
  ids, so the book integration is unaffected.
