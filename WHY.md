# Why panschema?

## The Problem: Data Modeling is Fragmented

Data modeling spans many schema languages, each with its own ecosystem:

| Schema Language | Domain | Tools |
|----------------|--------|-------|
| **OWL/RDF** | Semantic Web, Knowledge Graphs | Protégé, Widoco, LODE |
| **LinkML** | Data modeling, FAIR data | gen-linkml |
| **JSON Schema** | APIs, Configuration | Various generators |
| **SHACL** | RDF validation | SHACL validators |
| **SQL DDL** | Databases | Database-specific |

Each has its own documentation tools, conversion utilities, and validation approaches. Teams working across domains must learn multiple toolchains.

## The Vision: pandoc for Data Modeling

**pandoc** revolutionized document conversion — one tool that speaks every document format.

**panschema** aims to do the same for data modeling:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  OWL/TTL    │     │             │     │    HTML     │
│  LinkML     │ ──► │  LinkML IR  │ ──► │  Markdown   │
│  JSON Schema│     │ (canonical) │     │  LinkML     │
│  SHACL      │     │             │     │  JSON Schema│
└─────────────┘     └─────────────┘     └─────────────┘
    Readers              Core              Writers
```

### Core Capabilities

1. **Convert** between schema languages
   - OWL → LinkML → JSON Schema
   - Preserve semantics across formats

2. **Generate documentation** from any format
   - Beautiful, responsive HTML
   - Searchable, AI-readable

3. **Validate** schemas
   - Check syntax and semantics
   - Verify compatibility between versions

4. **Compare** schemas
   - Diff two schemas
   - Track changes over time

## Why Rust?

Current ontology tools often require:
- A JVM (Widoco, LODE, Protégé)
- Python environments (LinkML generators)
- Slow startup times
- Heavy CI containers

panschema is:
- **Single Binary**: No runtime dependencies
- **Blazing Fast**: Millisecond documentation generation
- **CI Native**: Perfect for GitHub Actions
- **Memory Safe**: Rust's guarantees

## The Goal

Make working with any schema language as easy as working with Markdown:

```bash
# Document an ontology
panschema doc ontology.ttl

# Convert OWL to LinkML
panschema convert input.ttl --to linkml

# Validate a schema
panschema validate schema.yaml

# Compare two versions
panschema diff v1.ttl v2.ttl
```

> "If it's not documented, it doesn't exist. If documentation is hard, it won't happen."

panschema makes it easy.
