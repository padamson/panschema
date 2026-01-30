//! OWL/Turtle Writer
//!
//! Writes LinkML IR to OWL ontologies in Turtle format.

use std::fs;
use std::path::Path;

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;

/// Writer for OWL ontologies in Turtle (.ttl) format
pub struct OwlWriter;

impl OwlWriter {
    /// Create a new OWL writer
    pub fn new() -> Self {
        Self
    }

    /// Generate Turtle content from a SchemaDefinition
    pub fn generate_turtle(schema: &SchemaDefinition) -> String {
        let mut output = String::new();

        // Write prefixes
        output.push_str(&Self::generate_prefixes(schema));
        output.push('\n');

        // Write ontology metadata
        output.push_str(&Self::generate_ontology_metadata(schema));
        output.push('\n');

        // Write classes
        output.push_str(&Self::generate_classes(schema));

        // Write properties (slots)
        output.push_str(&Self::generate_properties(schema));

        // Write individuals
        output.push_str(&Self::generate_individuals(schema));

        output
    }

    fn generate_prefixes(schema: &SchemaDefinition) -> String {
        let mut output = String::new();

        // Standard prefixes
        output.push_str("@prefix owl: <http://www.w3.org/2002/07/owl#> .\n");
        output.push_str("@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .\n");
        output.push_str("@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n");
        output.push_str("@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n");
        output.push_str("@prefix dcterms: <http://purl.org/dc/terms/> .\n");

        // Ontology prefix (use schema id or generate one)
        if let Some(ref id) = schema.id {
            output.push_str(&format!("@prefix : <{}#> .\n", id));
        }

        // Custom prefixes from schema
        for (prefix, iri) in &schema.prefixes {
            output.push_str(&format!("@prefix {}: <{}> .\n", prefix, iri));
        }

        output
    }

    fn generate_ontology_metadata(schema: &SchemaDefinition) -> String {
        let mut output = String::new();
        output.push_str("# Ontology metadata\n");

        // Ontology IRI
        let ontology_iri = schema
            .id
            .as_deref()
            .unwrap_or("http://example.org/ontology");
        output.push_str(&format!("<{}> a owl:Ontology", ontology_iri));

        // rdfs:label from title
        if let Some(ref title) = schema.title {
            output.push_str(&format!(
                " ;\n    rdfs:label \"{}\"",
                Self::escape_string(title)
            ));
        }

        // rdfs:comment from description
        if let Some(ref description) = schema.description {
            output.push_str(&format!(
                " ;\n    rdfs:comment \"{}\"",
                Self::escape_string(description)
            ));
        }

        // owl:versionInfo from version
        if let Some(ref version) = schema.version {
            output.push_str(&format!(" ;\n    owl:versionInfo \"{}\"", version));

            // owl:versionIRI - pattern: {id}/{version}
            if let Some(ref id) = schema.id {
                output.push_str(&format!(" ;\n    owl:versionIRI <{}/{}>", id, version));
            }
        }

        // dcterms:license
        if let Some(ref license) = schema.license {
            output.push_str(&format!(" ;\n    dcterms:license <{}>", license));
        }

        // dcterms:creator from contributors
        for contributor in &schema.contributors {
            output.push_str(&format!(
                " ;\n    dcterms:creator \"{}\"",
                Self::escape_string(&contributor.name)
            ));
        }

        // dcterms:created
        if let Some(ref created) = schema.created {
            output.push_str(&format!(" ;\n    dcterms:created \"{}\"", created));
        }

        // dcterms:modified
        if let Some(ref modified) = schema.modified {
            output.push_str(&format!(" ;\n    dcterms:modified \"{}\"", modified));
        }

        output.push_str(" .\n");
        output
    }

