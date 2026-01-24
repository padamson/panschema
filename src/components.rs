//! Component rendering for isolated preview and testing.
//!
//! This module provides the ability to render individual UI components
//! in isolation, enabling:
//! - Snapshot testing of component HTML output
//! - Style guide generation
//! - Component-driven development workflow
//!
//! # Adding New Components
//!
//! 1. Create the template in `templates/components/your_component.html`
//! 2. Add the Askama template struct in this module
//! 3. Add a render method to `ComponentRenderer`
//! 4. Add the component to the style guide template
//! 5. Write snapshot tests for the component

// These components are primarily used for testing and the styleguide feature.
// They will be used more extensively as we build out the documentation layout.
#![allow(dead_code)]

use askama::Template;

/// Sample data for component previews and testing.
#[derive(Debug, Clone)]
pub struct SampleData {
    pub title: String,
    pub iri: String,
    pub version: Option<String>,
    pub comment: Option<String>,
}

impl Default for SampleData {
    fn default() -> Self {
        Self {
            title: "Example Ontology".to_string(),
            iri: "https://example.org/ontology/example".to_string(),
            version: Some("1.0.0".to_string()),
            comment: Some("An example ontology for demonstrating rontodoc components.".to_string()),
        }
    }
}

impl SampleData {
    /// Create sample data with minimal fields (no optional values).
    pub fn minimal() -> Self {
        Self {
            title: "Minimal Ontology".to_string(),
            iri: "https://example.org/minimal".to_string(),
            version: None,
            comment: None,
        }
    }
}

/// Header component template.
#[derive(Template)]
#[template(path = "components/header.html")]
pub struct HeaderComponent<'a> {
    pub title: &'a str,
}

/// Footer component template.
#[derive(Template)]
#[template(path = "components/footer.html")]
pub struct FooterComponent;

/// Hero component template.
#[derive(Template)]
#[template(path = "components/hero.html")]
pub struct HeroComponent<'a> {
    pub title: &'a str,
    pub comment: Option<&'a str>,
}

/// Metadata card component template.
#[derive(Template)]
#[template(path = "components/metadata_card.html")]
pub struct MetadataCardComponent<'a> {
    pub iri: &'a str,
    pub version: Option<&'a str>,
    pub comment: Option<&'a str>,
}

/// Style guide page template.
#[derive(Template)]
#[template(path = "styleguide.html")]
pub struct StyleGuideTemplate<'a> {
    pub title: &'a str,
    pub iri: &'a str,
    pub version: Option<&'a str>,
    pub comment: Option<&'a str>,
}

/// Renders individual components for testing and preview.
pub struct ComponentRenderer;

impl ComponentRenderer {
    /// Render the header component.
    pub fn header(title: &str) -> anyhow::Result<String> {
        let template = HeaderComponent { title };
        Ok(template.render()?)
    }

    /// Render the footer component.
    pub fn footer() -> anyhow::Result<String> {
        let template = FooterComponent;
        Ok(template.render()?)
    }

    /// Render the hero component.
    pub fn hero(title: &str, comment: Option<&str>) -> anyhow::Result<String> {
        let template = HeroComponent { title, comment };
        Ok(template.render()?)
    }

    /// Render the metadata card component.
    pub fn metadata_card(
        iri: &str,
        version: Option<&str>,
        comment: Option<&str>,
    ) -> anyhow::Result<String> {
        let template = MetadataCardComponent {
            iri,
            version,
            comment,
        };
        Ok(template.render()?)
    }

    /// Render the complete style guide page.
    pub fn styleguide(data: &SampleData) -> anyhow::Result<String> {
        let template = StyleGuideTemplate {
            title: &data.title,
            iri: &data.iri,
            version: data.version.as_deref(),
            comment: data.comment.as_deref(),
        };
        Ok(template.render()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_renders() {
        let html = ComponentRenderer::header("Test Ontology").unwrap();
        assert!(html.contains("Test Ontology"));
        assert!(html.contains("site-header"));
    }

    #[test]
    fn footer_renders() {
        let html = ComponentRenderer::footer().unwrap();
        assert!(html.contains("rontodoc"));
        assert!(html.contains("site-footer"));
    }

    #[test]
    fn hero_renders_with_comment() {
        let html = ComponentRenderer::hero("My Ontology", Some("A description")).unwrap();
        assert!(html.contains("My Ontology"));
        assert!(html.contains("A description"));
    }

    #[test]
    fn hero_renders_without_comment() {
        let html = ComponentRenderer::hero("My Ontology", None).unwrap();
        assert!(html.contains("My Ontology"));
        // The paragraph element with hero-description class should not be rendered
        assert!(!html.contains("<p class=\"hero-description\">"));
    }

    #[test]
    fn metadata_card_renders_full() {
        let html = ComponentRenderer::metadata_card(
            "https://example.org/onto",
            Some("2.0.0"),
            Some("A test ontology"),
        )
        .unwrap();
        assert!(html.contains("https://example.org/onto"));
        assert!(html.contains("2.0.0"));
        assert!(html.contains("A test ontology"));
    }

    #[test]
    fn metadata_card_renders_minimal() {
        let html =
            ComponentRenderer::metadata_card("https://example.org/onto", None, None).unwrap();
        assert!(html.contains("https://example.org/onto"));
        // Version and comment sections should not appear
        assert!(!html.contains("Version"));
    }

    #[test]
    fn styleguide_renders() {
        let data = SampleData::default();
        let html = ComponentRenderer::styleguide(&data).unwrap();
        assert!(html.contains("Style Guide"));
        assert!(html.contains("Header"));
        assert!(html.contains("Footer"));
        assert!(html.contains("Hero"));
        assert!(html.contains("Metadata Card"));
    }

    // Snapshot tests using insta
    mod snapshots {
        use super::*;

        #[test]
        fn snapshot_header() {
            let html = ComponentRenderer::header("Test Ontology").unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_footer() {
            let html = ComponentRenderer::footer().unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_hero_with_comment() {
            let html =
                ComponentRenderer::hero("Test Ontology", Some("A test ontology description."))
                    .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_hero_minimal() {
            let html = ComponentRenderer::hero("Test Ontology", None).unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_metadata_card_full() {
            let html = ComponentRenderer::metadata_card(
                "https://example.org/ontology/test",
                Some("1.0.0"),
                Some("A comprehensive test ontology."),
            )
            .unwrap();
            insta::assert_snapshot!(html);
        }

        #[test]
        fn snapshot_metadata_card_minimal() {
            let html =
                ComponentRenderer::metadata_card("https://example.org/ontology/test", None, None)
                    .unwrap();
            insta::assert_snapshot!(html);
        }
    }
}
