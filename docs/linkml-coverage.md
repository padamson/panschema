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
   - **SHACL** — [shacl_writer.rs](../panschema/src/shacl_writer.rs) validation shapes ([feature 17 slice 4](features/17-class-validation-constructs.md)); a cross-cutting constraints projection (one `sh:NodeShape` per class with property shapes for slot value-constraints), not tracked as a per-construct column in the table below
   - **JSON Schema** — [json_schema_writer.rs](../panschema/src/json_schema_writer.rs) draft-2020-12 structured-output/validation contract ([feature 32](features/32-json-schema-writer.md)); a cross-cutting projection (one closed `object` per class under `$defs`), not tracked as a per-construct column in the table below

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
| `default_range` | ● | ● | ● | ● | ● | ●◨ | materialized into rangeless slot definitions at load, per declaring file (an import's slots take its own file's default, never the root's), so every writer and the validator see a populated range; an unresolvable default is a dangling-reference warning |
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
| `slots` | ● | ● | ● | ● | ● | ●◨ | resolved effective set (HTML/graph/Rust/Postgres); the RDF family now declares a property per effective slot too (type/label/range + `rdfs:domain` from the owning class), so OWL and SHACL describe the same vocabulary |
| `attributes` | ● | ◐ | ● | ● | ● | ●◨ | folded into the resolved slot set (every writer, including the RDF family, shares the same resolver) — an inline attribute emits as an `owl:{Datatype,Object}Property` with its owning class as `rdfs:domain` |
| `slot_usage` | ● | ● | ● | ○ | ◐ | ◐ | scalar overrides + "refined here"; induced per-class range computed in the resolver (slice 12.5), rendered on the class card (slice 19) and as per-class graph range edges (slice 22). Rust codegen still flattens scalar overrides only; Postgres shares the same resolver as Rust (scalar overrides flow through to column type/required) but has no dedicated test pinning this yet |
| `class_uri` | ● | ● | ● | ● | ✗ | ✗ | card IRI; node URI; subject IRI; not applicable to DDL |
| `subclass_of` (external) | ● | ● | ● | ● | ✗ | ✗ | "Subclass of (external)"; `rdfs:subClassOf <external>`; graph draws an edge to a muted/dashed shared external category node ([feature 35](features/35-external-groundings-in-graph.md) ✅), labelled by the cached upstream `rdfs:label` (CURIE fallback), classes sharing a grounding sharing one node |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | ✗ | see Common metadata |
| `rules` | ● | ● | ● | ✗ | ✗ | ●◨ | class-level conditional constraints: card renders each rule's title/description plus a "when … then …" sentence built from its pre/postcondition `slot_conditions` (`range`/`required`/cardinality/value bounds/`pattern`/`equals_string`/`equals_number`) ([feature 17 slice 1](features/17-class-validation-constructs.md) ✅). Graph surfaces rules directly ([feature 31](features/31-rule-visualization-in-the-schema-graph.md) ✅): every node a rule touches (a trigger or governed slot, or the class that declares it) wears a persistent amber ring (explained in the graph legend), and hovering a rule entry in any card highlights the rule's participant nodes (trigger/governed slots + owning class) with an amber ring; the node hover also reuses the rendered HTML card for the full Rules section. No dedicated edge — a rule's conditional, multi-slot, `any_of` structure isn't a binary relation. SHACL emits a conditional `sh:or ( [sh:not <pre>] <post> )` shape per rule ([feature 17 slice 4](features/17-class-validation-constructs.md) ✅, `oxigraph`-verified — see the SHACL writer bullet above), typing an `equals_number` `sh:hasValue` from the slot's range (an integer range gets an `xsd:integer` literal, not `xsd:double`) and projecting `value_presence` (`PRESENT`→`sh:minCount 1`, `ABSENT`→`sh:maxCount 0`) and both `any_of` forms (alternative slot values and alternative condition sets) as `sh:or` shapes, and skipping with a diagnostic any rule it still can't express — one-sided, a condition side with neither `slot_conditions` nor `any_of`, or a condition naming a slot the class lacks; Postgres emits a conditional `CONSTRAINT <table>_rule<n>_check CHECK (NOT (pre) OR (post))` per rule ([feature 24 slice 3](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`), skipping with a diagnostic any rule with no single-column CHECK form (one-sided, or a `range`/cardinality condition). `validate --data` enforces rules natively too ([feature 34 slice 7](features/34-validate-instance-data.md) ✅): a record whose precondition holds must satisfy the postcondition, over the same facets SHACL projects (`equals_string`/`equals_number`, `value_presence`, `required`, both `any_of` forms) plus bounds, `pattern`, and cardinality inside a condition — so the single-tool check and the SHACL check agree on what a rule means; a `range:` inside a condition is a type assertion and is not evaluated |
| `unique_keys` | ● | ● | ◐ | ✗ | ✗ | ●◨ | uniqueness constraints: card renders a "Unique keys" row per key with its slot tuple; each key slot is checked against the class's effective slot set and an unresolved slot warns at generate time ([feature 17 slice 2](features/17-class-validation-constructs.md) ✅). Graph is indirect — the class-node hover reuses the rendered HTML card, so the Unique keys row shows there too; no dedicated node/edge. No RDF/Rust projection (instance-data enforcement is the consumer's job); Postgres emits a table-level `CONSTRAINT <table>_<key>_key UNIQUE (...)` per key ([feature 24 slice 2](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`), dropping any key that names a slot the class lacks |
| `tree_root` | ● | ● | ● | ● | — | — | modeled on the IR ([feature 33](features/33-linkml-instance-reader.md)): marks the data-container class an instance-data file is a single instance of. Drives the JSON-Schema writer's document root `$ref` and is the entry point for the LinkML instance reader (`generate --instances data.yaml`), which walks the container into the first-class instance model and renders it as the HTML instance graph. `panschema validate --data` walks the same container to check each record against its class's constraints ([feature 34](features/34-validate-instance-data.md)). Feature 36 wires the resulting A-box through the outputs: RDF-family emission as `owl:NamedIndividual`s, an `instance-graph-json` document, the navigable HTML instance section with unified cards, and `publish` `[[instances]]` carriage — all sharing one IRI minting. Rust/Postgres don't surface it |
| `union_of` `defining_slots` `classification_rules` `disjoint_with` `class_expression` (`any_of`/`all_of`/`exactly_one_of`/`none_of`) | ✗ | — | — | — | — | — | not modeled, but no longer *silent*: `generate` warns on any unmodeled class key by default (`crate::diagnostics`, ignore-list starts empty) — so these and any not-yet-enumerated construct are reported. Class-level boolean expressions are the remaining high-value validation gap ([feature 17 slice 3](features/17-class-validation-constructs.md)) |

---

## SlotDefinition

The metaschema's largest class (~117 resolved metaslots). panschema models a
focused subset of the structural ones.

| Metaslot | IR | HTML | Graph | RDF | Rust | Postgres | Notes |
|---|:--:|:--:|:--:|:--:|:--:|:--:|---|
| `name` | ● | ● | ● | ● | ● | ●◨ | field name (snake_case) in codegen; Rust keyword names emitted as raw identifiers; Postgres column name ([feature 24 slice 1](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `description` | ● | ● | ● | ● | ● | ✗ | not emitted as `COMMENT ON COLUMN` |
| `range` | ● | ● | ● | ● | ● | ●◨ | "Range" row; edge; `rdfs:range` (a scalar's XSD datatype or a class range's IRI; an **enum** range emits no `rdfs:range` — enums have no RDF form yet, so it's guarded rather than fabricating a nonexistent `xsd:{EnumName}`); field type; Postgres column type — scalar mapping, enum type, or FK to the target's primary key (feature 24 slice 1 ✅, syntax-verified) |
| `domain` | ● | ◐ | ● | ● | ○ | ◐ | HTML infers from class membership; `rdfs:domain`; Rust uses class-side `slots:`; Postgres likewise determines table membership via the shared resolver rather than modeling `domain` distinctly |
| `required` | ● | ● | ● | ○ | ● | ●◨ | characteristic badge; `Option<T>` framing; Postgres `NOT NULL`, derived from the *effective* lower bound so an explicit `minimum_cardinality ≥ 1` also drives it (feature 24 slice 1 ✅, syntax-verified). SHACL reconciles `required` and `minimum_cardinality` into a single `sh:minCount` (explicit cardinality wins) rather than emitting a contradictory pair |
| `multivalued` | ● | ● | ● | ○ | ● | ◐ | characteristic badge; `Vec<T>` framing; Postgres emits an **array column** for a scalar or enum range (`text[]`, `integer[]`, enum arrays — [feature 24 slice 4](features/24-postgres-ddl-writer.md) ✅, syntax-verified), and a **linking table** for a multivalued class range — `<owner>_<slot>`, both sides `NOT NULL`, the pair as primary key, named for the slot so two slots onto one class stay distinct ([slice 5](features/24-postgres-ddl-writer.md) ✅, syntax-verified). A `pattern` or value bound on a multivalued slot is per-element and has no `CHECK` form over an array column, so it is dropped and reported rather than emitted. List **order is not preserved** in either form |
| `minimum_cardinality` `maximum_cardinality` | ● | ● | ● | ○ | ● | ◐ | `min..max` badge; effective-cardinality overlay. Postgres projects `minimum_cardinality` indirectly — `min ≥ 1` folds into the column's `NOT NULL` via the shared effective-cardinality view; `maximum_cardinality` has no column form yet (a `> 1` upper bound is the multivalued/array case, [feature 24 slices 4-5](features/24-postgres-ddl-writer.md)) |
| `pattern` | ● | ● | ● | ○ | ○ | ●◨ | "Pattern" row (truncated + tooltip); not enforced in RDF/Rust; Postgres emits an inline `CHECK (col ~ 'pattern')` (single quotes escaped) ([feature 24 slice 2](features/24-postgres-ddl-writer.md) ✅, syntax-verified via `pg_query`) |
| `identifier` | ● | ● | ● | ○ | ○ | ●◨ | characteristic badge; not surfaced in RDF/Rust; Postgres: the effective `identifier` slot becomes the primary key (feature 24 slice 1 ✅, syntax-verified) |
| `inverse` | ● | ● | ● | ● | ○ | ✗ | "Inverse of"; edge; `owl:inverseOf` |
| `slot_uri` | ● | ● | ● | ● | ✗ | ✗ | card IRI; node URI; subject IRI |
| `any_of` | ● | ● | ● | ● | ● | ◐ | union on card; one range edge per member; `#[serde(untagged)]` enum; a union whose members are all classes emits in RDF as an `owl:ObjectProperty` whose `rdfs:range` is a class expression over `owl:unionOf` of the members — and instance values at such a slot ingest as references, so the A-box asserts object properties rather than literals ([feature 34 slice 4b](features/34-validate-instance-data.md) ✅); Postgres detects and skips a class with a polymorphic `any_of` slot (diagnostic) — no clean single mapping, deferred indefinitely ([feature 24 slice 7](features/24-postgres-ddl-writer.md)) |
| `*_mappings` (5) | ● | ● | ○ | ● | ○ | ✗ | see Common metadata |
| `symmetric` `asymmetric` `reflexive` `irreflexive` `transitive` | ● | ● | — | ● | — | — | OWL relationship characteristics: card badge + `owl:<Name>Property` axiom; round-trips (OWL reader reads the axioms back into the flags); not applicable to relational modeling |
| `ifabsent` | ● | ● | — | — | ● | ✗ | schema-encoded default. Rust: enum and scalar (`int`/`float`/`double`/`string`/boolean) forms generate a non-`Option` field with `#[serde(default)]` + default fn; HTML "Default" row shows the value; Postgres doesn't yet emit a column `DEFAULT` from it |
| `key` | ● | ○ | ○ | ○ | ○ | ●◨ | identifies records within their container: the record-id slot for instance data (scoping per dataset — see feature 41), and the Postgres primary key when no `identifier` exists. Not yet surfaced as a card badge |
| `is_a` (slot) | ● | ● | — | ● | — | — | slot specialization: "Specializes" card line; `rdfs:subPropertyOf` (read back by the OWL reader for parents the ontology itself defines; several axioms project deterministically onto the single-valued field); `validate` enforces per-record value containment; a class using the child without the parent is warned. **Divergence:** the parent's field values are not inherited onto the child (linkml-runtime induces them); a `slot_usage`-declared `is_a` is class-scoped — enforced by `validate`, deliberately not emitted as a global RDF axiom |
| `designates_type` `subproperty_of` `singular_name` `recommended` `slot_group` `unit` `implicit_prefix` `readonly` `shared` `list_elements_unique`/`_ordered` | ✗ | — | — | — | — | — | not modeled. `subproperty_of` (an *external* `rdfs:subPropertyOf` target URI) would complement slot-level `is_a`, which covers the in-schema case |
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
| `permissible_values` | ● | ● | ● | ● | ● | ●◨ | card list; graph hover; Rust variants (keyword names → raw identifiers). RDF emits the enum as an `owl:Class` closed by `owl:oneOf` over its values, each a labelled `owl:NamedIndividual` of that class (a value's `meaning:` CURIE supplies its IRI when given); an enum-ranged slot is therefore an `owl:ObjectProperty` over those individuals with the enum class as its `rdfs:range`, and an individual's enum-valued assertion names the value's IRI rather than a literal (a value the enum doesn't permit stays a literal and is reported by validation); the SHACL shape for an enum-ranged slot closes the value set with `sh:in` over those same value IRIs, so data carrying an unlisted value fails validation instead of passing unconstrained — a rule *condition* on such a slot is deliberately not closed this way, since the condition's own `sh:hasValue` would then be unsatisfiable; Postgres enum value list (feature 24 slice 1 ✅, syntax-verified) |
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
   `unique_keys` are modeled, rendered, and projected to the constraint
   writers ([feature 17](features/17-class-validation-constructs.md) slices
   1, 2, 4 ✅): `rules` become Postgres conditional `CHECK`s and SHACL
   conditional shapes, `unique_keys` become Postgres `UNIQUE` constraints;
   a format that projects neither warns of the gap, and a `unique_keys`
   slot the class lacks warns at generate time. Cross-instance `unique_keys`
   in SHACL (needs SPARQL) is still to come. Still not
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
   Remaining tail: `subproperty_of` (an external `rdfs:subPropertyOf`
   target URI; slot-level `is_a` covers the in-schema case).
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

## Spec-conformance findings (audited 2026-08-04)

Checked against the LinkML metamodel docs (tree_root, slots, inlining,
URIs-and-mappings), not against this file's own claims. Ranked by impact.

1. ~~**T-box fallback IRIs diverge from LinkML's rule.**~~ **Resolved
   2026-08-04:** the fallback now expands `{default_prefix}:{Name}` exactly
   as linkml-runtime does, unified across every site that names a class or
   slot; the fragment form survives only for schemas with no usable
   `default_prefix`. Original finding: When `class_uri` /
   `slot_uri` is absent, LinkML mints `{default_prefix}:{Name}` — e.g.
   `https://example.org/estate/Provider`. panschema mints
   `{schema.id}#{Name}` — `https://example.org/estate#Provider`
   (`class_iri_string` / `slot_iri_string` / `enum_iri_string`). Every
   element without an explicit URI therefore gets a *different IRI* than
   linkml-runtime or gen-owl would produce for the same schema, so
   cross-tool RDF joins fail silently. panschema is also internally split:
   **instance** ids already expand via `default_prefix` (conforming), so the
   A-box follows the spec while the T-box does not. Aligning is a breaking
   change to every consumer's emitted IRIs — the cheapest moment is before
   v0.3.0, while nothing published depends on the fragment form.

2. ~~**`identifier` means globally unique; the container-scoped construct is
   `key`.**~~ **Resolved 2026-08-04:** `key` is modeled, identifies records,
   and is what per-dataset scoping keys on; `identifier` recovered its
   global meaning — an identifier-carrying record mints unscoped in the
   schema namespace, the same individual wherever it appears. Original
   finding: The metamodel: an identifier value cannot recur *anywhere*; a
   `key` value must be unique only within its container. Per-dataset scoping
   deliberately gives `identifier` key-like semantics (unique per dataset,
   scope from the root's IRI) because consumers' data reuses generic ids
   across datasets. Under strict LinkML that data is non-conforming and the
   idiomatic modeling is `key` — which panschema does not model. Recorded as
   a **deliberate deviation**; modeling `key` would let spec-conscious
   authors express the same thing idiomatically, with `identifier` keeping
   its global meaning.

3. ~~**More than one `tree_root` deviates from a metamodel "should".**~~
   **Addressed 2026-08-04:** loading a multi-root schema now warns once,
   naming the roots and the metamodel's recommendation; deliberately not an
   error and not promoted by `--strict`. The interop caveat stands.
   Original finding: The
   spec: "each schema should have at most one tree root." Per-dataset root
   selection intentionally supports several, because a separate reference
   schema would duplicate or import the model's hub classes. Advisory, not
   normative — but upstream LinkML tooling may warn on or mishandle a
   two-root schema, so a consumer round-tripping through linkml-runtime
   should expect friction there.

4. ~~**Inlining is inferred from data shape; the spec's flags are not
   modeled.**~~ **Half-addressed 2026-08-04:** `inlined`/`inlined_as_list`
   are modeled (tri-state, as declared), and the SimpleDict form is read —
   a scalar dict entry expands into the class's one non-identifying slot,
   closing the silent-data-loss hole. **Still open, deliberately:**
   enforcing the flags in `validate` (data that inlines without declaring
   `inlined: true` would newly flag), and reading remains shape-inferred.
   Original finding: LinkML: a class-ranged slot whose range has no identifier is
   *always* inlined; with one, it defaults to a reference unless `inlined:
   true`; `inlined_as_list` selects list vs identifier-keyed dict; and a
   one-extra-slot class may serialize as a **SimpleDict**
   (`{key: primary_value}`). panschema reads list and keyed-dict forms by
   shape, does not consult `inlined`/`inlined_as_list`, and does not read
   the SimpleDict form at all — a conforming LinkML file using it would
   lose those records. Reading permissively is defensible; not reading
   SimpleDict is a gap.

Confirmed conforming, for the record: declared `class_uri`/`slot_uri` are
honored and CURIE-expanded; bare instance ids resolve via `default_prefix`
and CURIE ids via declared prefixes, exactly the documented identifier
behavior; and the guidance that identifiers be `uriorcurie`-shaped is what
the shared-dataset CURIE convention leans on.

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
