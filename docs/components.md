# Component Development Guide

This guide explains how to develop, preview, and test UI components for rontodoc's generated documentation.

> **Note:** The styleguide command requires building with the `dev` feature flag.
> This is intended for rontodoc contributors, not end users.

## Overview

rontodoc uses a component-driven development workflow inspired by Storybook. Components are:

- **Isolated**: Each component can be rendered and tested independently
- **Documented**: All components are showcased in a style guide
- **Tested**: Snapshot tests ensure components render consistently

## Directory Structure

```
templates/
├── base.html                    # Base layout template
├── index.html                   # Main documentation page
├── styleguide.html              # Component showcase page
└── components/
    ├── header.html              # Site header with navigation
    ├── footer.html              # Site footer with attribution
    ├── hero.html                # Page hero section
    └── metadata_card.html       # Ontology metadata display

src/
├── components.rs                # Component rendering module
└── snapshots/                   # Insta snapshot files
```

## Previewing the Style Guide

Generate and preview the style guide to see all components. You must build with the `dev` feature:

```bash
# Build with dev feature
cargo build --features dev

# Generate style guide
cargo run --features dev -- styleguide

# Generate and serve with live reload
cargo run --features dev -- styleguide --serve

# Use custom port
cargo run --features dev -- styleguide --serve --port 4000
```

Open http://localhost:3000/styleguide.html in your browser.

## Adding a New Component

### Step 1: Create the Template

Create a new file in `templates/components/`:

```html
<!-- templates/components/my_component.html -->
<div class="my-component">
    <h3>{{ title }}</h3>
    {% if let Some(description) = description %}
    <p>{{ description }}</p>
    {% endif %}
</div>
<style>
    .my-component {
        padding: 1rem;
        border: 1px solid var(--color-border);
        border-radius: 0.5rem;
    }
</style>
```

**Guidelines:**
- Use CSS custom properties (design tokens) from `base.html`
- Include component-specific styles in a `<style>` tag
- Use Askama's `{% if let Some(...) %}` for optional fields

### Step 2: Add the Template Struct

In `src/components.rs`, add an Askama template struct:

```rust
/// My component template.
#[derive(Template)]
#[template(path = "components/my_component.html")]
pub struct MyComponentTemplate<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
}
```

### Step 3: Add Render Method

Add a render method to `ComponentRenderer`:

```rust
impl ComponentRenderer {
    /// Render my component.
    pub fn my_component(title: &str, description: Option<&str>) -> anyhow::Result<String> {
        let template = MyComponentTemplate { title, description };
        Ok(template.render()?)
    }
}
```

### Step 4: Add to Style Guide

Update `templates/styleguide.html` to include your component:

```html
<section class="component-section">
    <h2 class="component-name">My Component</h2>
    <p class="component-description">
        Description of what this component does.
    </p>
    <div class="component-preview">
        <div class="preview-label">Preview</div>
        <div class="preview-content">
            {% include "components/my_component.html" %}
        </div>
    </div>
    <p class="component-file">Template: <code>templates/components/my_component.html</code></p>
</section>
```

### Step 5: Add Snapshot Tests

Add snapshot tests in `src/components.rs`:

```rust
mod snapshots {
    #[test]
    fn snapshot_my_component() {
        let html = ComponentRenderer::my_component("Title", Some("Description")).unwrap();
        insta::assert_snapshot!(html);
    }

    #[test]
    fn snapshot_my_component_minimal() {
        let html = ComponentRenderer::my_component("Title", None).unwrap();
        insta::assert_snapshot!(html);
    }
}
```

### Step 6: Accept Snapshots

Run tests and accept new snapshots:

```bash
cargo nextest run

# If you have cargo-insta installed:
cargo insta review

# Or manually accept by renaming:
mv src/snapshots/*.snap.new src/snapshots/*.snap
```

## Design Tokens

Use these CSS custom properties for consistent styling:

| Token | Value | Usage |
|-------|-------|-------|
| `--color-primary` | `#2563eb` | Links, interactive elements |
| `--color-primary-dark` | `#1d4ed8` | Hover states |
| `--color-text` | `#1f2937` | Primary text |
| `--color-text-muted` | `#6b7280` | Secondary text |
| `--color-bg` | `#ffffff` | Page background |
| `--color-bg-secondary` | `#f9fafb` | Card backgrounds |
| `--color-border` | `#e5e7eb` | Borders, dividers |
| `--font-sans` | System fonts | Body text |
| `--font-mono` | Monospace fonts | Code, IRIs |

## Existing Components

### Header (`components/header.html`)
Site header with ontology title and navigation links.

**Props:**
- `title`: Ontology name

### Footer (`components/footer.html`)
Site footer with rontodoc attribution.

**Props:** None

### Hero (`components/hero.html`)
Large title section at the top of the documentation.

**Props:**
- `title`: Ontology name
- `comment`: Optional description

### Metadata Card (`components/metadata_card.html`)
Displays ontology metadata in a structured format.

**Props:**
- `iri`: Ontology IRI
- `version`: Optional version string
- `comment`: Optional description

## Testing

Run all component tests:

```bash
cargo nextest run components
```

Update snapshots after intentional changes:

```bash
# Run tests to generate .snap.new files
cargo nextest run

# Review and accept changes
for f in src/snapshots/*.snap.new; do mv "$f" "${f%.new}"; done
```
