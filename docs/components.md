# Component Development Guide

rontodoc uses a component-driven development workflow inspired by Storybook. Components are isolated, documented in a style guide, and validated with snapshot tests.

> **Note:** The styleguide command requires building with the `dev` feature flag.

## Quick Start

```bash
# Preview the style guide with live reload
cargo run --features dev -- styleguide --serve
```

Open http://localhost:3000/styleguide.html

## Directory Structure

```
templates/
├── base.html                    # Base layout with CSS design tokens
├── layout.html                  # Two-column layout with sidebar
├── index.html                   # Main documentation page
├── styleguide.html              # Component showcase page
└── components/
    ├── header.html              # Fixed site header with navigation
    ├── footer.html              # Site footer with attribution
    ├── hero.html                # Page hero section
    ├── metadata_card.html       # Ontology metadata display
    ├── sidebar.html             # Fixed sidebar navigation
    ├── namespace_table.html     # Namespace prefix/IRI table
    ├── section_header.html      # Section title with count badge
    ├── class_card.html          # Class documentation card
    └── property_card.html       # Property documentation card

src/
├── components.rs                # Component rendering and snapshot tests
└── renderer.rs                  # Page rendering (EntityRef, Namespace)
```

## Design Tokens

CSS custom properties defined in `base.html` for consistent styling.

### Colors
| Token | Usage |
|-------|-------|
| `--color-primary` | Links, interactive elements |
| `--color-primary-hover` | Hover states |
| `--color-text` | Primary text |
| `--color-text-muted` | Secondary text |
| `--color-bg` | Page background |
| `--color-bg-secondary` | Card backgrounds |
| `--color-border` | Borders, dividers |

### Semantic Colors
| Token | Usage |
|-------|-------|
| `--color-class` | Class labels and links |
| `--color-property` | Property labels and links |
| `--color-individual` | Individual labels |
| `--color-datatype` | Datatype labels |

### Layout
| Token | Usage |
|-------|-------|
| `--sidebar-width` | Fixed sidebar width (260px) |
| `--content-max-width` | Main content max width (900px) |
| `--header-height` | Fixed header height (60px) |

### Spacing
`--space-1` through `--space-12` for consistent spacing.

### Dark Mode
Automatically enabled via `prefers-color-scheme: dark`.

## Layout Templates

**base.html** - Root template with HTML structure and design tokens.

**layout.html** - Two-column layout with fixed header and sidebar. Extends base.html.

## Components

| Component | Purpose |
|-----------|---------|
| `header.html` | Fixed site header with navigation |
| `footer.html` | Site footer with attribution |
| `hero.html` | Page title and description |
| `metadata_card.html` | Ontology IRI, version, description |
| `sidebar.html` | Navigation with class/property links |
| `namespace_table.html` | Prefix/IRI mappings |
| `section_header.html` | Section title with count badge |
| `class_card.html` | OWL class documentation |
| `property_card.html` | OWL property documentation |

## Adding Components

1. Create template in `templates/components/`
2. Add Askama struct and render method in `src/components.rs`
3. Add to `styleguide.html`
4. Add snapshot tests

See existing components in `src/components.rs` for examples.
