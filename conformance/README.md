# Conformance oracle

A uv-managed Python environment pinning `linkml-runtime` and the `linkml`
validator, for settling questions about LinkML metamodel semantics
against the reference implementation instead of arguing from the docs.

This is a development tool, not part of the test suite: `cargo nextest`
never touches it, and the crates' gates need no Python. Run a probe when a
semantics question comes up, record the verdict in the code or
`docs/linkml-coverage.md`, and move on.

## Snapshots

`snapshots/` holds what each probe prints at the LinkML pinned in
`uv.lock`, so a LinkML that answers a question differently shows up as a
diff instead of waiting for someone to rerun the probe:

```bash
uv run --project conformance conformance/snapshot.py          # record every probe's output
uv run --project conformance conformance/snapshot.py --check  # diff the probes against the record, exit 1 if any differ
```

Re-record in the same commit as a new or changed probe, and when the pin
moves to a LinkML that changes a verdict. A probe must print the same
thing on every run: one whose generator varies its output's order sorts
it before printing.

`.github/workflows/upstream-linkml-check.yml` runs `--check` on every PR,
and every push to `main`, that touches `conformance/`. The check is
required on `main`, so a Dependabot bump of the pin that changes a verdict
waits for review instead of auto-merging. Weekly, it also runs
it against the latest LinkML release and against LinkML's `main`, keeps one
tracking issue for each (label `upstream-linkml`) with the diff, and closes
it when the probes match again. It never moves the pin or a snapshot;
Dependabot's `conformance` group moves the pin. A differing probe is run
once more before it is reported, since several fetch from `w3id.org`, and
`--check` exits 2, not 1, when it cannot run at all.

## Usage

```bash
uv run --project conformance conformance/probe_annotations.py
uv run --project conformance conformance/probe_inlining.py
```

The inlining probe crosses every slot declaration (`inlined`,
`inlined_as_list`, neither; identifier, key-only and no-identifier
ranges) with every authored form and prints the validator's verdict on
each; it is the evidence behind `verify`'s inlining check and the
shape the JSON Schema and OpenAPI writers give each inlining.

```bash
uv run --project conformance conformance/probe_key_scoping.py
```

The key-scoping probe spells a per-class `identifier`/`key` split on one
shared `id` slot several ways and reports which ones LinkML's loader,
generators, and validator accept; it is the evidence behind the load
refusal of a slot that is both. `gen-rdf` fetches from `w3id.org`, so run
it with network access.

```bash
uv run --project conformance conformance/probe_mixin_merge.py
```

The mixin-merge probe gives a class one slot from several ancestors
(an `is_a` parent, mixins, their own parents, a redeclaring attribute)
and prints the fields `induced_slot` settles on beside the
`class_ancestors` order; it is the evidence behind the resolver's
precedence, which takes each field from the first ancestor in that order
that sets it.

```bash
uv run --project conformance conformance/probe_mixin_flag.py
```

The mixin-flag probe runs `gen-sqlddl`, `gen-shacl`, `gen-json-schema`,
`gen-pydantic`, and `gen-owl` over an abstract class, a `mixin: true`
class, and a concrete class using both, prints what each generator emits
for them, and asks `linkml-validate` whether it accepts a record typed as
each. Every generator gives the abstract class and the mixin a table, a
shape, a definition, and a model, and the validator accepts both records:
LinkML enforces neither flag, and both say the class holds no records of
its own. It is the evidence behind the Postgres writer emitting no table
for either while the SHACL and JSON Schema writers keep the shape and the
definition. `gen-owl` fetches from `w3id.org`, so run it with network
access.

```bash
uv run --project conformance conformance/probe_permissible_value_iri.py
```

The element-IRI probe puts a class, slots, and enums named outside
LinkML's usual casing, with enum values that contain a space, a slash, or
a non-ASCII letter, through `gen-owl` under a `/` and a `#` default
prefix, and prints the IRI each element gets; it is the evidence behind
the casing of a default element IRI and the percent-encoded
`{enum IRI}#{value}` form of a permissible value's IRI.

```bash
uv run --project conformance conformance/probe_merge_imports.py
```

The merge-imports probe gives a main schema an import, and the import two
imports of its own, colliding on purpose, and compares the unmerged schema
(what `gen-owl` projects) with `SchemaView.merge_imports` and with
`gen-linkml --mergeimports` reloaded standalone: which definition and prefix
binding win, what provenance survives, whether `linkml:types` stays an
import, and the IRI and range an imported element ends up with. It is the
evidence behind the loader minting an imported element under its own file's
default prefix and typing a rangeless imported slot from the root's
`default_range`; the merged files `gen-linkml` writes move every imported
element into the root's namespace, which is why they are not the reference
for meaning.

