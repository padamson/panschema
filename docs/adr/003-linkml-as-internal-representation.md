# ADR-003: LinkML as Internal Representation

## Status
Accepted

## Context

With rontodoc v0.1.0 released, we are pivoting toward a broader vision: **panschema** — a "pandoc for data modeling" tool that supports bidirectional conversion between schema languages and generates documentation from multiple formats.

The first expanded use case is supporting both LinkML and OWL/TTL formats:
- Convert LinkML schemas to OWL ontologies
- Convert OWL ontologies to LinkML schemas
- Generate documentation from either format
- Enable side-by-side documentation views (same model in both formats)

This requires choosing an internal representation (IR) that serves as the canonical data structure through which all formats pass, similar to how pandoc uses an AST as its universal intermediate format.

### Options Considered

1. **Custom IR**: Design our own schema representation from scratch
   - Pro: Tailored exactly to our needs
   - Con: Must define and maintain mappings to every format ourselves
   - Con: No community validation or tooling support

2. **OWL as IR**: Use OWL 2 constructs as the internal model
   - Pro: Already implemented in rontodoc
   - Con: OWL is more expressive than LinkML in some ways, less in others
   - Con: Lossy round-trips for LinkML-specific features (slots, enums, types)

3. **LinkML as IR**: Use LinkML's metamodel as the canonical representation
   - Pro: LinkML was explicitly designed as a bridge between schema languages
   - Pro: Well-defined mappings to OWL, JSON Schema, SHACL, SQL DDL already exist
   - Pro: Active community and tooling ecosystem
   - Pro: Human-readable YAML format for debugging and manual editing
   - Con: Rust implementation needed (no official Rust crate yet)

## Decision

**We will use LinkML's metamodel as the internal representation**, implementing a core subset in Rust.

### Core Subset for v0.2.0

