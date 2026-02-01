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
| `index.html` | Interactive HTML documentation with graph visualization |
| `panschema_viz.js` | WASM visualization JavaScript bindings |
| `panschema_viz_bg.wasm` | WASM binary for force simulation |
| `graph.json` | Graph topology JSON (optional standalone format) |

## Graph Visualization

The generated HTML includes an animated force-directed graph visualization:

```bash
# Generate HTML with visualization (default)
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output

# Test in browser
open examples/university/output/index.html  # macOS
xdg-open examples/university/output/index.html  # Linux
```

### Visualization Options

```bash
# Disable graph visualization
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output --no-graph

# Force 2D Canvas mode
cargo run -- generate --input examples/university/schema.yaml --output examples/university/output --viz-mode 2d
```

The visualization shows:
- **Nodes**: Classes (blue), Slots (green), Enums (purple), Types (orange)
- **Edges**: Labeled relationships (subclassOf, domain, range, etc.)
- **Interaction**: Drag to pan, scroll to zoom, touch-enabled

## Feature Status

| Feature | Status | Slice |
|---------|--------|-------|
| HTML Documentation | ✅ Available | - |
| OWL/Turtle Output | ✅ Available | - |
| Graph JSON Output | ✅ Available | 3 |
| GPU Force Simulation | ✅ Complete | 1 |
| 3D Renderer | ✅ Complete | 2 |
| 2D Canvas WASM | ✅ Complete | 4 |
| WebGPU 3D WASM | 🔲 Not Started | 4 |
| Interactive Controls | ✅ Complete | 4 |