```bash
uv run --project conformance conformance/probe_identifier_columns.py
```

The identifier-columns probe runs `gen-sqlddl`, `gen-json-schema`, and
`gen-pydantic` over a schema in which every class has an identifier and a
root holds some parts through an `inlined_as_list` slot. `gen-sqlddl`
gives each such part a column naming its holder (`Part."Holder_id"`, a
foreign key) and keeps a linking table for a multivalued slot that is not
inlined. Every id is a `TEXT` column, whether its range is `string` with a
pattern, `uri`, `uriorcurie`, or a type derived from `uriorcurie`, and no
generator acts on `implicit_prefix`. `gen-json-schema` puts `format: uri`
on a `uri`-ranged id, and on a reference to a class it identifies, and
carries an id's `pattern` (not onto a reference); `gen-sqlddl`
emits no `CHECK` for one. It is the evidence behind the Postgres writer
keeping an IRI id a `text` column rather than `uuid`. The pattern `CHECK`
on that column is panschema's own choice, as on any patterned column.

```bash
uv run --project conformance conformance/probe_anonymous_records.py
```

The anonymous-records probe holds records of a class with no `identifier`
or `key` in a list, as a single value, nested in another such record, and
twice under one `name`, and prints `linkml-validate`'s verdict and the
Turtle `linkml-convert` writes. Each record is a blank node typed by its
class, with no `rdfs:label`; two sharing a `name` are valid and stay two
nodes. A bare string or a dict spelling where such a record belongs is
rejected, and the validator's paths count entries from 0
(`/parcels/0/stamps/0`). It is the evidence behind a record with no
identifier having no IRI in the instance model, projecting as a blank node,
and being named in a finding by its place in the data, and behind `verify`
reporting a key on a dict of such records.

```bash
uv run --project conformance conformance/probe_metamodel_extension.py
```

The metamodel-extension probe spells one count-bound declaration on a
slot every way LinkML offers for a tool's own meaning: an annotation
(bare, under a declared prefix, under an undeclared one), an extension,
`instantiates:` a metaclass with the fields as annotation tags or as one
structured value (the metaclass imported, not imported, or undefined),
and a bare metaslot key. It gives each a typo'd or missing field,
declares each on the slot and in a class's `slot_usage`, and reports
each tool's output as its difference from a control schema with no
declaration. A bare metaslot key is refused by the loader,
`linkml-lint`, `linkml-validate`, and every generator. Every other
spelling loads, and `induced_slot` hands it to a subclass from
`slot_usage`. Nothing checks a value: the loader, `linkml-lint`, and
`linkml-validate` against the metamodel accept a typo'd field, a
missing required field, and a metaclass that is not imported or does
not exist, and the `InstantiatesValidationPlugin` finds nothing (it is
off by default, and its source reads `instantiates` on classes only).
`gen-json-schema` emits nothing for any spelling, and `gen-owl` and
`gen-shacl --include-annotations` emit nothing for an extension. An
annotation reaches `gen-owl` as a property in the consumer schema's
namespace when its tag is bare, under its own prefix when that prefix
is declared, and not at all when it is not. `gen-shacl` puts a bare tag
in predicate position as a literal, which RDF does not allow, and an
undeclared prefix as an IRI whose scheme is the prefix. Both write a
structured value as the Python repr of the object. A value does fail
`linkml-validate` when it is validated as data against the metaclass.
It is the evidence behind the spelling of schema-declared slot
semantics. `gen-owl` and `gen-shacl` fetch from `w3id.org`, so run it
with network access.

Run it against LinkML's `main` (at 2026-11-25, `eff06809d4`) to see
what the next release changes:

```bash
uv run --no-project --python 3.12 \
  --with "linkml-runtime @ git+https://github.com/linkml/linkml@main#subdirectory=packages/linkml_runtime" \
  --with "linkml @ git+https://github.com/linkml/linkml@main#subdirectory=packages/linkml" \
  python conformance/probe_metamodel_extension.py
```

At `eff06809d4` every verdict matches 1.12.1 but one: in the flat
`instantiates:` form with the metaclass imported, `gen-owl` and
`gen-shacl` write each field under the IRI of the metaclass slot that
declares it (`pm:counted_slot`), from linkml/linkml#4157. Nothing checks
a value there either.

Load an arbitrary schema through the reference implementation:

```bash
uv run --project conformance python -c \
  "from linkml_runtime.utils.schemaview import SchemaView; SchemaView('path/to/schema.yaml')"
```

Add a probe script per topic as questions come up; keep each one small
and self-explaining, with the panschema behavior it checks named in the
docstring.
