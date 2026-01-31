# University Schema Example

This example demonstrates panschema's schema conversion and visualization capabilities using a university domain schema.

## Schema Overview

The `schema.yaml` file defines a LinkML schema for university entities:

- **Class Hierarchy**:
  - `NamedEntity` (abstract base)
    - `Person` → `Student`, `Faculty`, `Staff`
    - `Department`
    - `Course`
    - `ResearchGroup`

- **Relationships**:
  - Students enroll in Courses
  - Faculty teach Courses
  - Departments contain Faculty and Courses
  - Courses have prerequisites (self-reference)
  - ResearchGroups have Faculty leads and Person members

## Commands

> **Note:** During development, use `cargo run --` instead of `panschema`.
> For release builds, use `cargo run --release --`.

### Generate HTML Documentation

```bash
# Development (from repository root):
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output/docs.html

# If panschema is installed:
panschema generate --input examples/university/schema.yaml --output examples/university/output/docs.html
```

### Convert to OWL/Turtle

```bash
# Development:
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output/schema.ttl --format ttl

# Installed:
panschema generate --input examples/university/schema.yaml --output examples/university/output/schema.ttl --format ttl
```

### Start Development Server with Hot Reload

```bash
# Development:
cargo run -- serve --input examples/university/schema.yaml

# Installed:
panschema serve --input examples/university/schema.yaml
# Opens browser at http://localhost:3000
```

### Generate Graph JSON (for visualization)

```bash
# Development:
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output/graph.json --format graph-json

# Installed:
panschema generate --input examples/university/schema.yaml --output examples/university/output/graph.json --format graph-json
```

## Expected Output

After running the commands, `examples/university/output/` will contain:

| File | Description |
|------|-------------|
| `docs.html` | Interactive HTML documentation with class browser |
| `schema.ttl` | OWL ontology in Turtle format |
| `graph.json` | Graph topology JSON for visualization |

## GPU Visualization (Future)

When Slice 4 is complete, additional commands will be available:

```bash
# Generate HTML with embedded 3D visualization (Slice 4)
cargo run --features gpu -- generate --input examples/university/schema.yaml --output examples/university/output/viz.html --format html --graph
```

The visualization will show:
- **Nodes**: Classes as blue spheres, Slots as green, Enums as purple, Types as orange
- **Edges**: Inheritance (is_a), Mixins, Domain/Range, Inverse
- **Interaction**: Orbit camera, zoom, pan, click to select

## Feature Status

| Feature | Status | Slice |
|---------|--------|-------|
| HTML Documentation | ✅ Available | - |
| OWL/Turtle Output | ✅ Available | - |
| Graph JSON Output | ✅ Available | 3 |
| GPU Force Simulation | ✅ Complete | 1 |
| 3D Renderer | ✅ Complete | 2 |
| HTML+WASM Integration | 🔲 Not Started | 4 |
| Interactive Controls | 🔲 Not Started | 5 |
