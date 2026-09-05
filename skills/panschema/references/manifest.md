# `panschema.toml` — the consumer manifest

Drives `panschema generate` when run with **no `--schema`**. Discovered
cargo-style by walking up from the current directory.

> Every table is `deny_unknown_fields`. A misspelled key is a **hard parse
> error**, not a silently ignored line.

## The one thing that trips everyone

`[generate.<name>]` only runs if there is a **matching `[schemas.<name>>]`**.
The generate loop iterates the *schemas* table; a `[generate]` block with no
counterpart prints `no [generate.<name>] block; skipping` for the schemas it
does know, produces nothing, and **exits 0**. A silent no-op, not an error.

## A complete, working manifest

This exact example is extracted from this file and executed by panschema's
test suite, so it cannot rot.

```toml
[schemas.demo]
path = "."

[generate.demo]
ttl = "out/demo.ttl"
json_schema = "out/demo.schema.json"
```

Run `panschema generate` from the directory holding this file.

## `[schemas.<name>]` — where a schema comes from

Exactly one of two forms. **`path` and `source` are different fields.**

| Form | Keys | Meaning |
|---|---|---|
| Local package | `path = "./pkg"` | a directory containing `panschema-publish.toml`, resolved relative to the manifest |
| Remote package | `source = "github:owner/repo"` + `version = "1.2.3"` | `version` is required; matches the git tag modulo a leading `v` |

- `source` is the **remote** field and understands `github:` only. There is
  no `path:` protocol — `source = "path:."` fails with "unrecognized source
  protocol." Use the `path` field instead.
- **A repo can generate from its own package**: `path = "."` when
  `panschema-publish.toml` sits at the repo root. This is the normal way to
  emit non-HTML artifacts for your own schema.
- Caveat for that self-reference: `fetch` checksums the main file into
  `panschema.lock`, so `fetch --check` reports drift after every edit to a live
  schema. Keep `fetch`/`fetch --check` for real dependencies; `generate` never
  consults the lockfile.

## `[generate.<name>]` — what to emit

Every key is optional; an absent key means that writer does not run. Paths
are manifest-relative.

| Key | Output |
|---|---|
| `html` | **A directory** — the docs site, plus the viz assets |
| `instances` | Array of LinkML instance-data files (A-boxes). Declaration order drives the in-page selector |
| `html_graph_aspect` | `"W:H"`, default `16:8`. Only meaningful with `html` |
| `html_default_layout` | Layout name; see the formats reference |
| `html_page_layout` | `"schema-first"` (default) or `"instances-first"` — which half of the page leads |
| `html_schema_sections` | `false` omits the schema graph and class/slot/enum/type cards (metadata + namespaces stay); default `true` |
| `rust` | Rust structs/enums |
| `rust_time` | Time crate for generated temporal fields: `"chrono"` (default) or `"jiff"`. Wire format (RFC 3339 / ISO 8601 strings) is identical either way; pick the crate the consuming workspace already carries. Only meaningful beside `rust` |
| `postgres` | Postgres DDL — **the key is `postgres`, there is no `sql`** |
| `shacl` | SHACL shapes graph |
| `json_schema` | JSON Schema — **underscore**, though the CLI flag is `--format json-schema` |
| `openapi` | OpenAPI 3.1 `components/schemas` |
| `ttl` | OWL/Turtle |
| `jsonld` | JSON-LD |
| `rdfxml` | RDF/XML |
| `ntriples` | N-Triples |
| `graph-json` | Schema graph wire format — **hyphen** |
| `instance-graph-json` | A-box graph wire format — **hyphen** |
| `migrations` | **A directory** of versioned migration files. Written by `panschema migrate`, *not* by `generate` |

Note the naming is not uniform: `json_schema` and `html_graph_aspect` use
underscores, `graph-json` and `instance-graph-json` use hyphens. With
`deny_unknown_fields`, guessing wrong is a parse error.

`migrations` is the odd one out in a second way. Every other key names an
output regenerated from scratch on each run, so `generate` owns it. A
migration directory is append-only — a runner checksums each file it has
applied and aborts when the bytes change — so `generate` never writes there.
Run `panschema migrate` to add to it.

## `[check.<name>]` — what gets verified

Verification policy, deliberately separate from `[generate.<name>]`: checking
never requires declaring an output, and the policy survives deleting a
generate block. **Bare `panschema verify`** (no flags) reads the manifest
and runs everything here plus instance conformance for every declared
dataset, writing nothing — findings warn, `--strict` fails on them.
`generate --strict` also refuses to ship what these checks reject for the
entries it generates, and bare `verify --strict` additionally promotes
the schema-level diagnostics `generate --strict` refuses (untyped slots,
dangling schema references), so the check verb covers what the build verb
would reject. A `[check.<name>]` naming no `[schemas]` entry is a
configuration error in every manifest-driven command, not a silent no-op.