    fn generate_classes(schema: &SchemaDefinition) -> String {
        if schema.classes.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        output.push_str("# Classes\n");

        for (name, class_def) in &schema.classes {
            // Get the class IRI
            let class_iri = class_def
                .class_uri
                .clone()
                .unwrap_or_else(|| format!(":{}", name));

            output.push_str(&format!("{} a owl:Class", Self::format_iri(&class_iri)));

            // rdfs:subClassOf from is_a
            if let Some(ref parent) = class_def.is_a {
                // Look up parent's IRI
                let parent_iri = schema
                    .classes
                    .get(parent)
                    .and_then(|c| c.class_uri.clone())
                    .unwrap_or_else(|| format!(":{}", parent));
                output.push_str(&format!(
                    " ;\n    rdfs:subClassOf {}",
                    Self::format_iri(&parent_iri)
                ));
            }

            // rdfs:label from name or annotation
            let label = class_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| name.to_string());
            output.push_str(&format!(
                " ;\n    rdfs:label \"{}\"",
                Self::escape_string(&label)
            ));

            // rdfs:comment from description
            if let Some(ref description) = class_def.description {
                output.push_str(&format!(
                    " ;\n    rdfs:comment \"{}\"",
                    Self::escape_string(description)
                ));
            }

            output.push_str(" .\n\n");
        }

