# Output formats

Every format below is accepted by `generate --format <name>` (matching is
case-insensitive) and has a corresponding `[generate.<name>]` manifest key —
though **the manifest key is not always the same string**; see the manifest
reference.

| `--format` | Output | Notes |
|---|---|---|
| `html` | **directory** | Docs site + graph viz. The only format taking several `--instances`, and the only one honouring `--no-graph` / `--viz-mode` |
| `ttl` | file | OWL/Turtle. Accepts one `--instances`, folding the A-box into the same graph |
| `jsonld` | file | Accepts one `--instances` |
| `rdfxml` | file | Accepts one `--instances` |
| `ntriples` | file | Accepts one `--instances` |
| `graph-json` | file | Schema (T-box) graph wire format |
| `instance-graph-json` | file | A-box graph. Without `--instances`, falls back to the schema's embedded OWL individuals |
| `rust` | file | Structs, enums, and traits. A class other classes inherit from, or one declared `abstract` or `mixin: true`, is a trait; a slot ranging over it takes `<Name>Kind`, the closed enum of its concrete descendants, which holds no value until one exists. Generated code needs `serde`, plus a time crate for temporal ranges: `chrono` by default, or `jiff` with `features = ["serde"]` when the manifest sets `rust_time = "jiff"` |
| `postgres` | file | DDL. A class declared `abstract` or `mixin: true` gets no table; a mixin's columns land on each class listing it. Skips a class using `is_a` or a polymorphic `any_of`, and one whose slot ranges over a class with no table, with a warning per skip that `--strict` does not refuse, so read the output for a missing table |
| `shacl` | file | Shapes graph, separate artifact from the OWL output |
| `json-schema` | file | Draft 2020-12. Manifest key is `json_schema` |
| `openapi` | file | OpenAPI 3.1, `components/schemas` only — no `paths` |

Inputs: OWL/Turtle (`.ttl`, `.turtle`) and LinkML YAML (`.yaml`, `.yml`).
There is no JSON, JSON-LD or RDF/XML *reader*.

## Layout algorithms (`html_default_layout`)

Working: `force-directed`, `hierarchical`, `stress`, `kamada-kawai`, `sgd`.
Accepted but unimplemented, and greyed out in the picker: `circular`,
`radial-tree`. Unset, the viz picks `sgd`, or `hierarchical` for an
`is_a`-heavy schema.
