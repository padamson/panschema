# Feature 33: LinkML instance reader + first-class instance model

**Feature:** Read LinkML **instance data** (an A-box conforming to a schema)
into a first-class `InstanceSet` — a flat, id-keyed collection of typed
records with typed references — and render it as the instance graph, the same
way OWL individuals do today. `generate --input schema.yaml --instances
data.yaml` ingests a `tree_root`-container LinkML data file; the `InstanceSet`
becomes the hub every instance consumer (the instance graph, RDF, and later
`validate --data`) goes through.

**User Story:** As someone building a graphRAG application over a LinkML
ontology, I want to author (or have an LLM agent construct) a worked-example
A-box as a LinkML data file, and have panschema read it, visualize it, and
validate it — staying entirely in LinkML + JSON, with no OWL/TTL detour.

**Related ADR:** [003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md).
Builds on the instance-graph exporter ([feature 18](18-exemplar-individuals-in-graph.md))
and the JSON-Schema writer ([feature 32](32-json-schema-writer.md)); pairs with
`panschema validate --data` (the next stream).

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Design decisions

### Decouple the on-disk format from the internal model

Like schema *format* (TTL/YAML) is decoupled from the schema *IR*, instance
*format* is decoupled from the instance *model*:

- **On-disk format:** the canonical LinkML **`tree_root` container** — a single
  object conforming to a `tree_root` class whose slots are typed collections;
  references between instances use LinkML's `inlined: false` identifier
  semantics. This is what `linkml-convert` and the LinkML runtime read/write,
  so a data file round-trips through the ecosystem. It is *not* a bespoke
  `@type`-tagged shape.
- **Internal model:** a first-class, flat, **id-keyed `InstanceSet`** of typed
  records, each carrying its class, slot values, and typed references. This is
  the hub the instance graph, RDF, and validation all consume.

### Why this is the robust foundation for LLM-agentic graphRAG construction

An LLM agent does not emit a whole container in one shot. It emits **one typed
record at a time** — e.g. `rig`'s `Extractor<Wine>` producing a `Wine`
constrained by Wine's JSON Schema ([feature 32](32-json-schema-writer.md)) —
and the orchestration knows the class because it asked for it. The flat
`InstanceSet`:

- **accumulates records incrementally**, deduped by identifier (a nested
  container can't be built this way cleanly);
- supports **reference integrity for self-correction** — a `from_region:
  beaujolais` with no `beaujolais` Region is a *dangling instance reference*
  (the A-box analog of the loader's dangling-schema-ref check), the feedback
  signal an agent loop uses to fix itself;
- is validated **per record** against its class (`validate --data`);
- serializes to many outputs from one model: the **instance graph** (viz),
  **RDF** (into oxigraph for retrieval), and a **`tree_root` container**
  (idiomatic persistence / round-trip).

`InstanceSet` is to instance data what the LinkML IR is to schemas: the pivot
every reader/writer/validator goes through.

---

## Vertical Slices

### Slice 1: Model `tree_root` in the IR

**Status:** Complete

**Priority:** Must Have

**User Value:** The schema IR records which class is the data container, so the
instance reader has an entry point — and the JSON-Schema writer can point its
document root at it (completing the deferred root-`$ref`).

**Acceptance Criteria:**
- [x] `ClassDefinition` gains a `tree_root` boolean; the LinkML/YAML reader parses `tree_root: true` (via the same serde the schema read uses), and it round-trips through the IR (default `false`, skips serialization when absent).
- [x] The JSON-Schema writer's document root `$ref`s the `tree_root` class when one exists (the branch deferred in feature 32), verified by the `jsonschema` oracle.
- [x] Test: `class_parses_tree_root_flag` (parse) and `document_roots_at_the_tree_root_class` (JSON-Schema root `$ref`, oracle-checked).

### Slice 2: First-class `InstanceSet`; move the OWL path + exporter onto it

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slice 1.

**User Value:** One instance model, behavior-preserving — the OWL individual
path and the instance-graph exporter stop reading `panschema:individual*`
annotations and read the `InstanceSet` instead.

**Acceptance Criteria:**
- [ ] An `InstanceSet` type: flat records keyed by identifier, each with class, literal slot values, and typed references (id → id, labelled by property).
- [ ] The OWL reader produces an `InstanceSet` (replacing / alongside the annotation flattening), and `schema_to_instance_graph` consumes an `InstanceSet` — the rendered instance graph is unchanged (the feature-18 e2e still passes).
- [ ] Test: the OWL fixture yields the same instance graph via the `InstanceSet` path.

### Slice 3: LinkML instance reader (`--instances`)

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slices 1–2.

**User Value:** A LinkML data file renders as the instance graph — the demo
stays LinkML + JSON, and this is the dogfood for the Step-7 examples.

**Acceptance Criteria:**
- [ ] `generate --input schema.yaml --instances data.yaml` reads a `tree_root`-container LinkML data file into an `InstanceSet`: each typed collection slot's items become records of that slot's range class; identifiers resolve (map key or `identifier` slot); a `inlined: false` class-ranged value becomes a typed reference (edge), an inlined object a nested record, a literal node metadata.
- [ ] The instance graph renders from the LinkML `InstanceSet` (no OWL needed); e2e paints it (a checked-in wine Step-7 example fixture — self-contained).
- [ ] Handles both inlined-as-dict and inlined-as-list collections.

### Slice 4: Instance reference-integrity diagnostic

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 3.

**User Value:** A reference to an instance that doesn't exist is reported (the
agent-self-correction signal), not silently dropped.

**Acceptance Criteria:**
- [ ] A typed reference whose target identifier isn't in the `InstanceSet` produces a diagnostic naming the referring record, the property, and the missing id — routed through the same diagnostics path as dangling schema refs; `--strict` fails on it.
- [ ] Test: a data file with a dangling instance reference warns (and fails under `--strict`).

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: model `tree_root` | Must Have | — | Complete |
| Slice 2: `InstanceSet` + move OWL/exporter onto it | Must Have | Slice 1 | Not Started |
| Slice 3: LinkML instance reader | Must Have | Slices 1–2 | Not Started |
| Slice 4: instance reference integrity | Should Have | Slice 3 | Not Started |

## Definition of Done

- [ ] Slices 1–3 met (slice 4 recommended); `validate --data` builds on the `InstanceSet` in its own stream.
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`.
- [ ] Rendered instance graph e2e-verified from a LinkML data file (not just OWL).
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) notes `tree_root` + LinkML instance ingestion.