        output
    }

    fn generate_properties(schema: &SchemaDefinition) -> String {
        if schema.slots.is_empty() {
            return String::new();
        }

        let mut output = String::new();

        // Separate object and datatype properties
        let mut object_props = Vec::new();
        let mut datatype_props = Vec::new();

        for (name, slot_def) in &schema.slots {
            let prop_type = slot_def
                .annotations
                .get("panschema:owl_property_type")
                .map(|s| s.as_str())
                .unwrap_or_else(|| {
                    // Infer from range - if range is a class, it's ObjectProperty
                    if let Some(ref range) = slot_def.range {
                        if schema.classes.contains_key(range) {
                            "ObjectProperty"
                        } else {
                            "DatatypeProperty"
                        }
                    } else {
                        "DatatypeProperty"
                    }
                });

            if prop_type == "ObjectProperty" {
                object_props.push((name, slot_def));
            } else {
                datatype_props.push((name, slot_def));
            }
        }

        // Write object properties
        if !object_props.is_empty() {
            output.push_str("# Object Properties\n");
            for (name, slot_def) in object_props {
                output.push_str(&Self::generate_property(schema, name, slot_def, true));
            }
        }

        // Write datatype properties
        if !datatype_props.is_empty() {
            output.push_str("# Datatype Properties\n");
            for (name, slot_def) in datatype_props {
                output.push_str(&Self::generate_property(schema, name, slot_def, false));
            }
        }

        output
    }

    fn generate_property(
        schema: &SchemaDefinition,
        name: &str,
        slot_def: &crate::linkml::SlotDefinition,
        is_object_property: bool,
    ) -> String {
        let mut output = String::new();

        // Get the property IRI
        let prop_iri = slot_def
            .slot_uri
            .clone()
            .unwrap_or_else(|| format!(":{}", name));

        let prop_type = if is_object_property {
            "owl:ObjectProperty"
        } else {
            "owl:DatatypeProperty"
        };

        output.push_str(&format!("{} a {}", Self::format_iri(&prop_iri), prop_type));

        // rdfs:label from name or annotation
        let label = slot_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| name.to_string());
        output.push_str(&format!(
            " ;\n    rdfs:label \"{}\"",
            Self::escape_string(&label)
        ));

        // rdfs:comment from description
        if let Some(ref description) = slot_def.description {
            output.push_str(&format!(
                " ;\n    rdfs:comment \"{}\"",
                Self::escape_string(description)
            ));
        }

        // rdfs:domain
        if let Some(ref domain) = slot_def.domain {
            let domain_iri = schema
                .classes
                .get(domain)
                .and_then(|c| c.class_uri.clone())
                .unwrap_or_else(|| format!(":{}", domain));
            output.push_str(&format!(
                " ;\n    rdfs:domain {}",
                Self::format_iri(&domain_iri)
            ));
        }

        // rdfs:range
        if let Some(ref range) = slot_def.range {
            let range_iri = if is_object_property {
                // For object properties, range is a class
                schema
                    .classes
                    .get(range)
                    .and_then(|c| c.class_uri.clone())
                    .unwrap_or_else(|| format!(":{}", range))
            } else {
                // For datatype properties, map to XSD
                Self::map_linkml_to_xsd(range)
            };
            output.push_str(&format!(
                " ;\n    rdfs:range {}",
                Self::format_iri(&range_iri)
            ));
        }

        // owl:inverseOf
        if let Some(ref inverse) = slot_def.inverse {
            let inverse_iri = schema
                .slots
                .get(inverse)
                .and_then(|s| s.slot_uri.clone())
                .unwrap_or_else(|| format!(":{}", inverse));
            output.push_str(&format!(
                " ;\n    owl:inverseOf {}",
                Self::format_iri(&inverse_iri)
            ));
        }

        output.push_str(" .\n\n");
        output
    }

    fn generate_individuals(schema: &SchemaDefinition) -> String {
        // Check if we have individuals stored in annotations
        let individuals = match schema.annotations.get("panschema:individuals") {
            Some(list) => list.split(',').collect::<Vec<_>>(),
            None => return String::new(),
        };

        if individuals.is_empty() {
            return String::new();
        }

        let mut output = String::new();
        output.push_str("# Individuals\n");

        for ind_id in individuals {
            let ind_id = ind_id.trim();
            if ind_id.is_empty() {
                continue;
            }

            // Get individual IRI
            let iri_key = format!("panschema:individual:{}:_iri", ind_id);
            let ind_iri = schema
                .annotations
                .get(&iri_key)
                .cloned()
                .unwrap_or_else(|| format!(":{}", ind_id));

            // Get types
            let types_key = format!("panschema:individual:{}", ind_id);
            let types = schema
                .annotations
                .get(&types_key)
                .map(|s| s.split(',').collect::<Vec<_>>())
                .unwrap_or_default();

            // Start individual declaration
            output.push_str(&format!("<{}> a owl:NamedIndividual", ind_iri));

            // Add types
            for type_iri in &types {
                let type_iri = type_iri.trim();
                if !type_iri.is_empty() {
                    output.push_str(&format!(" , <{}>", type_iri));
                }
            }

            // Get label
            let label_key = format!("panschema:individual:{}:_label", ind_id);
            if let Some(label) = schema.annotations.get(&label_key) {
                output.push_str(&format!(
                    " ;\n    rdfs:label \"{}\"",
                    Self::escape_string(label)
                ));
            }

            // Get property values (scan all annotations for this individual)
            let prop_prefix = format!("panschema:individual:{}:", ind_id);
            for (key, value) in &schema.annotations {
                if key.starts_with(&prop_prefix) {
                    let prop_part = &key[prop_prefix.len()..];
                    // Skip metadata keys
                    if prop_part.starts_with('_') || prop_part.contains(":_") {
                        continue;
                    }
                    // This is a property value
                    let prop_iri = format!(":{}", prop_part);
                    // Try to determine if it's a literal or IRI
                    if value.starts_with("http://") || value.starts_with("https://") {
                        output.push_str(&format!(" ;\n    {} <{}>", prop_iri, value));
                    } else if let Ok(num) = value.parse::<i64>() {
                        output.push_str(&format!(" ;\n    {} {}", prop_iri, num));
                    } else {
                        output.push_str(&format!(
                            " ;\n    {} \"{}\"",
                            prop_iri,
                            Self::escape_string(value)
                        ));
                    }
                }
            }

            output.push_str(" .\n\n");
        }

        output
    }

    /// Map LinkML built-in types to XSD datatypes
    fn map_linkml_to_xsd(linkml_type: &str) -> String {
        match linkml_type {
            "string" => "xsd:string".to_string(),
            "integer" => "xsd:integer".to_string(),
            "float" => "xsd:float".to_string(),
            "double" => "xsd:double".to_string(),
            "boolean" => "xsd:boolean".to_string(),
            "date" => "xsd:date".to_string(),
            "datetime" => "xsd:dateTime".to_string(),
            "time" => "xsd:time".to_string(),
            "uri" => "xsd:anyURI".to_string(),
            _ => format!("xsd:{}", linkml_type),
        }
    }

    /// Escape special characters in strings for Turtle
    fn escape_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    }

    /// Format an IRI for Turtle output
    /// Wraps full IRIs in angle brackets, leaves prefixed names as-is
    fn format_iri(iri: &str) -> String {
        if iri.starts_with("http://") || iri.starts_with("https://") || iri.starts_with("urn:") {
            format!("<{}>", iri)
        } else {
            iri.to_string()
        }
    }
}

