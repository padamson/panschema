# ADR-006: "slot" is the canonical term for a schema relation

## Status

Accepted (2026-06-14)

## Context

panschema rendered the same entity under two names. The schema graph
(`panschema-viz`, driven by the LinkML IR) calls a relation a **slot**
— the node legend says "Slot", the hover says `Type: Slot`. The HTML
document, whose card lineage came from the OWL/RDF side, called the same
thing a **property** — a "Properties" section and sidebar entry, and an
`Object Property` / `Datatype Property` badge on each card.

They are the same thing. LinkML's metamodel term is `slot`; the author
writes `slots:` in the YAML; OWL/RDF calls it a property and further
splits it into object vs datatype properties by whether the range is a
class or a literal. A reader moving between the graph and the doc body
sees two vocabularies for one concept (surfaced as authoring friction
during the scimantic-schema dogfood).

## Decision

**Use "slot" as the single user-facing term**, matching panschema's
LinkML IR and the graph.

- The HTML "Properties" section, sidebar entry, and section heading
  become **"Slots"** (`id="slots"`, `#slots` anchor).
- The per-card badge reads **"Slot"** instead of `Object Property` /
  `Datatype Property`. The object-vs-datatype distinction is conveyed by
  the card's existing **Range** row (a class link vs a datatype name),
  so no information is lost; the OWL-specific qualifier is dropped.
- This holds regardless of source format: an OWL-imported schema also
  reads as "slot", because the reader normalizes everything to the
  LinkML IR before rendering.

**Internal identifiers are renamed too**, so the code is
self-documenting and doesn't carry a second vocabulary the next reader
has to translate: `PropertyData` → `SlotData`, `property_card.html` →
`slot_card.html`, the `#prop-<name>` card ids → `#slot-<name>`,
`property_type` → `slot_type`, `PropertyCardComponent` →
`SlotCardComponent`, and the `property` / `properties` template vars and
`.property-badge` / `.prop-ref` CSS classes follow suit.

## Consequences

- The graph and the HTML doc use one vocabulary; the graph hover (which
  reuses the HTML card and maps node id → `#slot-<name>`) is consistent
  with the legend and node labels.
- The `#properties` anchor and `#prop-<name>` card ids become `#slots`
  and `#slot-<name>` — a breaking change to any external deep link,
  acceptable pre-1.0.
- "slot" is the single term in the source as well as the output, so a
  contributor reading the writer code sees the same word an author and a
  reader of the docs see.
