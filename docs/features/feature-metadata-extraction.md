# Ontology Metadata Extraction - Implementation Plan

**Feature:** Metadata Extraction & Structure

**User Story:** As an ontology author, I want `rontodoc` to automatically extract my ontology's metadata (title, version, abstract) and structural elements so that the generated documentation gives users context about the domain.

**Related ADR (if applicable):** N/A

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

This implementation follows **vertical slicing** - each slice delivers end-to-end user value and can be tested/released independently.

*When developing this implementation plan, also consider the following documentation, and note any updates to documentation required by the user story implementation:*
- [Main README](../../README.md)
- [WHY](../../WHY.md) (To reflect that we now support W3C compliant metadata)

---

## Vertical Slices

### Slice 0: Walking Skeleton (Baseline)

**Status:** Completed

**User Value:** A "Hello World" version of the tool that proves the end-to-end pipeline (CLI -> Read -> HTML Output) works, even if the data is hardcoded.

**Acceptance Criteria:**
- [x] CLI accepts `--input` and `--output`.
- [x] Parser loads a valid TTL file (using `oxigraph`).
- [x] Generator outputs a W3C-style HTML file.
- [x] E2E Tests verify the generated HTML loads in a browser.

**Notes:**
- Hardcoded data used to verify pipeline connectivity.
- E2E tests served as the primary verification mechanism.

---

### Slice 1: Core Metadata Extraction

**Status:** Completed

**User Value:** Users see the actual Title, Version, and Description of the ontology in the HTML header, rather than hardcoded placeholders.

**Acceptance Criteria:**
- [x] Implement `src/extractor.rs` to query `oxigraph` Store.
- [x] Extract `dc:title` (or `rdfs:label`, `skos:prefLabel`, `dcterms:title`).
- [x] Extract `owl:versionInfo` (or `dc:date`, `dcterms:modified`).
- [x] Extract `dc:description` (or `rdfs:comment`, `dcterms:description`).
- [x] Validated by `tests/e2e_generation.rs`.

**Notes:**
- We need to handle ontologies with multiple `owl:Ontology` definitions (warn or pick first).
- Fallback logic: If no title, use URI fragment.

---

### Slice 2: Structural Elements (Abstract & Namespaces)

**Status:** Completed

**User Value:** Users get a high-level "Abstract" section and a "Namespaces" table, which are critical W3C / LODE specification requirements.

**Acceptance Criteria:**
- [x] Generate "Abstract" HTML section from Description candidates.
- [x] Generate "Namespaces" table by scanning source file for `@prefix` definitions (instead of scanning used IRIs).
- [x] Update `templates/index.html` to include these sections.

---

### Slice 3: Core Entities (Classes & Properties)

**Status:** Completed

**User Value:** The core content of the ontology (Classes, Object Properties, Data Properties) is extracted and displayed, replacing hardcoded placeholders.

**Acceptance Criteria:**
- [x] Extract `owl:Class` entities with Label & Description.
- [x] Extract `owl:ObjectProperty` entities.
- [x] Extract `owl:DatatypeProperty` entities.
- [x] Update `src/extractor.rs` to use SPARQL for entity extraction.
- [x] Remove temporary hardcoded entities from `main.rs`.

---

### Slice 4: Annotations & Named Individuals

**Status:** Completed

**User Value:** Users can see Annotation Properties and Named Individuals documentation sections, ensuring full coverage of ontology entities.

**Acceptance Criteria:**
- [x] Extract `owl:AnnotationProperty` entities.
- [x] Extract `owl:NamedIndividual` entities.
- [x] Add corresponding sections to HTML template and TOC.

---

### Slice 5: Cross-Reference (Table of Contents)

**Status:** Completed

**User Value:** Users can easily navigate the ontology through a "Cross Reference" section (Table of Contents) that groups entities by type (Classes, Properties, etc.) and provides quick links to their definitions.

**Acceptance Criteria:**
- [x] Generate a "Cross Reference" section in the HTML.
- [x] Group entities by type (`Classes`, `Object Properties`, `Data Properties`, `Named Individuals`, `Annotation Properties`).
- [x] Provide anchor links to the full entity definitions.
- [x] Ensure the TOC is dynamically generated based on extracted entities.

---

### Slice 6: Detailed Class Axioms

**Status:** Completed

**User Value:** Users gain deeper insight into the logical structure of the ontology by seeing the relationships between classes, specifically their parent classes (Superclasses) and which classes they are disjoint with.

**Acceptance Criteria:**
- [x] Extract `rdfs:subClassOf` relationships for Classes.
- [x] Extract `owl:disjointWith` relationships for Classes.
- [x] Handle symmetric nature of `owl:disjointWith` (A disjointWith B implies B disjointWith A).
- [x] Update `extractor.rs` to include these fields in the `Entity` struct.
- [x] Display "Superclasses" and "Disjoint Classes" sections in the HTML for each Class.
