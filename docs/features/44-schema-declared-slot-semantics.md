# Feature 44: Schema-Declared Slot Semantics

**Feature:** Read three slot-level meanings from the schema that declares
the slot — absence claims (`asserts_absence`), anchor-IRI expansion
(`expand_against`), and version pins (`records_version_of`) — instead of
from each consumer's manifest, and bind
cross-graph checks to the dataset a record itself names.

**User Story:** As the author of a contract schema whose slots carry
domain semantics (this slot's values are joint-absence claims; this
slot's values are anchors into a target namespace), I want to state
those semantics once, on the slot, in my own schema — so every consumer
checks the same meaning, no consumer can silently restate it more
weakly, and a reader of the schema sees what a slot means without
opening any consumer's configuration.

**Tracking:** panschema#128 (with panschema#103 resolved by Slice 4).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Design

Both semantics ride on LinkML slot annotations, which since panschema
supports LinkML 1.6+ structured values (#127) can carry a structured
value under `value:`. The tool already interprets schema-declared
meaning for `identifier`, `key`, `tree_root`, and `rules`; these three
join that family.

**Absence claims.** A slot annotated

```yaml
unconnected_anchors:
  annotations:
    asserts_absence:
      value:
        via_slot: connecting_class
```

declares: each record's values at this slot are anchors the record
claims no single sibling record joins (two or more anchors claimed
unjoined; a single anchor claimed unreferenced — the joint-vs-single
reading stays derived from the anchor count, as today). `via_slot`
optionally narrows the claim to records of the class a sibling slot
names. Declaration scope follows LinkML: a top-level `slots:`
declaration binds every class carrying the slot; a class's own
declaration (an attribute, or `slot_usage` — which overrides, as
`slot_usage` does everywhere) binds that class's records and wins over
the schema-wide one. The manifest keeps only enablement:
`resolve_against` says which sibling entries' datasets claims are
verified against, exactly as it already scopes reference resolution.
The `verify_absences` manifest key (unreleased) is removed; several
slots may carry the annotation, and each is checked.

Note this deliberately drops the issue example's `joint_referent: true`
field: presence of the annotation is the assertion, and the joint
reading is anchor-count-derived, so the field would be a knob with no
"off" position. An unrecognized field inside the annotation is a load
warning, not silently ignored.

**Anchor expansion.** A slot annotated

```yaml
expected_anchors:
  annotations:
    expand_against: target_schema
```

declares: a scheme-less value at this slot expands against the value of
the named slot on the same record (the base is already in the data).
Values with a scheme — absolute IRIs, CURIEs against declared prefixes —
are untouched and stay legal, so third-party graphs whose records live
outside the target namespace remain expressible. Expansion applies only
where the slot's range class is not inlinable anywhere in the schema, so
a bare value can never be confused with an in-dataset reference;
elsewhere the annotation is a load warning and no expansion happens.

**Version pinning.** A slot annotated

```yaml
target_dataset_version:
  annotations:
    records_version_of:
      value:
        sibling_slot: target_dataset
```

declares: the record's value at this slot is the package version the
record was written against, and `sibling_slot` names the slot on the
same record whose value says which sibling. That value binds to a
`resolve_against` sibling by its `[schemas]` entry key, by the name its
publish manifest declares, by its schema's `id` IRI, or by a dataset
name that manifest lists — so a benchmark naming a target *schema* by
IRI or a target *dataset* by name pins the package that publishes it. At check time the declared version is compared with the
version the sibling actually resolved to; a mismatch warns per record
and fails under `--strict`. A pin that cannot be evaluated — no value
or several, a non-string version, a `sibling_slot` value naming no
sibling — is reported uncheckable, never as agreeing. Two versions name the same
release when both parse as semver (a leading `v` allowed) and compare
equal by precedence: build metadata does not separate them, a
pre-release does, and an unparsable side is uncheckable. Without the
pin, a benchmark written against one release of its target reads green
when checked against any later one, so nothing distinguishes a
benchmark checked against the release it was written for from one
checked against whatever happens to be installed.

**Dataset binding.** Where the referring record itself names which
target dataset its claims are about, checks bind to that dataset rather
than to every dataset the target entry declares (today's behavior, safe
only while every checked dataset happens to be a subset of the declared
one). The slot that names the dataset is itself schema-declared (see
Open Questions).

**Projection.** Schema-declared semantics that only panschema can read
are a quieter form of the coupling this feature removes, so the emitted
RDF must carry them (see Open Questions for the vocabulary).

---

## Slices

### Slice 1: absence claims read from the schema — Complete

**Acceptance criteria:**

- [x] A schema slot annotated `asserts_absence` (value-wrapped mapping,
      optional `via_slot`) has its records' claims verified during bare
      `verify` and `generate --strict` whenever the entry's
      `[check.<name>]` names `resolve_against` siblings — with the same
      verdicts, warnings, and summary counts the manifest binding
      produced.
- [x] Several annotated slots are all checked.
- [x] A manifest still carrying `verify_absences` fails to parse (the
      key is unreleased; the error need not be bespoke).
- [x] `asserts_absence` whose `via_slot` names no slot of any class is
      a load warning, and the claims it would narrow are reported
      uncheckable rather than silently widened.
- [x] An unrecognized field inside the annotation's value is a load
      warning naming the field.
- [x] A schema with no annotated slot and `resolve_against` set checks
      references exactly as today, with no absence pass and no new
      output.
- [x] A class-scoped declaration (attribute or `slot_usage`) governs
      only that class's records; another class's same-named slot holds
      ordinary data.
- [x] A `slot_usage` declaration overrides the top-level one for its
      class, the direction `slot_usage` overrides everywhere in LinkML.
- [x] A declaration whose `via_slot` is not a string binds with its
      claims uncheckable — never evaluated wide of what the author
      wrote.
- [x] Declaration defects fail `--strict`, on both the generate and
      validate paths.
- [x] A schema that declares claims while its `[check.<name>]` names no
      `resolve_against` siblings (or has no check table) gets a note
      that nothing verifies them.

### Slice 2: anchor-IRI expansion — Complete

**Acceptance criteria:**

- [x] A scheme-less value at a slot annotated `expand_against: <slot>`
      reads, everywhere panschema reads it (cross-graph resolution,
      absence claims, emitted RDF, rendered pages), as the value of the
      named slot on the same record concatenated with the bare value.
- [x] Absolute IRIs and CURIEs against declared prefixes at the same
      slot are unchanged.
- [x] A record with no base of its own resolves it from the nearest
      containing record that carries one — a benchmark states its
      target once, above its questions — with the record's own value
      winning when both exist. Only slots the containing record's class
      carries as scalars can supply a base (a stray same-named field
      cannot), and a dict-keyed record's identifier key counts as its
      authored value here as everywhere.
- [x] A record whose nearest carried base is unusable warns naming the
      containing record that supplied it; a chain that supplies no
      usable base at all leaves the bare values unexpanded and warns
      that no containing record carries one. A base the containment
      structure can never supply — uncarried along any containment path
      to the governed class, only class-ranged there, or itself
      declared `expand_against` — is refused at load instead.
- [x] A class-ranged slot's declaration binds exactly when no local
      record of its range class can exist — every site ranging the class
      is itself declared external; while any is not (a `tree_root`
      collection, or another slot without the annotation), the
      declaration is a load warning naming that site, and values are
      read as before. An uncarried base slot or a non-string value warns
      likewise; defects fail `--strict` like the absence declarations'.
- [x] At a bound class-ranged slot, a bare scheme-less value reads as an
      external reference into the declared base's namespace; an inline
      record authored there is a validation finding, not silence.
- [x] The wine-shaped benchmark authored with bare anchors resolves and
      verifies identically to the same benchmark authored with absolute
      IRIs.

### Slice 3: checks bind to the record's declared target dataset

**Acceptance criteria:**

- [ ] When the referring record names a target dataset, its references
      and absence claims are checked against only that dataset of the
      target entry; a record citing an IRI that only a *different*
      dataset mints is unresolved.
- [ ] A named dataset the target entry does not declare is reported per
      record, and the record's claims are uncheckable, not silently
      checked against everything.
- [ ] Records naming no dataset keep today's all-datasets behavior.

### Slice 4: sibling scalar citations count as references (#103)

**Acceptance criteria:**

- [ ] A sibling record citing an anchor's IRI through a `uri`-ranged
      scalar slot contradicts a single-anchor claim, and a sibling
      citing every anchor of a joint claim through scalars contradicts
      it, symmetrically with how the claim side already reads both
      authorings.
- [ ] The absence-check documentation states the widened definition.

### Slice 5: schema-declared semantics project to RDF

**Acceptance criteria:**

- [ ] The Turtle emitted for a schema carrying `asserts_absence` or
      `expand_against` states them on the slot's IRI, so a consumer
      reading only the published graph sees the declaration.
- [ ] Round trip: the OWL reader reads the projection back into the
      same annotations.

### Slice 6: version pins are checked against the sibling — Complete

**Acceptance criteria:**

- [x] A record whose annotated version slot matches the version the
      named sibling resolved to counts as an agreeing pin, and the
      check's summary says how many pins agree.
- [x] A record whose declared version differs from the sibling's warns
      naming the record, the slot, both versions, and the sibling; the
      manifest-wide check fails under `--strict`.
- [x] A pin that cannot be evaluated is reported uncheckable with the
      reason, and counts against `--strict` like an uncheckable
      absence claim.
- [x] The sibling binds by entry key, published name, schema IRI, or a
      declared dataset name; a value matching none — or, through the
      aliases, more than one — is uncheckable, not silently compared
      against the first sibling.
- [x] Declaration defects (a value that is not a mapping with a string
      `sibling_slot`, an unknown field, a slot or `sibling_slot` no
      class carries, a slot naming itself) are load warnings that
      `--strict` refuses, like the other two families.
- [x] The shipped skill documents the annotation's value shape and
      the verdicts a pin can receive (a test holds the skill to it).

**Notes:**
- Same `ScopedBindings` walk and the same manifest enablement
  (`resolve_against`) as the other two families; the only new input is
  the sibling's resolved version, which the check reads from the
  sibling's publish manifest through the same resolution `fetch` uses.
- A version authored as a bare YAML float (`1.0`) loads as a number, not
  a string, and is reported uncheckable with a quoting hint rather than
  compared through a lossy conversion.
- The skill documents it in the manifest reference beside the other
  two families; the three families share one declaration walk and one
  registration table, so load diagnostics, the strict gate, and the
  check's enablement notes cannot disagree about which families exist.
- A record naming a sibling binds by exact entry key first; the
  published name, the schema IRI, and dataset names are aliases, and a
  name that matches two siblings through aliases is uncheckable rather
  than first-match.

---

## Open Questions

1. **Slice 3 mechanism.** How does panschema know which slot names the
   target dataset? Proposal: a third annotation in the same family
   (e.g. `names_dataset_of: <entry-agnostic marker>`), read like the
   other two. Needs a decision before Slice 3; the contract schema's
   `target_dataset` description already defines the value's referent (a
   published dataset name of the target package, per that package's
   publish manifest).
2. **Slice 5 vocabulary.** LinkML's RDF mapping has no standard
   per-annotation predicate. Proposal: mint predicates under the
   schema's own namespace (`<ns>asserts_absence`, structured value as a
   blank node with `via_slot`), documented in the coverage table. The
   alternative — no projection — re-creates the coupling this feature
   exists to remove, and is rejected for that reason.
