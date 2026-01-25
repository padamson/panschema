# Feature: Core Ontology Documentation

**Feature:** MVP Ontology Content Extraction & Display

**User Story:** As an ontology developer, I want to generate documentation that shows my classes, properties, and individuals, so that others can understand and use my ontology.

**Related ADR:** None yet

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

This implementation follows **vertical slicing** - each slice delivers end-to-end user value and can be tested/released independently.

Building on Feature 01 (Foundational UI Stack), this feature adds actual ontology content extraction and display using the existing component infrastructure.

*Documentation updates required:*
- [Main README](../../README.md) - Update once content is rendered
- [CHANGELOG](../../CHANGELOG.md) - Document each slice

---

## Vertical Slices

### Slice 1: Class Extraction & Display

**Status:** Completed

**User Value:** Users see their ontology's classes with labels, descriptions, and class hierarchy displayed in the documentation.

**Acceptance Criteria:**
- [x] Parser extracts owl:Class entities from Turtle files
- [x] Parser extracts rdfs:label and rdfs:comment for each class
- [x] Parser extracts rdfs:subClassOf relationships
- [x] Classes section displays all classes (not "0" count)
- [x] Class cards show label, description, parent class, and subclasses
- [x] E2E test verifies classes are rendered from reference.ttl

**Notes:**
- Uses existing class_card component
- Class hierarchy is displayed as links (parent/subclass)
- Reference ontology has: Animal → Mammal → Dog, Cat; Person

---

### Slice 2: Property Extraction & Display

**Status:** Completed

**User Value:** Users see their ontology's properties with types, domains, ranges, and descriptions.

**Acceptance Criteria:**
- [x] Parser extracts owl:ObjectProperty entities
- [x] Parser extracts owl:DatatypeProperty entities
- [x] Parser extracts rdfs:domain and rdfs:range for properties
- [x] Parser extracts owl:inverseOf relationships
- [x] Properties section displays all properties (not "0" count)
- [x] Property cards show label, description, type, domain, range
- [x] E2E test verifies properties are rendered from reference.ttl

**Notes:**
- Uses existing property_card component
- Domain/range resolved to class links (EntityRef) when the IRI matches a known class, otherwise displayed as datatype text (e.g., xsd:integer)
- Inverse-of relationships displayed as characteristics on property cards (e.g., "Inverse of: has owner")
- Reference ontology has: hasOwner, owns (object); hasName, hasAge (datatype)
- Sidebar simplified from individual entity listings to section-level links (Overview, Namespaces, Classes, Properties) with count badges
- Namespace count badge and section header with count added to main layout
- namespace_table.html refactored to remove its own heading/section wrapper (now wrapped by section_header in index.html)
- Metadata card heading renamed from "Ontology Metadata" to "Overview" for consistency with sidebar
- "Namespaces" link added to header navigation

---

### Slice 3: Individual Extraction & Display

**Status:** Not Started

**User Value:** Users see example individuals with their types and property values.

**Acceptance Criteria:**
- [ ] Parser extracts named individuals (entities with rdf:type pointing to a class)
- [ ] Parser extracts property values for individuals
- [ ] Individuals section displays all individuals
- [ ] Individual cards show label, type(s), and property values
- [ ] E2E test verifies individuals are rendered from reference.ttl

**Notes:**
- May need a new individual_card component (or extend class_card)
- Reference ontology has: fido (a Dog with name and age)

---

### Slice 4: Release v0.1.0

**Status:** Not Started

**User Value:** Users can install rontodoc from crates.io and generate useful documentation.

**Acceptance Criteria:**
- [ ] crates.io account configured with API token
- [ ] GitHub secret for CARGO_REGISTRY_TOKEN
- [ ] Release workflow publishes to crates.io on tag
- [ ] Tag v0.1.0 and verify CD pipeline
- [ ] `cargo install rontodoc` works from crates.io
- [ ] Remove "(Note: Not yet published to crates.io)" from README

**Notes:**
- Moved from Feature 01 Slice 6
- Only release once slices 1-3 provide real value

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Classes | Must Have | Feature 01 | Completed |
| Slice 2: Properties | Must Have | Slice 1 | Completed |
| Slice 3: Individuals | Should Have | Slice 2 | Not Started |
| Slice 4: Release | Must Have | Slice 1-3 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All acceptance criteria from user story are met
- [ ] All vertical slices marked as "Completed"
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete with examples: `cargo doc`
- [ ] Code formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
- [ ] v0.1.0 published to crates.io
