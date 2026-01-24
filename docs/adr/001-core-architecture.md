# ADR-001: Core Architecture

## Status
Accepted

## Context

Rontodoc needs an architecture that:
- Processes OWL 2 ontologies (.ttl files) and generates static HTML documentation
- Achieves millisecond generation times for CI/CD pipelines
- Produces a single binary with no external dependencies
- Supports full OWL 2 constructs (classes, properties, restrictions, axioms)
- Includes client-side search, filtering, and interactive graph visualization
- Enables hot-reload development workflow

Existing tools (Widoco, LODE) use Java with XML/XSLT, resulting in slow startup times and complex deployment. We need a clean, fast, Rust-native approach.

## Decision

### Pipeline Architecture

We adopt a pipeline pattern with distinct stages:

```
Input (.ttl) → Parse → Model → Analyze → Render → Output (HTML)
```

Each stage has a single responsibility:
- **Parse**: Read RDF/OWL syntax into raw triples
- **Model**: Build typed OWL 2 object graph
- **Analyze**: Extract hierarchies, compute metrics, prepare documentation data
- **Render**: Generate HTML from templates with bundled assets

### Module Organization

```
src/
├── main.rs              # Entry point
├── cli/                 # clap-based argument parsing
├── parser/              # RDF/OWL parsing via sophia crate
├── model/               # OWL 2 internal representation
│   ├── class.rs         # Classes and class expressions
│   ├── property.rs      # Object/data/annotation properties
│   ├── individual.rs    # Named individuals
│   ├── annotation.rs    # Labels, comments, custom annotations
│   └── restriction.rs   # Property restrictions
├── analyzer/            # Documentation extraction
│   ├── hierarchy.rs     # Taxonomy computation
│   └── metrics.rs       # Ontology statistics
├── renderer/            # HTML generation via askama
│   └── templates/       # Compile-time HTML templates
├── search/              # JSON index generation for client-side search
├── graph/               # Visualization data structure
└── error.rs             # Unified error types
```

### Output Structure

Generated documentation follows a predictable structure:

```
output/
├── index.html           # Ontology overview
├── classes/             # Class documentation
├── properties/          # Property documentation
├── individuals/         # Individual documentation
├── search-index.json    # Client-side search data
├── graph-data.json      # Visualization data
└── assets/              # CSS, JS (search, graph libs)
```

### CLI Design

```bash
rontodoc --input ontology.ttl --output docs/
rontodoc serve --input ontology.ttl --port 3000  # Dev mode
```

## Consequences

### Positive

- **Testability**: Each pipeline stage can be unit tested in isolation
- **Performance**: No runtime parsing of templates (askama compiles them)
- **Extensibility**: New output formats can be added as new renderers
- **Debuggability**: Intermediate representations can be inspected
- **Maintainability**: Clear module boundaries prevent coupling

### Negative

- **Compile time**: askama templates increase compilation time
- **Flexibility**: Compile-time templates require rebuild for template changes (mitigated by dev mode)
- **Complexity**: OWL 2 Full support requires comprehensive model layer