3. **Scalar-citation scope (Slice 4).** The recommendation is to count
   scalar citations (symmetry); the alternative reading (edge-only,
   annotations-are-not-joins) loses to the observation that the claim
   side already reads scalars as anchors.

## Things to watch

- A Turtle-authored contract schema cannot express these annotations
  until Slice 5's projection round-trips them through the OWL reader —
  and the manifest fallback is gone — so absence checking is
  LinkML-YAML-only until then.
- Class-scoped declarations do not yet propagate to subclasses: the
  resolved slot view deliberately drops `slot_usage` annotations, so
  inheritance here would drift from it. Revisit alongside resolution
  support.
- Class-ranged expansion implements the issue's not-inlinable rule as
  *local declarability*: refused while any site could still hold a local
  record of the range class or its `is_a`/mixin family — an unannotated
  ranging site (ancestors count: a designator can type a record into the
  class there; so do descendants, whose records are instances of it), or
  the class's family holding a `tree_root` (its records are document
  roots). Only declarations that themselves bind vouch for their sites,
  iterated to a fixpoint, so a refused declaration cannot prop up a
  sibling. Judged from the resolved induced view, so `slot_usage`
  inheritance and `any_of` replacement are seen.
- A base that does not form an absolute IRI gaps a class-ranged
  expansion rather than minting a phantom in-dataset name; inline
  records at a bound slot are one validation finding per claiming
  record and slot, whatever the authoring spelling.
