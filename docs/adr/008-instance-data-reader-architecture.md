# ADR-008: Instance-Data (A-box) Reader Architecture

## Status
Accepted

## Context

ADR-004 established a reader/writer architecture for **schemas** (the T-box):
any input format is converted to the `SchemaDefinition` IR (ADR-003) before any
consumer touches it, so writers and documentation are format-agnostic.

Instance data — the **A-box**, the records that populate a schema — grew a
parallel need without the same discipline. panschema now has two instance
sources feeding one model, `InstanceSet` (feature 33):

- `InstanceSet::from_owl_annotations` — a worked example authored as OWL
  `NamedIndividual`s.
- `InstanceSet::from_linkml_data` — a LinkML instance-data file (a `tree_root`
  container of records).

And a growing set of instance *consumers*: the HTML instance graph, the
`validate` command (feature 34), and later an RDF A-box emitter for retrieval.

The first cut of `validate` walked the raw LinkML-YAML tree directly. That
couples the validator to one on-disk format: an A-box in any other format
panschema understands (OWL individuals today; JSON later) could not be
validated through the same path. This is the A-box analog of the problem
ADR-004 solved for schemas, and it answers ADR-004's open question #3
("should readers validate input, or is that a separate concern?"): **validation
is a separate concern that operates on the instance model, not on any one input
format.**

## Decision

### Instance data flows through one model, like schemas flow through the IR

```
Instance-data file → instance Reader → InstanceSet (instance model) → consumers
   (LinkML data,                                                       (graph,
    OWL individuals,                                                    validator,
    future JSON)                                                        RDF A-box)
```

`InstanceSet` is to instance data what `SchemaDefinition` is to schemas: the
single hub every reader produces and every consumer goes through. A new A-box
format is a new reader that produces an `InstanceSet`; no consumer changes.

### The instance model preserves typed, slot-keyed values

For the model to serve *validation* — not just display — an `Instance` records
its authored slot values with enough fidelity to check constraints:

- **Slot-keyed, not label-keyed.** Values are keyed by slot *name* (what the
  schema constrains), so a validator can resolve each value's slot definition.
  The display graph's human labels are a separate projection.
- **Typed, not stringified.** A scalar retains its kind (`String` / `Integer` /
  `Float` / `Boolean`) so numeric-bound checks don't re-parse, and a reference
  is distinguished from a scalar so range-kind checks work.

Concretely, an `Instance` carries `slot_values: Vec<SlotValue>` — the complete
authored assignments (every field, including identifier and label slots) as
`InstanceValue`s (`Scalar(ScalarValue)` or `Reference(id)`). The display fields
(`label`, `literals`, `references`) remain as a human-facing projection built
alongside it.

### Validation consumes the model, not the format

The validator has two layers:

- `validate_instances(schema, &InstanceSet)` — the format-agnostic core. It
  checks each record's `slot_values` against its class's effective-slot
  constraints and the cross-record reference integrity. Any reader's
  `InstanceSet` validates through it.
- A thin per-format adapter (e.g. `validate_instance_data(schema, &yaml)` for a
  LinkML data file) handles reading and structural errors, builds the
  `InstanceSet`, then calls the core.

## Consequences

### Positive

- **Format-agnostic validation**: one validator serves every A-box format a
  reader exists for; adding a format doesn't touch the validator.
- **One instance hub**: graph, validator, and future RDF A-box all read the same
  `InstanceSet`, so they can't drift.
- **Right fidelity in one place**: typed, slot-keyed values live on the model,
  not re-derived per consumer.

### Negative

- **Two projections on `Instance`**: the typed `slot_values` and the display
  `literals`/`references` overlap; a LinkML record's values are recorded in both
  shapes. Unifying them (display derived purely from `slot_values`) is a future
  cleanup, deferred to avoid perturbing the graph wire format now.
- **Uneven reader coverage during rollout**: `from_linkml_data` populates
  `slot_values`; `from_owl_annotations` does not yet (OWL-individual *validation*
  isn't a wired use case), so an OWL A-box is display-complete but not
  validation-complete until that reader is upgraded.

## References

- [ADR-003: LinkML as Internal Representation](003-linkml-as-internal-representation.md)
- [ADR-004: Reader/Writer Architecture](004-reader-writer-architecture.md) (the T-box counterpart)
- [Feature 33: LinkML instance reader](../features/33-linkml-instance-reader.md)
- [Feature 34: `validate --data`](../features/34-validate-instance-data.md)