The LinkML specification ([linkml.io/linkml-model/latest/docs/specification/](https://linkml.io/linkml-model/latest/docs/specification/)) defines multiple profiles for different implementation depths:

- **MinimalSubset**: Essential core elements
- **BasicSubset**: Standard functionality
- **SpecificationSubset**: Full specification coverage

For v0.2.0, we target the **MinimalSubset** plus key elements from BasicSubset:

| Element | Description |
|---------|-------------|
| `SchemaDefinition` | Root container with name, prefixes, imports |
| `ClassDefinition` | Classes with attributes, inheritance, mixins |
| `SlotDefinition` | Slots (properties) with range, cardinality, constraints |
| `EnumDefinition` | Enumerated value sets with permissible values |
| `TypeDefinition` | Custom types mapping to XSD datatypes |

Future releases will expand toward full SpecificationSubset coverage.

### Modular Reader/Writer Architecture

Following pandoc's proven design, panschema adopts a **modular architecture** with distinct readers, writers, and filters:

```
Input → Reader → LinkML IR → [Filters] → Writer → Output
```

**Readers** parse input formats into the LinkML IR:
- `LinkmlReader`: Parse LinkML YAML directly into IR
- `OwlReader`: Parse OWL/TTL and map to IR (leverages existing rontodoc parser)
- Future: `JsonSchemaReader`, `ShaclReader`, `SqlDdlReader`

**Writers** convert the LinkML IR to output formats:
- `LinkmlWriter`: Serialize IR back to LinkML YAML
- `OwlWriter`: Generate OWL/TTL from IR
- `HtmlWriter`: Generate documentation (extends existing renderer)
- Future: `JsonSchemaWriter`, `ShaclWriter`, `SqlDdlWriter`

**Filters** (optional) transform the IR between reading and writing:
- Users can implement custom filters to modify schemas
- Built-in filters for common transformations (e.g., flattening inheritance, adding annotations)

This design means adding a new format requires only implementing a reader, a writer, or both — not N×N converters.

### Supported Conversions in v0.2.0

- **LinkML YAML → LinkML IR**: Direct parsing via `LinkmlReader`
- **OWL/TTL → LinkML IR**: `OwlReader` using existing parser + mapping layer
- **LinkML IR → OWL/TTL**: `OwlWriter` generates Turtle output
- **LinkML IR → Documentation**: `HtmlWriter` extends existing renderer

### Why LinkML Fits

LinkML was designed with explicit goals that align with panschema:

1. **Bridge Format**: LinkML's metamodel intentionally captures concepts common across schema languages
2. **Well-Defined Mappings**: Official generators exist for OWL, JSON Schema, SHACL, SQL DDL, providing reference implementations
3. **Minimal Impedance**: Converting OWL ↔ LinkML loses less information than OWL ↔ custom IR
4. **Extensible**: Annotations and extensions allow preserving format-specific metadata

### Specification-Driven TDD

The LinkML specification provides a formal, testable definition of the metamodel. We adopt a **specification-as-tests** approach:

1. **Extract requirements from spec**: Each specification section defines behavior that can be encoded as test cases
2. **Write tests first**: Create tests that encode specification requirements before implementation
3. **Implement to pass tests**: Build the minimal implementation that satisfies the specification tests
4. **Validate against reference**: Compare panschema output against official LinkML tooling (Python `linkml` package) for conformance

Example workflow for implementing `ClassDefinition`:
```
1. Read spec section on ClassDefinition
2. Create test: "class with attributes parses correctly"
3. Create test: "class inheritance (is_a) resolves correctly"
4. Create test: "class mixins merge slots correctly"
5. Implement ClassDefinition struct and parsing
6. Validate: parse same schema with linkml-python, compare output
```

This approach ensures our implementation is **specification-compliant** and provides a clear measure of completeness against the official spec.

### Implementation Approach

Since there's no official Rust LinkML crate:

1. Define Rust structs mirroring LinkML's metamodel (serde-compatible)
2. Implement YAML parser using `serde_yaml`
3. Write specification-derived tests for each metamodel element
4. Implement readers (`LinkmlReader`, `OwlReader`) with spec compliance tests
5. Implement writers (`OwlWriter`, `HtmlWriter`) with round-trip tests
6. Validate against official LinkML Python tooling for conformance

## Consequences

### Positive

- **Ecosystem Alignment**: Following LinkML's metamodel means we can leverage existing documentation, examples, and community knowledge
- **Future-Proof**: Additional formats (JSON Schema, SHACL, SQL DDL) can be added by implementing a reader or writer — not N×N converters
- **Interoperability**: Schemas created in panschema are compatible with the broader LinkML ecosystem
- **Simpler Maintenance**: Modular reader/writer architecture isolates format-specific code
- **Round-Trip Fidelity**: LinkML's design minimizes information loss during conversions
- **Spec-Driven Quality**: Tests derived from official specification ensure conformance and provide clear completeness metrics
- **Extensibility**: Filter architecture allows users to customize schema transformations

### Negative

- **Implementation Effort**: Must build LinkML support in Rust from scratch
- **Subset Limitations**: Initial v0.2.0 won't support full LinkML metamodel (rules, structured patterns, etc.)
- **Dependency on External Spec**: LinkML metamodel changes would require updates
- **Learning Curve**: Contributors need familiarity with LinkML concepts

### Migration Path

The existing rontodoc OWL model will be refactored into the reader/writer architecture:

1. Current `OntologyClass`, `OntologyProperty`, `OntologyIndividual` become intermediate types within `OwlReader`
2. `OwlReader` parses TTL, builds intermediate types, then maps to LinkML IR
3. Existing renderer becomes `HtmlWriter`, accepting LinkML IR instead of OWL types
4. Eventually, direct OWL → renderer path is replaced by OWL → LinkML IR → HTML pipeline

This migration can be done incrementally, maintaining backward compatibility until the full reader/writer architecture is in place.
