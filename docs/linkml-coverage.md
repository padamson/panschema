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
   - **Postgres** — [postgres_writer.rs](../panschema/src/postgres_writer.rs) DDL ([feature 24](features/24-postgres-ddl-writer.md))

Legend: ● full · ◐ partial / indirect · ○ modeled but inert (silent-drop
risk) · — not applicable to this writer · ✗ not modeled in the IR.

A cell may also carry a **V&V square**, marking whether *that writer's
output* has been checked against an independent oracle for the target
language — not just against this codebase's own expectations of what
correct output looks like (see
[feature 25](features/25-rust-writer-output-verification.md)–[28](features/28-postgres-ddl-writer-output-verification.md)
for what each writer's oracle actually is): ■ verified (a real
parser/compiler/browser/reasoner has checked it) · ◨ partially verified
(fast syntax-tier only, no thorough/behavioral tier yet) · no square =
not yet audited for V&V, not "unverified" — **this axis is being
introduced starting with the Postgres column** (the newest writer, added
alongside its V&V harness in the same change) and extended to the other
four columns only as each is actually audited; a blank square there is
not a claim either way yet.

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

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `description` | ● | ● | ● | ● | ● | ✗ | markdown + `[[xref]]` in HTML; tooltip in graph; `rdfs:comment`; doc-comment; not emitted as `COMMENT ON` |
| `annotations` | ● | ◐ | ◐ | ◐ | ○ | ✗ | generic map; only `panschema:*` keys consumed (label, individuals, owl_property_type) |
| `title` | ◐ | ● | ◐ | ● | ✗ | ✗ | modeled on schema only; `rdfs:label` on the ontology |
| `exact_mappings` `close_mappings` `related_mappings` `narrow_mappings` `broad_mappings` | ● | ● | ○ | ● | ○ | ✗ | modeled on class + slot; HTML "Mappings" row; RDF `skos:*Match` (round-trips: OWL reader reads them back); graph/Rust/postgres ignore |
| `deprecated` | ● | ● | — | ● | — | ✗ | modeled on schema/class/slot/enum/type; HTML "Deprecated" badge + note; `owl:deprecated true` on class/slot IRI (round-trips as a boolean — OWL reader reads it back into the flag; the note text is RDF-lossy); graph/Rust/postgres ignore |
| `aliases` `see_also` | ● | ● | — | ● | — | ✗ | modeled on schema/class/slot/enum/type; HTML "Aliases" row + "See also" CURIE-expanded links; RDF `skos:altLabel` + `rdfs:seeAlso` on class/slot IRI (round-trips: OWL reader reads them back); graph/Rust/postgres ignore |
| `examples` | ● | ● | — | n/a | — | ✗ | modeled on schema/class/slot/enum/type; HTML "Examples" section listing each `value` + optional `description`; no standard RDF predicate; graph/Rust/postgres ignore |
| `comments` `notes` `todos` `in_subset` `rank` `status` `keywords` `categories` `created_by` `modified_by` `source` `structured_aliases` `alt_descriptions` `contributors` `created_on` `last_updated_on` … | ✗ | — | — | — | — | — | not modeled (except `contributors`/`created`/`modified` on schema, RDF-only — see below). Editorial/provenance long tail; biggest doc-completeness gap |

---

