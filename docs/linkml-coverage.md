# LinkML specification coverage

Tracks which LinkML metamodel metaslots panschema supports, and — for the
ones it does — **which writers actually surface them**. The goal is to make
the silent-drop class of bug visible: a field can exist in the IR yet render
nowhere (e.g. `any_of`, `exact_mappings`, and `subclass_of` each parsed-and-
vanished before they were wired through). This is the measurable backstop
[ADR-003](adr/003-linkml-as-internal-representation.md) gestured at ("expand
toward full SpecificationSubset coverage") but never operationalized, and the
target for [feature 08](features/08-bootstrap-linkml-ir.md) (metaschema-
bootstrapped IR, which would make column **IR** hold by construction).

## How to read this

Two different kinds of "support" are tracked separately:

1. **IR** — is the metaslot modeled as a field on the Rust IR
   ([linkml.rs](../panschema/src/linkml.rs))? A field that isn't modeled is
   parsed-and-dropped by serde with no error (no `deny_unknown_fields`).
2. **Render/emit** — does each writer actually surface a modeled field?
   - **HTML** — [html_writer.rs](../panschema/src/html_writer.rs) + templates
   - **Graph** — [graph_writer.rs](../panschema/src/graph_writer.rs) (nodes/edges/hover metadata)
   - **RDF** — [rdf_serializers.rs](../panschema/src/rdf_serializers.rs) + [owl_writer.rs](../panschema/src/owl_writer.rs)
   - **Rust** — [rust_writer.rs](../panschema/src/rust_writer.rs) codegen

Legend: ● full · ◐ partial / indirect · ○ modeled but inert (silent-drop
risk) · — not applicable to this writer · ✗ not modeled in the IR.

The metaslot inventory below is resolved from the upstream LinkML metaschema
(`linkml/linkml-model`, `metamodel_version` 1.11.0) — direct slots plus those
inherited via `is_a` / `mixins` (`element` → `common_metadata` / `extensible`
/ `annotatable`; `definition`; `slot_expression`; etc.). Only metaslots
relevant to the entities panschema models are listed; the editorial/provenance
long tail is collapsed (see each section's last row).

---

## Common metadata (applies to every definition)

LinkML's `common_metadata` mixin gives ~35 shared metaslots to schema, class,
slot, enum, type, and permissible-value alike. panschema models only a few:

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `description` | ● | ● | ● | ● | ● | markdown + `[[xref]]` in HTML; tooltip in graph; `rdfs:comment`; doc-comment |
| `annotations` | ● | ◐ | ◐ | ◐ | ○ | generic map; only `panschema:*` keys consumed (label, individuals, owl_property_type) |
| `title` | ◐ | ● | ◐ | ● | ✗ | modeled on schema only; `rdfs:label` on the ontology |
| `exact_mappings` `close_mappings` `related_mappings` `narrow_mappings` `broad_mappings` | ● | ● | ○ | ● | ○ | modeled on class + slot; HTML "Mappings" row; RDF `skos:*Match` (round-trips: OWL reader reads them back); graph/Rust ignore |
| `deprecated` | ● | ● | — | ● | — | modeled on schema/class/slot/enum/type; HTML "Deprecated" badge + note; `owl:deprecated true` on class/slot IRI (round-trips as a boolean — OWL reader reads it back into the flag; the note text is RDF-lossy); graph/Rust ignore |
| `aliases` `see_also` | ● | ● | — | ● | — | modeled on schema/class/slot/enum/type; HTML "Aliases" row + "See also" CURIE-expanded links; RDF `skos:altLabel` + `rdfs:seeAlso` on class/slot IRI (round-trips: OWL reader reads them back); graph/Rust ignore |
| `examples` | ● | ● | — | n/a | — | modeled on schema/class/slot/enum/type; HTML "Examples" section listing each `value` + optional `description`; no standard RDF predicate; graph/Rust ignore |
| `comments` `notes` `todos` `in_subset` `rank` `status` `keywords` `categories` `created_by` `modified_by` `source` `structured_aliases` `alt_descriptions` `contributors` `created_on` `last_updated_on` … | ✗ | — | — | — | — | not modeled (except `contributors`/`created`/`modified` on schema, RDF-only — see below). Editorial/provenance long tail; biggest doc-completeness gap |

---

## SchemaDefinition

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ◐ | ● | sidebar/title; node label; codegen comment |
| `id` | ● | ● | — | ● | ✗ | metadata card IRI; ontology IRI subject; `owl:versionIRI` base |
| `title` | ● | ● | ● | ● | ✗ | |
| `description` | ● | ● | — | ● | ✗ | `rdfs:comment` on ontology |
| `version` | ● | ● | — | ● | ● | `owl:versionInfo`; codegen comment |
| `license` | ● | ○ | — | ● | ✗ | **RDF-only** (`dcterms:license`); HTML drops it |
| `contributors` | ● | ○ | — | ● | ✗ | **RDF-only** (`dcterms:creator`); HTML drops it |
| `created` `modified` | ● | ○ | — | ● | ✗ | **RDF-only** (`dcterms:created`/`modified`); HTML drops them |
| `prefixes` | ● | ● | ◐ | ● | ✗ | namespace table; CURIE expansion; `@prefix` |
| `default_prefix` | ● | ● | ◐ | ◐ | ✗ | bare-name CURIE resolution |
| `default_range` | ● | ○ | ○ | ○ | ○ | modeled, but no writer applies it |
| `imports` | ● | ◐ | ◐ | ◐ | ◐ | local file imports resolved + merged at load time (every writer sees one schema); CURIE/remote/builtin imports + provenance rendering still pending |
| `classes` `slots` `enums` `types` | ● | ● | ● | ● | ● | the indexes the writers walk |
| `subsets` `settings` `bindings` `emit_prefixes` `source_file` `metamodel_version` `generation_date` … | ✗ | — | — | — | — | not modeled |

---

## ClassDefinition

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | struct/trait name in codegen; Rust keyword names emitted as raw identifiers |
| `description` | ● | ● | ● | ● | ● | |
| `is_a` | ● | ● | ● | ● | ● | "Subclass of"; edge; `rdfs:subClassOf`; trait + impl |
| `mixins` | ● | ● | ● | ● | ● | "Mixes in"; edges; per-mixin `rdfs:subClassOf`; supertraits |
| `abstract` | ● | ● | ● | ○ | ◐ | badge; dashed node; codegen doc-comment only |
| `slots` | ● | ● | ● | ○ | ● | resolved effective set (HTML/graph/Rust); RDF emits via slot side |
| `attributes` | ● | ◐ | ● | ○ | ● | folded into the resolved slot set |
| `slot_usage` | ● | ● | ● | ○ | ◐ | scalar overrides + "refined here"; induced per-class range computed in the resolver (slice 12.5), rendered on the class card (slice 19) and as per-class graph range edges (slice 22). Rust codegen still flattens scalar overrides only |
| `class_uri` | ● | ● | ● | ● | ✗ | card IRI; node URI; subject IRI |
| `subclass_of` (external) | ● | ● | ○ | ● | ✗ | "Subclass of (external)"; `rdfs:subClassOf <external>`; graph ignores |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | see Common metadata |
| `union_of` `defining_slots` `tree_root` `unique_keys` `rules` `classification_rules` `disjoint_with` `class_expression` (`any_of`/`all_of`/`exactly_one_of`/`none_of`/`slot_conditions`) | ✗ | — | — | — | — | not modeled. `rules` + `unique_keys` are the high-value validation gaps |

---

## SlotDefinition

The metaschema's largest class (~117 resolved metaslots). panschema models a
focused subset of the structural ones.

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | field name (snake_case) in codegen; Rust keyword names emitted as raw identifiers |
| `description` | ● | ● | ● | ● | ● | |
| `range` | ● | ● | ● | ● | ● | "Range" row; edge; `rdfs:range`; field type |
| `domain` | ● | ◐ | ● | ● | ○ | HTML infers from class membership; `rdfs:domain`; Rust uses class-side `slots:` |
| `required` | ● | ● | ● | ○ | ● | characteristic badge; `Option<T>` framing |
| `multivalued` | ● | ● | ● | ○ | ● | characteristic badge; `Vec<T>` framing |
| `minimum_cardinality` `maximum_cardinality` | ● | ● | ● | ○ | ● | `min..max` badge; effective-cardinality overlay |
| `pattern` | ● | ● | ● | ○ | ○ | "Pattern" row (truncated + tooltip); not enforced in RDF/Rust |
| `identifier` | ● | ● | ● | ○ | ○ | characteristic badge; not surfaced in RDF/Rust |
| `inverse` | ● | ● | ● | ● | ○ | "Inverse of"; edge; `owl:inverseOf` |
| `slot_uri` | ● | ● | ● | ● | ✗ | card IRI; node URI; subject IRI |
| `any_of` | ● | ● | ● | ○ | ● | union on card; one range edge per member; `#[serde(untagged)]` enum |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | see Common metadata |
| `symmetric` `asymmetric` `reflexive` `irreflexive` `transitive` | ● | ● | — | ● | — | OWL relationship characteristics: card badge + `owl:<Name>Property` axiom; round-trips (OWL reader reads the axioms back into the flags) |
| `ifabsent` | ● | ● | — | — | ● | schema-encoded default. Rust: enum and scalar (`int`/`float`/`double`/`string`/boolean) forms generate a non-`Option` field with `#[serde(default)]` + default fn; HTML "Default" row shows the value |
| `key` `designates_type` `subproperty_of` `singular_name` `recommended` `slot_group` `unit` `implicit_prefix` `readonly` `shared` `list_elements_unique`/`_ordered` | ✗ | — | — | — | — | not modeled. `subproperty_of` (`rdfs:subPropertyOf`) would further enrich RDF/OWL |
| `minimum_value` `maximum_value` | ● | ● | — | ○ | — | numeric value bounds: `≥`/`≤` card badge (feature 14 slice 2); RDF `owl:withRestrictions` facet deferred (slice 2b) |
| `equals_string` `equals_string_in` `equals_number` `equals_expression` `exact_cardinality` `has_member` `all_members` `structured_pattern` `range_expression` `all_of` `exactly_one_of` `none_of` `array` | ✗ | — | — | — | — | not modeled. Value/boolean-expression constraints (a validation-feature family) |

---

## EnumDefinition + PermissibleValue

The HTML **Enumerations** section ([feature 02 slice 18](features/02-core-ontology-documentation.md))
renders an enum card per enum; the graph hover reuses it.

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `EnumDefinition.name` | ● | ● | ● | ✗ | ● | `#enum-` card; node; Rust enum (keyword names → raw identifiers) |
| `EnumDefinition.description` | ● | ● | ● | ✗ | ● | card; tooltip; doc-comment |
| `permissible_values` | ● | ● | ● | ✗ | ● | card list; graph hover; Rust variants (keyword names → raw identifiers). No RDF representation |
| `PermissibleValue.text` | ● | ● | ● | ✗ | ● | card; variant ident |
| `PermissibleValue.description` | ● | ● | ● | ✗ | ● | |
| `PermissibleValue.meaning` | ● | ● | ● | ✗ | ○ | CURIE-expanded hyperlink on the card + graph; Rust ignores |
| `enum_uri` `code_set` `pv_formula` `include` `minus` `inherits` `reachable_from` `matches` `concepts` | ✗ | — | — | — | — | not modeled. Dynamic/derived enums |

---

## TypeDefinition

The HTML **Types** section ([feature 02 slice 18](features/02-core-ontology-documentation.md))
renders a type card per type; the graph hover reuses it. Types still produce no RDF.

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ✗ | ◐ | `#type-` card; node; primitives handled by hardcoded range mapping, not type defs |
| `description` | ● | ● | ● | ✗ | ○ | card; tooltip |
| `typeof` | ● | ● | ● | ✗ | ○ | "Type of" row; `type_of` edge in graph |
| `uri` | ● | ● | ● | ✗ | ○ | card URI row; node URI |
| `pattern` | ● | ● | ○ | ✗ | ○ | card Pattern row |
| `base` `repr` `type_uri` `minimum_value` `maximum_value` `union_of` | ✗ | — | — | — | — | not modeled |

---

## Priority gaps

Ordered by impact, with the slices already filed against each:

1. ~~**`slot_usage` induced ranges**~~ **(done).** Per-class range narrowing
   (`range ∩ any_of`, `maximum_cardinality: 0`) is computed by the resolver as
   an `InducedRange` view
   ([feature 12 slice 12.5](features/12-linkml-ir-resolver-services.md) ✅),
   rendered on the class card
   ([feature 02 slice 19](features/02-core-ontology-documentation.md) ✅), and
   drawn as per-class graph range edges
   ([feature 04 slice 22](features/04-schema-force-graph-visualization.md) ✅).
   Remaining tail: Rust codegen still applies only scalar `slot_usage`
   overrides, not the induced-range narrowing.
2. ~~**Enum + Type HTML sections**~~ **(done).** Enumerations and Types now
   render as doc-body card sections, and the graph hover reuses them
   ([feature 02 slice 18](features/02-core-ontology-documentation.md) ✅) —
   every node kind the graph draws has a matching HTML card.
3. **Schema metadata in HTML** (`license`, `contributors`, `created`,
   `modified` render in RDF but not the doc body). Unfiled — a "Schema info"
   card would close it.
4. **Validation-feature families** (mostly not modeled): slot value bounds
   `minimum_value` / `maximum_value` are modeled + rendered as card badges
   ([feature 14 slice 2](features/14-slot-constraints.md) ✅; their RDF
   `owl:withRestrictions` facet is deferred, slice 2b). Still not modeled:
   class `rules` / `unique_keys`, `equals_*`, and boolean expressions
   (`all_of` / `exactly_one_of` / `none_of`). Route to
   [feature 07](features/07-schema-validation.md).
5. **Editorial/provenance metadata** (not modeled): `comments`,
   `in_subset`. Documentation completeness; low individual cost, high
   collective coverage. (`aliases`, `see_also`, `deprecated`, and
   `examples` are now modeled — see Common metadata; the first three also
   round-trip through RDF.)
6. ~~**Property characteristics**~~ **(mostly done).** The five OWL
   relationship characteristics — `symmetric`, `asymmetric`, `reflexive`,
   `irreflexive`, `transitive` — are modeled and emit `owl:<Name>Property`
   axioms + card badges ([feature 14 slice 1](features/14-slot-constraints.md) ✅).
   Remaining tail: `subproperty_of` (`rdfs:subPropertyOf`).
7. **Dynamic enums / imports resolution**: `reachable_from`, `code_set`;
   `imports` of local files now resolve + merge at load time, so a schema
   split across files renders as one. CURIE/remote/builtin (`linkml:*`)
   imports and import provenance in the rendered docs are still pending.
8. **Subsets** (not modeled): `subsets` on the schema + `in_subset` per
   element would enable subset-scoped documentation (render only the terms in
   a named profile). Self-contained, additive.

The structural answer to columns **IR** drifting from the spec is
[feature 08](features/08-bootstrap-linkml-ir.md) — generate the IR from the
metaschema so every field is modeled by construction. It does not fill the
render columns; those stay per-writer work tracked here.

## Maintaining this matrix

Regenerate by diffing the IR ([linkml.rs](../panschema/src/linkml.rs))
against the upstream metaschema (`linkml/linkml-model`,
`linkml_model/model/schema/meta.yaml`) and re-walking each writer. The render
columns shift whenever a writer learns a new field; update the relevant row in
the same change.