impl Default for OwlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for OwlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let turtle = Self::generate_turtle(schema);
        fs::write(output, turtle).map_err(IoError::Io)
    }

    fn format_id(&self) -> &str {
        "ttl"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Reader;
    use crate::linkml::{ClassDefinition, SlotDefinition};

    // ========== Basic Writer Tests ==========

    #[test]
    fn owl_writer_format_id_is_ttl() {
        let writer = OwlWriter::new();
        assert_eq!(writer.format_id(), "ttl");
    }

    #[test]
    fn owl_writer_has_default() {
        let writer = OwlWriter;
        assert_eq!(writer.format_id(), "ttl");
    }

    // ========== Prefix Generation Tests ==========

    #[test]
    fn generates_standard_prefixes() {
        let schema = SchemaDefinition::new("test");
        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("@prefix owl:"));
        assert!(turtle.contains("@prefix rdf:"));
        assert!(turtle.contains("@prefix rdfs:"));
        assert!(turtle.contains("@prefix xsd:"));
    }

    #[test]
    fn generates_dcterms_prefix() {
        let schema = SchemaDefinition::new("test");
        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("@prefix dcterms: <http://purl.org/dc/terms/>"));
    }

    #[test]
    fn generates_ontology_prefix_from_id() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/myontology".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("@prefix : <http://example.org/myontology#>"));
    }

    #[test]
    fn generates_custom_prefixes() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .prefixes
            .insert("ex".to_string(), "http://example.org/".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("@prefix ex: <http://example.org/>"));
    }

    // ========== Ontology Metadata Tests ==========

    #[test]
    fn generates_ontology_declaration() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("<http://example.org/test> a owl:Ontology"));
    }

    #[test]
    fn generates_ontology_label_from_title() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.title = Some("Test Ontology".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:label \"Test Ontology\""));
    }

    #[test]
    fn generates_ontology_comment_from_description() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.description = Some("A test ontology.".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:comment \"A test ontology.\""));
    }

    #[test]
    fn generates_ontology_version() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.version = Some("1.0.0".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("owl:versionInfo \"1.0.0\""));
    }

    #[test]
    fn generates_version_iri() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.version = Some("1.0.0".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("owl:versionIRI <http://example.org/test/1.0.0>"));
    }

    #[test]
    fn generates_dcterms_license() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.license = Some("https://creativecommons.org/licenses/by/4.0/".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("dcterms:license <https://creativecommons.org/licenses/by/4.0/>"));
    }

    #[test]
    fn generates_dcterms_creator_from_contributors() {
        use crate::linkml::Contributor;

        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.contributors.push(Contributor::new("Jane Doe"));
        schema.contributors.push(Contributor::new("John Smith"));

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("dcterms:creator \"Jane Doe\""));
        assert!(turtle.contains("dcterms:creator \"John Smith\""));
    }

    #[test]
    fn generates_dcterms_created() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.created = Some("2025-01-15".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("dcterms:created \"2025-01-15\""));
    }

    #[test]
    fn generates_dcterms_modified() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.modified = Some("2026-01-29".to_string());

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("dcterms:modified \"2026-01-29\""));
    }

    // ========== Class Generation Tests ==========

    #[test]
    fn generates_class_declaration() {
        let mut schema = SchemaDefinition::new("test");
        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("<http://example.org/test#Animal> a owl:Class"));
    }

    #[test]
    fn generates_class_with_subclass() {
        let mut schema = SchemaDefinition::new("test");

        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut dog = ClassDefinition::new("Dog");
        dog.class_uri = Some("http://example.org/test#Dog".to_string());
        dog.is_a = Some("Animal".to_string());
        schema.classes.insert("Dog".to_string(), dog);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:subClassOf <http://example.org/test#Animal>"));
    }

    #[test]
    fn generates_class_label() {
        let mut schema = SchemaDefinition::new("test");
        let animal = ClassDefinition::new("Animal");
        schema.classes.insert("Animal".to_string(), animal);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:label \"Animal\""));
    }

    #[test]
    fn generates_class_label_from_annotation() {
        let mut schema = SchemaDefinition::new("test");
        let mut animal = ClassDefinition::new("Animal");
        animal
            .annotations
            .insert("panschema:label".to_string(), "Living Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:label \"Living Animal\""));
    }

    #[test]
    fn generates_class_description() {
        let mut schema = SchemaDefinition::new("test");
        let mut animal = ClassDefinition::new("Animal");
        animal.description = Some("A living creature.".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:comment \"A living creature.\""));
    }

    // ========== Property Generation Tests ==========

    #[test]
    fn generates_object_property() {
        let mut schema = SchemaDefinition::new("test");

        let mut person = ClassDefinition::new("Person");
        person.class_uri = Some("http://example.org/test#Person".to_string());
        schema.classes.insert("Person".to_string(), person);

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.slot_uri = Some("http://example.org/test#hasOwner".to_string());
        has_owner.range = Some("Person".to_string());
        has_owner.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("<http://example.org/test#hasOwner> a owl:ObjectProperty"));
    }

    #[test]
    fn generates_datatype_property() {
        let mut schema = SchemaDefinition::new("test");

        let mut has_age = SlotDefinition::new("hasAge");
        has_age.slot_uri = Some("http://example.org/test#hasAge".to_string());
        has_age.range = Some("integer".to_string());
        has_age.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "DatatypeProperty".to_string(),
        );
        schema.slots.insert("hasAge".to_string(), has_age);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("<http://example.org/test#hasAge> a owl:DatatypeProperty"));
    }

    #[test]
    fn generates_property_domain() {
        let mut schema = SchemaDefinition::new("test");

        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut has_age = SlotDefinition::new("hasAge");
        has_age.slot_uri = Some("http://example.org/test#hasAge".to_string());
        has_age.domain = Some("Animal".to_string());
        has_age.range = Some("integer".to_string());
        schema.slots.insert("hasAge".to_string(), has_age);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:domain <http://example.org/test#Animal>"));
    }

    #[test]
    fn generates_property_range_for_datatype() {
        let mut schema = SchemaDefinition::new("test");

        let mut has_age = SlotDefinition::new("hasAge");
        has_age.range = Some("integer".to_string());
        schema.slots.insert("hasAge".to_string(), has_age);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:range xsd:integer"));
    }

    #[test]
    fn generates_property_range_for_class() {
        let mut schema = SchemaDefinition::new("test");

        let mut person = ClassDefinition::new("Person");
        person.class_uri = Some("http://example.org/test#Person".to_string());
        schema.classes.insert("Person".to_string(), person);

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.range = Some("Person".to_string());
        has_owner.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:range <http://example.org/test#Person>"));
    }

    #[test]
    fn generates_inverse_of() {
        let mut schema = SchemaDefinition::new("test");

        let mut owns = SlotDefinition::new("owns");
        owns.slot_uri = Some("http://example.org/test#owns".to_string());
        owns.inverse = Some("hasOwner".to_string());
        owns.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("owns".to_string(), owns);

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.slot_uri = Some("http://example.org/test#hasOwner".to_string());
        has_owner.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("owl:inverseOf <http://example.org/test#hasOwner>"));
    }

    #[test]
    fn generates_property_label() {
        let mut schema = SchemaDefinition::new("test");

        let mut has_age = SlotDefinition::new("hasAge");
        has_age
            .annotations
            .insert("panschema:label".to_string(), "has age".to_string());
        schema.slots.insert("hasAge".to_string(), has_age);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:label \"has age\""));
    }

    #[test]
    fn generates_property_comment() {
        let mut schema = SchemaDefinition::new("test");

        let mut has_age = SlotDefinition::new("hasAge");
        has_age.description = Some("The age in years.".to_string());
        schema.slots.insert("hasAge".to_string(), has_age);

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:comment \"The age in years.\""));
    }

    // ========== Individual Generation Tests ==========

    #[test]
    fn generates_individual_declaration() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .annotations
            .insert("panschema:individuals".to_string(), "fido".to_string());
        schema.annotations.insert(
            "panschema:individual:fido:_iri".to_string(),
            "http://example.org/test#fido".to_string(),
        );
        schema.annotations.insert(
            "panschema:individual:fido".to_string(),
            "http://example.org/test#Dog".to_string(),
        );

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("<http://example.org/test#fido> a owl:NamedIndividual"));
        assert!(turtle.contains("<http://example.org/test#Dog>"));
    }

    #[test]
    fn generates_individual_label() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .annotations
            .insert("panschema:individuals".to_string(), "fido".to_string());
        schema.annotations.insert(
            "panschema:individual:fido:_iri".to_string(),
            "http://example.org/test#fido".to_string(),
        );
        schema.annotations.insert(
            "panschema:individual:fido:_label".to_string(),
            "Fido".to_string(),
        );

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains("rdfs:label \"Fido\""));
    }

    #[test]
    fn generates_individual_property_values() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .annotations
            .insert("panschema:individuals".to_string(), "fido".to_string());
        schema.annotations.insert(
            "panschema:individual:fido:_iri".to_string(),
            "http://example.org/test#fido".to_string(),
        );
        schema.annotations.insert(
            "panschema:individual:fido:hasAge".to_string(),
            "5".to_string(),
        );
        schema.annotations.insert(
            "panschema:individual:fido:hasName".to_string(),
            "Fido".to_string(),
        );

        let turtle = OwlWriter::generate_turtle(&schema);

        assert!(turtle.contains(":hasAge 5"));
        assert!(turtle.contains(":hasName \"Fido\""));
    }

    // ========== XSD Type Mapping Tests ==========

    #[test]
    fn maps_linkml_types_to_xsd() {
        assert_eq!(OwlWriter::map_linkml_to_xsd("string"), "xsd:string");
        assert_eq!(OwlWriter::map_linkml_to_xsd("integer"), "xsd:integer");
        assert_eq!(OwlWriter::map_linkml_to_xsd("float"), "xsd:float");
        assert_eq!(OwlWriter::map_linkml_to_xsd("boolean"), "xsd:boolean");
        assert_eq!(OwlWriter::map_linkml_to_xsd("date"), "xsd:date");
        assert_eq!(OwlWriter::map_linkml_to_xsd("datetime"), "xsd:dateTime");
        assert_eq!(OwlWriter::map_linkml_to_xsd("uri"), "xsd:anyURI");
    }

    // ========== String Escaping Tests ==========

    #[test]
    fn escapes_special_characters() {
        assert_eq!(OwlWriter::escape_string("hello"), "hello");
        assert_eq!(OwlWriter::escape_string("hello\"world"), "hello\\\"world");
        assert_eq!(OwlWriter::escape_string("line1\nline2"), "line1\\nline2");
        assert_eq!(OwlWriter::escape_string("path\\to"), "path\\\\to");
    }

    // ========== Round-trip Tests ==========

    #[test]
    fn roundtrip_with_owl_reader() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;
        use tempfile::TempDir;

        // Read reference ontology
        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        // Write to temporary file
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write output.ttl");

        // Read back the written file
        let schema2 = reader
            .read(&output_path)
            .expect("Failed to read written output.ttl");

        // Verify key data is preserved
        assert_eq!(schema.name, schema2.name);
        assert_eq!(schema.title, schema2.title);
        assert_eq!(schema.version, schema2.version);
        assert_eq!(schema.classes.len(), schema2.classes.len());
        assert_eq!(schema.slots.len(), schema2.slots.len());
    }

    #[test]
    fn roundtrip_preserves_class_hierarchy() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;
        use tempfile::TempDir;

        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let schema2 = reader.read(&output_path).expect("Failed to read back");

        // Check Dog's parent is preserved
        let dog = schema.classes.get("Dog").unwrap();
        let dog2 = schema2.classes.get("Dog").unwrap();
        assert_eq!(dog.is_a, dog2.is_a);
    }

    #[test]
    fn roundtrip_preserves_inverse_relationships() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;
        use tempfile::TempDir;

        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let schema2 = reader.read(&output_path).expect("Failed to read back");

        // Check owns has inverse relationship
        let owns = schema.slots.get("owns").unwrap();
        let owns2 = schema2.slots.get("owns").unwrap();
        assert_eq!(owns.inverse, owns2.inverse);
    }
}
