# Why panschema?

## The problem: data modeling is fragmented

One data model gets written down in several modeling languages, each with
its own ecosystem:

| Modeling language | What it is | Tools |
|-------------------|------------|-------|
| **OWL/RDF** | An ontology language: open-world, formal semantics | Protégé, Widoco, LODE |
| **LinkML** | A schema language for FAIR data | the `gen-*` generators |
| **JSON Schema** | A contract for APIs, configuration, and structured output | many generators |
| **SHACL** | A constraint (shapes) language for RDF data | SHACL validators |
| **SQL DDL** | The physical schema of a database | one per database |

Each has its own documentation tools, converters, and checkers. A team that
works across them learns several toolchains and keeps several copies of one
model in step by hand, and the copies drift.

## The vision: pandoc for data modeling

**pandoc** made document conversion a solved problem: one tool that reads
every document format and writes every other.

**panschema** does the same for data modeling. Every input is read into one
internal representation (the LinkML model), and every output is written
from it:

```
┌─────────────┐     ┌─────────────┐     ┌──────────────────────────────┐
│  OWL/Turtle │     │             │     │  HTML docs (schema + data)   │
│  LinkML     │ ──► │  LinkML IR  │ ──► │  Turtle, JSON-LD, RDF/XML,   │
│             │     │ (canonical) │     │    N-Triples, SHACL          │
│             │     │             │     │  JSON Schema, OpenAPI        │
│             │     │             │     │  Rust, Postgres DDL          │
└─────────────┘     └─────────────┘     └──────────────────────────────┘
    Readers              Core                      Writers
```

Any reader pairs with any writer, so adding one reader adds every output
for that language. A JSON Schema reader is on the roadmap.

### What it does today

1. **Convert.** OWL/Turtle or LinkML in; the RDF family, SHACL, JSON
   Schema, OpenAPI, Rust types, and Postgres DDL out, all from one source.
   A LinkML construct panschema parses but does not model is reported
   rather than dropped, and `generate --strict` fails the build on it.
   Which modeled constructs each output carries is a table,
   [linkml-coverage.md](docs/linkml-coverage.md).

2. **Document.** Responsive HTML with a force-directed graph of the schema
   and, when instance data is supplied, a second graph of the data, with
   cards for every element. `publish` builds the docs for each version its
   `panschema-publish.toml` lists, read from git history, with a version
   switcher.

3. **Verify.** `verify --schema schema.yaml --data data.yaml` checks a
   LinkML instance-data file against its schema and exits non-zero on any
   violation (what other tools call *validation*); bare `verify` checks
   everything a `panschema.toml` manifest declares. Every finding is a
   record with a `kind`, not a sentence to parse.

4. **Migrate.** `migrate` writes the schema's Postgres DDL as a versioned
   migration file for a checksumming runner to apply. A semantic `diff`
   between two schema versions, with a compatibility verdict, is next.

Schemas are also packages: a manifest names its dependencies (`path:` or
`github:` sources), `fetch` resolves and locks them, and `publish` ships a
schema's docs and datasets so a downstream manifest can name them.

## What panschema calls things

The words are chosen once, so the docs, the rendered pages, and the code
agree. A **schema** is the thing panschema documents, whatever language it
was read from; an **ontology** is a schema written in OWL. The data that
conforms to a schema is **instance data**, held in a **dataset** (one data
file); each member of a dataset is an **instance** of its class, written to
RDF as an OWL **individual**. A schema's relations and attributes are
**slots**, LinkML's word. The schema is the T-box and the instance data the
A-box, in the description-logic sense, though `verify` checks a closed
world: a missing required slot is a violation. The full list, and why the
identifiers keep "individual", is in
[ADR-014](docs/adr/014-instance-terminology.md).

## Why Rust?

The current ontology tools need a JVM (Widoco, LODE, Protégé) or a Python
environment (the LinkML generators), start slowly, and make for heavy CI
containers.

panschema is one static binary with no runtime dependencies. It starts in
milliseconds, so documentation builds in the time a Python interpreter
takes to import, and it drops into a GitHub Actions job as a single
download.

## The goal

Working with any modeling language should be as easy as working with
Markdown:

```bash
# Document an ontology
panschema generate --schema ontology.ttl --output docs/

# Convert a LinkML schema to JSON Schema
panschema generate --schema schema.yaml --format json-schema --output schema.json

# Check instance data against its schema
panschema verify --schema schema.yaml --data data.yaml

# Write the schema's Postgres DDL as a migration file
panschema migrate --schema schema.yaml --migrations db/migrations/

# Build the versioned docs site for every released version
panschema publish
```

> "If it's not documented, it doesn't exist. If documentation is hard, it won't happen."

panschema makes it easy.