| Key | Meaning |
|---|---|
| `instances` | Datasets to check, **unioned** with the `[generate.<name>]` entry's list — this can add datasets but never hide the ones `generate` ships |
| `resolve_against` | An **array** of sibling entries whose datasets this entry's external references must resolve into (e.g. `resolve_against = ["catalog"]`). Only references landing in a namespace a listed sibling owns are checked — outside vocabularies stay unchecked, so one schema.org IRI can't fail the run. Each checked reference must equal an IRI the sibling's datasets mint under the sibling's own rules — a `key`-scoped record mints beneath its dataset root, so `namespace + bare id` guesses miss. Unresolved references warn; `--strict` fails on them. Naming the entry itself, or a name with no `[schemas]` entry, is an error |
| *(absence claims)* | There is no manifest key: the **schema** marks the claiming slot with an `asserts_absence` annotation (`annotations: {asserts_absence: {value: {via_slot: connecting_class}}}`, or `value: null` for a bare assertion), and claims are verified whenever `resolve_against` is set. A record listing anchors under a marked slot (references or IRI scalars) claims no single sibling record references them all — a single anchor claims no record references it at all ("references" = authored object-reference edges, not scalar IRI citations); `via_slot` (optional) names a slot whose value — a class IRI, CURIE or absolute — narrows the claim to joining records of that class. Holding is not joining at any depth: a container's collection slots and inlined children are containment, not citation (restating an already-declared record inline is a citation). A claim the check can't evaluate — a null or malformed anchor or via value, several via values, anchors collapsing to fewer distinct IRIs than authored, an anchor no sibling mints, a via naming no sibling class — is reported uncheckable, never as holding. Contradicted and uncheckable claims warn; `--strict` fails. Declaration scope follows LinkML: a top-level slot declaration binds every carrying class; an attribute's binds its class; `slot_usage` overrides. Declaration defects (unknown fields, a non-string or uncarried `via_slot`, an annotated slot no class carries) are load warnings that `--strict` refuses, and a schema declaring claims with no `resolve_against` gets a note that nothing verifies them. LinkML-YAML schemas only: a Turtle-authored schema cannot express the annotation until the RDF projection round-trips it |
| *(anchor expansion)* | Also schema-declared, no manifest key: a slot annotated `expand_against: <slot>` reads its scheme-less values as the named slot's value on the same record concatenated with the bare value (`target_schema: https://ns/` + `cabernet` → `https://ns/cabernet`); the base resolves on the record itself or, when absent there, on the nearest containing record — a benchmark states its target once above its questions. Values carrying a `:` — absolute IRIs, CURIEs — stay as authored, so out-of-namespace anchors remain expressible. Scope and overrides follow the absence declarations. A record supplying no single string base keeps its bare values and gets a verification finding. A class-ranged slot's declaration binds when no local record of its range class (or its `is_a` family) can exist: every ranging site is itself declared external and the family holds no `tree_root`; bare values then read as external references, and the base must form an absolute IRI (a relative base is a per-record finding). Otherwise — and for a self-referential base, an uncarried base, or a non-string value — the declaration is a load warning and does not expand |
| *(version pins)* | Also schema-declared, no manifest key: a slot annotated `records_version_of` (value `{sibling_slot: <slot>}`) holds the package version a record was written against, and `sibling_slot` names the slot whose value says which `resolve_against` sibling — by entry key, published name, its schema's `id` IRI, or a dataset name its publish manifest lists. The check compares it with the version the sibling resolved to: a mismatch warns per record and fails `--strict`; a pin it cannot evaluate (no single string version, an unknown sibling) is reported uncheckable, never as agreeing; versions match when they name the same semver release (`v` prefix allowed, build metadata ignored) |
| `require_namespace_coverage` | Opt-in: every external reference must land in a namespace some `resolve_against` sibling owns. Off, outside vocabularies stay unchecked by design; on, a typo'd namespace — which otherwise reads as an outside vocabulary and escapes every check — warns, and `--strict` fails on it |

## `[label_sources]`

Maps a **prefix name** (not a namespace IRI) to a label-source URL,
overriding the built-in map. Used to resolve labels for external groundings.

## `panschema.lock`

Written by `fetch`, checked by `fetch --check`. Records each dependency's resolved
source and a `sha256:` checksum of its main file. `generate` does not read
it.
