# Panschema Roadmap

> **Note:** This project is evolving from `rontodoc` (OWL documentation generator) to `panschema` (a "pandoc for data modeling" tool). The rename occurs at v0.2.0.

## Vision

**Panschema** aims to be the universal tool for data modeling workflows:
- Convert between schema languages (LinkML, OWL/TTL, JSON Schema, SHACL, SQL DDL)
- Generate documentation from any supported format
- Validate schemas and check compatibility
- Compare schemas and track changes

Like pandoc for documents, panschema provides a single binary that bridges the data modeling ecosystem.

## Architecture

See ADRs for architectural decisions:
- [ADR-003: LinkML as Internal Representation](adr/003-linkml-as-internal-representation.md)
- [ADR-004: Reader/Writer Architecture](adr/004-reader-writer-architecture.md)

### Core Pipeline

```
Input → Reader → LinkML IR → [Filters] → Writer → Output
```

| Component | Description |
|-----------|-------------|
| **Readers** | Parse input formats into LinkML IR |
| **Writers** | Generate output formats from LinkML IR |
| **Filters** | Transform IR (optional, user-customizable) |

## Release Strategy

### v0.1.0 — OWL Documentation MVP ✅
*Released as `rontodoc`*

- Turtle (.ttl) parser for OWL ontologies
- Documentation generation: classes, properties, individuals
- Development server with hot reload
- Cross-platform binaries (Linux, macOS, Windows)

### v0.2.0 — Reader/Writer Architecture (Next)
*Rename to `panschema`*

**Goal:** Refactor to reader/writer architecture while preserving existing functionality.

Same user experience:
```bash
panschema doc input.ttl    # Same output as rontodoc v0.1.0
```

New internal architecture:
```
input.ttl → OwlReader → LinkML IR → HtmlWriter → HTML
```

Scope:
- Define LinkML IR structs (core subset)
- Implement `OwlReader` (refactor existing parser + mapping layer)
- Implement `HtmlWriter` (refactor existing renderer)
- Rename CLI to `panschema`
- Existing E2E tests continue passing

### v0.3.0 — LinkML Input
**Goal:** Parse LinkML YAML and generate documentation.

```bash
panschema doc input.yaml    # NEW: docs from LinkML
```

Scope:
- Implement `LinkmlReader`
- Both TTL and YAML inputs produce consistent documentation

### v0.4.0 — Format Conversion
**Goal:** Convert between OWL and LinkML.

```bash
panschema convert input.ttl --to linkml    # TTL → LinkML YAML
panschema convert input.yaml --to ttl      # LinkML → TTL
```

Scope:
- Implement `LinkmlWriter`
- Implement `OwlWriter`
- Round-trip tests

### v0.5.0 — Schema Validation
**Goal:** Validate schemas with actionable error messages.

```bash
panschema validate input.yaml
panschema validate input.ttl
```

### v0.6.0 — JSON Schema Support
**Goal:** Convert between LinkML and JSON Schema.

```bash
panschema convert input.yaml --to json-schema
panschema doc input.json    # Docs from JSON Schema
```

### v1.0.0 — Production Ready
- Comprehensive format support
- Full OWL 2 and LinkML metamodel coverage
- Stable CLI and library API
- Plugin architecture for custom formats

## Feature Specifications

| # | Feature | Description | Status |
|---|---------|-------------|--------|
| 01 | [Foundational UI Stack](features/01-foundational-ui-stack.md) | Walking skeleton: CLI, Turtle parsing, HTML output, dev server | **Released v0.1.0** |
| 02 | [Core Ontology Documentation](features/02-core-ontology-documentation.md) | Classes, properties, individuals extraction and display | **Released v0.1.0** |
| 03 | [Reader/Writer Architecture](features/03-reader-writer-architecture.md) | Refactor to LinkML IR with OwlReader and HtmlWriter | **In Progress (v0.2.0)** |
| 04 | LinkML Input | LinkmlReader for parsing YAML schemas | Planned (v0.3.0) |
| 05 | Format Conversion | LinkmlWriter, OwlWriter for bidirectional conversion | Planned (v0.4.0) |
| 06 | Schema Validation | Validate LinkML and OWL with error messages | Planned (v0.5.0) |
| 07 | JSON Schema Support | JsonSchemaReader, JsonSchemaWriter | Planned (v0.6.0) |

## Delivery Approach

Each feature is a **vertical slice** that delivers working functionality:

1. **Incremental Refactoring** — v0.2.0 preserves existing behavior while introducing new architecture
2. **TDD Throughout** — Every slice includes tests before implementation
3. **Spec-Driven** — LinkML implementation follows official specification
4. **Outside-In Development** — Start with user-facing behavior, work inward