- The static base check mirrors the runtime walk: the containment
  graph is read from the same resolved class-ranged sites (with `is_a`
  family widening) the loader uses, so a base the walk could never
  reach refuses at load rather than gapping record by record. Chained
  bases refuse because frames carry raw authored values — a governed
  base's own expansion would otherwise read differently by path.
- A parent-declared class-scoped annotation is blocked by its
  subclasses' sites (governance does not propagate to subclasses yet) —
  conservative by intent: it fails closed.
- "Scheme-less" is spelled `contains(':')`: any colon-bearing value —
  an undeclared-prefix CURIE included — stays as authored, and a typo'd
  prefix surfaces through the resolution checks rather than expanding
  into a wrong IRI.
- A vessel root (no authored identifier) shows its metadata expanded
  when the base is usable, but its scalar metadata reports no gap —
  dataset description, not record data. Class-ranged root slots expand
  with full parity (references, gaps, inline findings) whether or not
  the root is identified.

- The annotation spellings are the value-wrapped form #127 requires for
  structured values; `expand_against` is a plain string annotation.
- The manifest `verify_absences` removal has the same no-deprecation
  precedent as the `[check]` key move: both shipped and die unreleased.
- Expansion materializes at dataset load, so every downstream consumer
  sees one form; watch the HTML instance pages for double-rendering.
