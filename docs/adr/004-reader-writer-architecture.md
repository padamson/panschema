# ADR-004: Reader/Writer Architecture

## Status
Accepted

## Context

With LinkML chosen as the internal representation (ADR-003), we need to define how components read input formats into the IR and write outputs from it. This architecture must:

- Support multiple input formats (OWL/TTL, LinkML YAML, future: JSON Schema, SHACL)
- Support multiple output formats (OWL/TTL, LinkML YAML, HTML documentation)
- Preserve format-specific metadata through conversions
- Be extensible for new formats without modifying existing code
- Generate consistent documentation regardless of input format

## Decision

### Reader/Writer Traits

We adopt a trait-based architecture where each format implements standardized interfaces:

```rust
pub trait Reader {
    /// Parse input into LinkML IR
    fn read(&self, input: &Path) -> Result<SchemaDefinition>;

    /// File extensions this reader handles
    fn supported_extensions(&self) -> &[&str];
}

pub trait Writer {
    /// Write LinkML IR to output format
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> Result<()>;

    /// Output format identifier
    fn format_id(&self) -> &str;
}
```

### Component Pipeline

```
Input File → detect format → Reader → SchemaDefinition (IR) → Writer → Output
```

Format detection uses file extension, with optional explicit override via CLI flag.

### v0.2.0 Components

| Component | Type | Description |
|-----------|------|-------------|
| `OwlReader` | Reader | Parse OWL/TTL into IR (refactored from existing parser) |
| `YamlReader` | Reader | Parse LinkML YAML directly into IR |
| `HtmlWriter` | Writer | Generate documentation (refactored from existing renderer) |
| `OwlWriter` | Writer | Generate OWL/Turtle using sophia |
| `JsonLdWriter` | Writer | Generate JSON-LD using sophia |
| `RdfXmlWriter` | Writer | Generate RDF/XML using sophia |
| `NTriplesWriter` | Writer | Generate N-Triples using sophia |

### Source Metadata Preservation

Format-specific details that don't map directly to LinkML core elements are preserved via annotations:

```yaml
# Example: OWL-specific metadata preserved in IR
classes:
  Person:
    annotations:
      panschema:source_format: "owl"
      panschema:owl_restriction: "hasAge exactly 1 xsd:integer"
```

This allows:
- Lossless round-trips where possible
- Format-specific sections in documentation
- Debugging and provenance tracking

### HtmlWriter Design

The documentation generator follows these principles:

1. **Format-agnostic core**: Classes, properties, and relationships render identically regardless of source format

2. **Semantic rendering**: Documentation reflects the LinkML IR semantics, not source syntax

3. **Optional format-specific sections**: Templates can conditionally display:
   - "OWL Details" for OWL-sourced schemas (restrictions, axioms)
   - "LinkML Details" for LinkML-sourced schemas (slot_usage, rules)

4. **Consistent visual identity**: Same CSS, layout, and navigation for all schemas

### Writer Implementation Patterns

#### RDF Writers

Writers producing RDF formats (Turtle, JSON-LD, RDF/XML, N-Triples) share a common implementation pattern using the [sophia](https://crates.io/crates/sophia) RDF library:

```
SchemaDefinition (LinkML IR)
        │
        ▼
  build_rdf_graph()
        │
        ▼
  sophia::FastGraph (transient)
        │
        ├──► TurtleSerializer ──► .ttl
        ├──► JsonLdSerializer ──► .jsonld
        ├──► RdfXmlSerializer ──► .rdf
        └──► NtSerializer ────► .nt
```

This approach provides:

1. **Semantic consistency**: Same triples across all RDF formats
2. **Correctness**: sophia handles RDF edge cases (escaping, blank nodes, datatypes)
3. **Single mapping**: One `build_rdf_graph()` function maps LinkML IR to RDF triples
4. **Maintainability**: Adding new RDF formats requires only a new serializer call

The sophia graph is **transient** — built on-demand for serialization, then discarded. The LinkML IR remains the canonical representation; the sophia graph is purely a serialization adapter.

#### Non-RDF Writers

Writers for non-RDF formats (HTML, YAML, JSON Schema) work directly with the LinkML IR without an intermediate representation.

### Adding New Formats

To add support for a new format (e.g., JSON Schema):

1. Implement `JsonSchemaReader` (if reading JSON Schema)
2. Implement `JsonSchemaWriter` (if writing JSON Schema)
3. Register with format dispatcher
4. No changes to existing readers, writers, or IR

## Consequences

### Positive

- **Single Responsibility**: Each reader/writer handles one format
- **Open/Closed**: New formats extend the system without modifying existing code
- **Testability**: Each component can be tested in isolation
- **Consistent Documentation**: Users get familiar UI regardless of input format
- **Metadata Preservation**: Format-specific details aren't lost in conversion

### Negative

- **Annotation Complexity**: Heavy use of annotations for format-specific metadata could become unwieldy
- **Mapping Challenges**: Some format constructs may not map cleanly to LinkML IR
- **Testing Surface**: Each reader/writer combination needs integration testing

### Open Questions (To Resolve During Implementation)

1. **Annotation namespace**: Should we use `panschema:` prefix or something else?
2. **Streaming vs. in-memory**: For large schemas, should readers support streaming?
3. **Validation hooks**: Should readers validate input, or is that a separate concern?
4. **Template customization**: How much control should users have over HtmlWriter output?

## References

- [ADR-003: LinkML as Internal Representation](003-linkml-as-internal-representation.md)
- [Pandoc Architecture](https://pandoc.org/MANUAL.html) - inspiration for reader/writer pattern