## SchemaDefinition

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ◐ | ● | ✗ | sidebar/title; node label; codegen comment |
| `id` | ● | ● | — | ● | ✗ | ✗ | metadata card IRI; ontology IRI subject; `owl:versionIRI` base |
| `title` | ● | ● | ● | ● | ✗ | ✗ | |
| `description` | ● | ● | — | ● | ✗ | ✗ | `rdfs:comment` on ontology |
| `version` | ● | ● | — | ● | ● | ✗ | `owl:versionInfo`; codegen comment |
| `license` | ● | ○ | — | ● | ✗ | ✗ | **RDF-only** (`dcterms:license`); HTML drops it |
| `contributors` | ● | ○ | — | ● | ✗ | ✗ | **RDF-only** (`dcterms:creator`); HTML drops it |
| `created` `modified` | ● | ○ | — | ● | ✗ | ✗ | **RDF-only** (`dcterms:created`/`modified`); HTML drops them |
| `prefixes` | ● | ● | ◐ | ● | ✗ | ✗ | namespace table; CURIE expansion; `@prefix` |
| `default_prefix` | ● | ● | ◐ | ◐ | ✗ | ✗ | bare-name CURIE resolution |
| `default_range` | ● | ○ | ○ | ○ | ○ | ✗ | modeled, but no writer applies it |
| `imports` | ● | ◐ | ◐ | ◐ | ◐ | ✗ | local file imports resolved + merged at load time (every writer sees one schema); CURIE/remote/builtin imports + provenance rendering still pending |
| `classes` `slots` `enums` `types` | ● | ● | ● | ● | ● | ●◨ | the indexes the writers walk; Postgres walks `classes`/`enums` ([feature 24 slice 1](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query` — [feature 28 slice 1](features/28-postgres-ddl-writer-output-verification.md) ✅); `slots`/`types` not applicable (no top-level slot or type table) |
| `subsets` `settings` `bindings` `emit_prefixes` `source_file` `metamodel_version` `generation_date` … | ✗ | — | — | — | — | — | not modeled |

---

## ClassDefinition

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | ●◨ | struct/trait name in codegen; Rust keyword names emitted as raw identifiers; Postgres table name ([feature 24 slice 1](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `description` | ● | ● | ● | ● | ● | ✗ | not emitted as `COMMENT ON TABLE` |
| `is_a` | ● | ● | ● | ● | ● | ◐ | "Subclass of"; edge; `rdfs:subClassOf`; trait + impl; Postgres: a class using `is_a` is detected and skipped with a diagnostic, not silently dropped, but not yet projected to a table (deferred, [feature 24 slice 6](features/24-postgres-ddl-writer.md)) |
| `mixins` | ● | ● | ● | ● | ● | ●◨ | "Mixes in"; edges; per-mixin `rdfs:subClassOf`; supertraits; Postgres flattens mixin attributes into the mixing class's table, matching how Rust flattens them (feature 24 slice 1 ✅, syntax-verified) |
| `abstract` | ● | ● | ● | ○ | ◐ | ●◨ | badge; dashed node; codegen doc-comment only; Postgres emits no table for an abstract class (deliberate — nothing to instantiate), verified via `pg_query` |
| `slots` | ● | ● | ● | ○ | ● | ●◨ | resolved effective set (HTML/graph/Rust/Postgres); RDF emits via slot side |
| `attributes` | ● | ◐ | ● | ○ | ● | ●◨ | folded into the resolved slot set (every writer, including Postgres, shares the same resolver) |
| `slot_usage` | ● | ● | ● | ○ | ◐ | ◐ | scalar overrides + "refined here"; induced per-class range computed in the resolver (slice 12.5), rendered on the class card (slice 19) and as per-class graph range edges (slice 22). Rust codegen still flattens scalar overrides only; Postgres shares the same resolver as Rust (scalar overrides flow through to column type/required) but has no dedicated test pinning this yet |
| `class_uri` | ● | ● | ● | ● | ✗ | ✗ | card IRI; node URI; subject IRI; not applicable to DDL |
| `subclass_of` (external) | ● | ● | ○ | ● | ✗ | ✗ | "Subclass of (external)"; `rdfs:subClassOf <external>`; graph ignores |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | ✗ | see Common metadata |
| `rules` | ● | ● | ◐ | ✗ | ✗ | ✗ | class-level conditional constraints: card renders each rule's title/description plus a "when … then …" sentence built from its pre/postcondition `slot_conditions` (`range`/`required`/cardinality/value bounds/`pattern`/`equals_string`/`equals_number`) ([feature 17 slice 1](features/17-class-validation-constructs.md) ✅). Graph is indirect — the class-node hover reuses the rendered HTML card, so the Rules section shows there too; no dedicated node/edge (it's not a binary relation). SHACL/RDF projection deferred (slice 4); Postgres `CHECK`-constraint projection is [feature 24 slice 3](features/24-postgres-ddl-writer.md), not yet built — a class with `rules` is currently skipped with a diagnostic rather than silently incomplete |
| `unique_keys` | ● | ● | ◐ | ✗ | ✗ | ●◨ | uniqueness constraints: card renders a "Unique keys" row per key with its slot tuple; each key slot is checked against the class's effective slot set and an unresolved slot warns at generate time ([feature 17 slice 2](features/17-class-validation-constructs.md) ✅). Graph is indirect — the class-node hover reuses the rendered HTML card, so the Unique keys row shows there too; no dedicated node/edge. No RDF/Rust projection (instance-data enforcement is the consumer's job); Postgres emits a table-level `CONSTRAINT <table>_<key>_key UNIQUE (...)` per key ([feature 24 slice 2](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`), dropping any key that names a slot the class lacks |
| `union_of` `defining_slots` `tree_root` `classification_rules` `disjoint_with` `class_expression` (`any_of`/`all_of`/`exactly_one_of`/`none_of`) | ✗ | — | — | — | — | — | not modeled, but no longer *silent*: `generate` warns on any unmodeled class key by default (`crate::diagnostics`, ignore-list starts empty) — so these and any not-yet-enumerated construct are reported. Class-level boolean expressions are the remaining high-value validation gap ([feature 17 slice 3](features/17-class-validation-constructs.md)) |

---

## SlotDefinition

The metaschema's largest class (~117 resolved metaslots). panschema models a
focused subset of the structural ones.

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | ●◨ | field name (snake_case) in codegen; Rust keyword names emitted as raw identifiers; Postgres column name ([feature 24 slice 1](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `description` | ● | ● | ● | ● | ● | ✗ | not emitted as `COMMENT ON COLUMN` |
| `range` | ● | ● | ● | ● | ● | ●◨ | "Range" row; edge; `rdfs:range`; field type; Postgres column type — scalar mapping, enum type, or FK to the target's primary key (feature 24 slice 1 ✅, syntax-verified) |
| `domain` | ● | ◐ | ● | ● | ○ | ◐ | HTML infers from class membership; `rdfs:domain`; Rust uses class-side `slots:`; Postgres likewise determines table membership via the shared resolver rather than modeling `domain` distinctly |
| `required` | ● | ● | ● | ○ | ● | ●◨ | characteristic badge; `Option<T>` framing; Postgres `NOT NULL` (feature 24 slice 1 ✅, syntax-verified) |
| `multivalued` | ● | ● | ● | ○ | ● | ◐ | characteristic badge; `Vec<T>` framing; Postgres detects and skips a class with a multivalued slot (diagnostic, not silent) — array columns (scalar range) and linking tables (class range) are [feature 24 slices 4-5](features/24-postgres-ddl-writer.md), not yet built |
| `minimum_cardinality` `maximum_cardinality` | ● | ● | ● | ○ | ● | ✗ | `min..max` badge; effective-cardinality overlay |
| `pattern` | ● | ● | ● | ○ | ○ | ●◨ | "Pattern" row (truncated + tooltip); not enforced in RDF/Rust; Postgres emits an inline `CHECK (col ~ 'pattern')` (single quotes escaped) ([feature 24 slice 2](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `identifier` | ● | ● | ● | ○ | ○ | ●◨ | characteristic badge; not surfaced in RDF/Rust; Postgres: the effective `identifier` slot becomes the primary key (feature 24 slice 1 ✅, syntax-verified) |
| `inverse` | ● | ● | ● | ● | ○ | ✗ | "Inverse of"; edge; `owl:inverseOf` |
| `slot_uri` | ● | ● | ● | ● | ✗ | ✗ | card IRI; node URI; subject IRI |
| `any_of` | ● | ● | ● | ○ | ● | ◐ | union on card; one range edge per member; `#[serde(untagged)]` enum; Postgres detects and skips a class with a polymorphic `any_of` slot (diagnostic) — no clean single mapping, deferred indefinitely ([feature 24 slice 7](features/24-postgres-ddl-writer.md)) |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | ✗ | see Common metadata |
| `symmetric` `asymmetric` `reflexive` `irreflexive` `transitive` | ● | ● | — | ● | — | — | OWL relationship characteristics: card badge + `owl:<Name>Property` axiom; round-trips (OWL reader reads the axioms back into the flags); not applicable to relational modeling |
| `ifabsent` | ● | ● | — | — | ● | ✗ | schema-encoded default. Rust: enum and scalar (`int`/`float`/`double`/`string`/boolean) forms generate a non-`Option` field with `#[serde(default)]` + default fn; HTML "Default" row shows the value; Postgres doesn't yet emit a column `DEFAULT` from it |
| `key` `designates_type` `subproperty_of` `singular_name` `recommended` `slot_group` `unit` `implicit_prefix` `readonly` `shared` `list_elements_unique`/`_ordered` | ✗ | — | — | — | — | — | not modeled. `subproperty_of` (`rdfs:subPropertyOf`) would further enrich RDF/OWL |
| `minimum_value` `maximum_value` | ● | ● | — | ○ | — | ●◨ | numeric value bounds: `≥`/`≤` card badge (feature 14 slice 2); RDF `owl:withRestrictions` facet deferred (slice 2b); Postgres emits one inline `CHECK (col >= min AND col <= max)`, or just the set side ([feature 24 slice 2](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `equals_string` `equals_string_in` `equals_number` `equals_expression` `exact_cardinality` `has_member` `all_members` `structured_pattern` `range_expression` `all_of` `exactly_one_of` `none_of` `array` | ✗ | — | — | — | — | — | not modeled. Value/boolean-expression constraints (a validation-feature family) |

---

## EnumDefinition + PermissibleValue

The HTML **Enumerations** section ([feature 02 slice 18](features/02-core-ontology-documentation.md))
renders an enum card per enum; the graph hover reuses it.

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `EnumDefinition.name` | ● | ● | ● | ✗ | ● | ●◨ | `#enum-` card; node; Rust enum (keyword names → raw identifiers); Postgres `CREATE TYPE ... AS ENUM` name ([feature 24 slice 1](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `EnumDefinition.description` | ● | ● | ● | ✗ | ● | ✗ | card; tooltip; doc-comment |
| `permissible_values` | ● | ● | ● | ✗ | ● | ●◨ | card list; graph hover; Rust variants (keyword names → raw identifiers). No RDF representation; Postgres enum value list (feature 24 slice 1 ✅, syntax-verified) |
| `PermissibleValue.text` | ● | ● | ● | ✗ | ● | ●◨ | card; variant ident; Postgres enum value literal (feature 24 slice 1 ✅, syntax-verified) |
| `PermissibleValue.description` | ● | ● | ● | ✗ | ● | ✗ | |
| `PermissibleValue.meaning` | ● | ● | ● | ✗ | ○ | ✗ | CURIE-expanded hyperlink on the card + graph; Rust ignores |
| `enum_uri` `code_set` `pv_formula` `include` `minus` `inherits` `reachable_from` `matches` `concepts` | ✗ | — | — | — | — | — | not modeled. Dynamic/derived enums |

---

## TypeDefinition

The HTML **Types** section ([feature 02 slice 18](features/02-core-ontology-documentation.md))
renders a type card per type; the graph hover reuses it. Types still
produce no RDF, and no Postgres output either — a `TypeDefinition` isn't
a table, and the Postgres writer resolves scalar ranges via its own
built-in mapping rather than consulting `TypeDefinition`.

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ✗ | ◐ | ✗ | `#type-` card; node; primitives handled by hardcoded range mapping, not type defs |
| `description` | ● | ● | ● | ✗ | ○ | ✗ | card; tooltip |
| `typeof` | ● | ● | ● | ✗ | ○ | ✗ | "Type of" row; `type_of` edge in graph |
| `uri` | ● | ● | ● | ✗ | ○ | ✗ | card URI row; node URI |
| `pattern` | ● | ● | ○ | ✗ | ○ | ✗ | card Pattern row |
| `base` `repr` `type_uri` `minimum_value` `maximum_value` `union_of` | ✗ | — | — | — | — | — | not modeled |

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
   `owl:withRestrictions` facet is deferred, slice 2b). Class `rules` and
   `unique_keys` are now modeled + rendered
   ([feature 17](features/17-class-validation-constructs.md) slices 1–2 ✅;
   `rules` RDF/SHACL projection deferred, slice 4 — generating a non-HTML
   format for a schema with `rules` warns of the gap in the meantime; a
   `unique_keys` slot the class lacks warns at generate time). Still not
   modeled: `equals_string_in` / `equals_expression` / other slot-condition
   equality forms beyond `equals_string` / `equals_number`, and class-level
   boolean expressions (`all_of` / `exactly_one_of` / `none_of`, slice 3).
   Route to [feature 17](features/17-class-validation-constructs.md)
   (class-level) / [feature 07](features/07-schema-validation.md)
   (structural validation).
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

The **V&V square** (■/◨) is being introduced incrementally, starting
with the Postgres column ([features 24](features/24-postgres-ddl-writer.md)
and [28](features/28-postgres-ddl-writer-output-verification.md) landed
together, so its cells could be marked honestly from day one). Extend it
to HTML/Graph/RDF/Rust only once each is actually audited against its own
V&V doc ([25](features/25-rust-writer-output-verification.md)–[27](features/27-rdf-owl-family-output-verification.md)) —
don't backfill a square from assumption. When a writer gains a new V&V
tier (e.g. Postgres's `testcontainers` apply test, feature 28 slice 2),
upgrade ◨ to ■ for the cells that tier actually covers, in the same
change that adds the tier.
