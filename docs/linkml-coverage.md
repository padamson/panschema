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
| `exact_mappings` `close_mappings` `related_mappings` `narrow_mappings` `broad_mappings` | ● | ● | ○ | ● | ○ | modeled on class + slot; HTML "Mappings" row; RDF `skos:*Match`; graph/Rust ignore |
| `aliases` `see_also` `deprecated` `comments` `notes` `todos` `examples` `in_subset` `rank` `status` `keywords` `categories` `created_by` `modified_by` `source` `structured_aliases` `alt_descriptions` `contributors` `created_on` `last_updated_on` … | ✗ | — | — | — | — | not modeled (except `contributors`/`created`/`modified` on schema, RDF-only — see below). Editorial/provenance long tail; biggest doc-completeness gap |

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
| `imports` | ● | ○ | ○ | ○ | ○ | tracked, never resolved or rendered |
| `classes` `slots` `enums` `types` | ● | ● | ● | ● | ● | the indexes the writers walk |
| `subsets` `settings` `bindings` `emit_prefixes` `source_file` `metamodel_version` `generation_date` … | ✗ | — | — | — | — | not modeled |

---

## ClassDefinition

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | struct/trait name in codegen |
| `description` | ● | ● | ● | ● | ● | |
| `is_a` | ● | ● | ● | ● | ● | "Subclass of"; edge; `rdfs:subClassOf`; trait + impl |
| `mixins` | ● | ● | ● | ● | ● | "Mixes in"; edges; per-mixin `rdfs:subClassOf`; supertraits |
| `abstract` | ● | ● | ● | ○ | ◐ | badge; dashed node; codegen doc-comment only |
| `slots` | ● | ● | ● | ○ | ● | resolved effective set (HTML/graph/Rust); RDF emits via slot side |
| `attributes` | ● | ◐ | ● | ○ | ● | folded into the resolved slot set |
| `slot_usage` | ● | ◐ | ◐ | ○ | ◐ | scalar overrides + "refined here"; induced per-class range now computed in the resolver (slice 12.5), **not yet rendered** — see Priority gaps |
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
| `name` | ● | ● | ● | ● | ● | field name (snake_case) in codegen |
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
| `key` `designates_type` `subproperty_of` `symmetric` `transitive` `reflexive` `asymmetric` `irreflexive` `singular_name` `ifabsent` `recommended` `slot_group` `unit` `implicit_prefix` `readonly` `shared` `list_elements_unique`/`_ordered` | ✗ | — | — | — | — | not modeled. Property characteristics (`symmetric`/`transitive`/…) would enrich RDF/OWL |
| `minimum_value` `maximum_value` `equals_string` `equals_string_in` `equals_number` `equals_expression` `exact_cardinality` `has_member` `all_members` `structured_pattern` `range_expression` `all_of` `exactly_one_of` `none_of` `array` | ✗ | — | — | — | — | not modeled. Value/boolean-expression constraints (a validation-feature family) |

---

## EnumDefinition + PermissibleValue

**No HTML section exists yet** — enums and types render in the graph (and
codegen) but have no doc-body card. Filed as [feature 02 slice 18](features/02-core-ontology-documentation.md).

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `EnumDefinition.name` | ● | ○ | ● | ✗ | ● | `[[xref]]` resolves to `#enum-` but no card target; node; Rust enum |
| `EnumDefinition.description` | ● | ○ | ● | ✗ | ● | tooltip; doc-comment |
| `permissible_values` | ● | ○ | ● | ✗ | ● | graph hover list; Rust variants. No RDF representation |
| `PermissibleValue.text` | ● | ○ | ● | ✗ | ● | variant ident |
| `PermissibleValue.description` | ● | ○ | ● | ✗ | ● | |
| `PermissibleValue.meaning` | ● | ○ | ● | ✗ | ○ | CURIE-expanded in graph; Rust ignores |
| `enum_uri` `code_set` `pv_formula` `include` `minus` `inherits` `reachable_from` `matches` `concepts` | ✗ | — | — | — | — | not modeled. Dynamic/derived enums |

---

## TypeDefinition

**No HTML section exists yet** — same gap as enums (feature 02 slice 18).
Types also produce no RDF.

| Metaslot | IR | HTML | Graph | RDF | Rust | Notes |
|---|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ○ | ● | ✗ | ◐ | node; primitives handled by hardcoded range mapping, not type defs |
| `description` | ● | ○ | ● | ✗ | ○ | tooltip |
| `typeof` | ● | ○ | ● | ✗ | ○ | `type_of` edge in graph |
| `uri` | ● | ○ | ● | ✗ | ○ | node URI |
| `pattern` | ● | ○ | ○ | ✗ | ○ | modeled, surfaced nowhere |
| `base` `repr` `type_uri` `minimum_value` `maximum_value` `union_of` | ✗ | — | — | — | — | not modeled |

---

## Priority gaps

Ordered by impact, with the slices already filed against each:

1. **`slot_usage` induced ranges** (computed in the IR, not yet rendered).
   Per-class range narrowing (`range ∩ any_of`, `maximum_cardinality: 0`) is
   now computed by the resolver as an `InducedRange` view
   ([feature 12 slice 12.5](features/12-linkml-ir-resolver-services.md) ✅),
   but cards and the graph still render the inherited union. Remaining: wire
   the view into [feature 02 slice 19](features/02-core-ontology-documentation.md)
   (card) + [feature 04 slice 22](features/04-schema-force-graph-visualization.md)
   (graph).
2. **Enum + Type HTML sections** (modeled, inert in HTML). Graph-only today.
   → [feature 02 slice 18](features/02-core-ontology-documentation.md).
3. **Schema metadata in HTML** (`license`, `contributors`, `created`,
   `modified` render in RDF but not the doc body). Unfiled — a "Schema info"
   card would close it.
4. **Validation-feature families** (not modeled): class `rules` /
   `unique_keys`; slot value constraints (`minimum_value` / `maximum_value` /
   `equals_*`) and boolean expressions (`all_of` / `exactly_one_of` /
   `none_of`). Route to [feature 07](features/07-schema-validation.md).
5. **Editorial/provenance metadata** (not modeled): `aliases`, `see_also`,
   `deprecated`, `comments`, `examples`, `in_subset`. Documentation
   completeness; low individual cost, high collective coverage.
6. **Property characteristics** (not modeled): `symmetric`, `transitive`,
   `subproperty_of`, etc. — would enrich the RDF/OWL output specifically.
7. **Dynamic enums / imports resolution**: `reachable_from`, `code_set`;
   `imports` is tracked but never followed.
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
