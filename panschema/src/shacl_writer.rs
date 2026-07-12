//! SHACL Writer
//!
//! Writes a SHACL shapes graph (Turtle) from the LinkML IR: one
//! `sh:NodeShape` per class with `sh:property` shapes mirroring each slot's
//! value constraints. A separate artifact from the OWL/TTL output — a
//! validation shapes file a SHACL engine consumes — built from the same
//! class/property IRIs (see `rdf_serializers::build_shacl_graph`).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::prefix::{Prefix, PrefixMapPair};
use sophia::api::serializer::TripleSerializer;
use sophia::iri::Iri;
use sophia::turtle::serializer::turtle::{TurtleConfig, TurtleSerializer};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::rdf_serializers::build_shacl_graph;

/// Writer for a SHACL shapes graph in Turtle (.ttl) format.
pub struct ShaclWriter;

impl ShaclWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ShaclWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for ShaclWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_shacl_graph(schema)?;

        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        // Same prefix-aware Turtle config the OWL writer uses, plus a `sh:`
        // declaration so the shapes graph reads in compact form.
        let config = TurtleConfig::new()
            .with_pretty(true)
            .with_own_prefix_map(build_prefix_map(schema));
        let mut serializer = TurtleSerializer::new_with_config(writer, config);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("Turtle serialization failed: {e}")))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "shacl"
    }
}

/// The schema's `prefixes:` block plus the `sh:` (SHACL) and `xsd:`
/// declarations the shapes graph itself uses, as a sophia prefix map.
/// Entries that fail sophia's prefix/IRI validation are dropped with a
/// `tracing::warn!` (they can't appear in the output anyway).
fn build_prefix_map(schema: &SchemaDefinition) -> Vec<PrefixMapPair> {
    let builtins = [
        ("sh", "http://www.w3.org/ns/shacl#"),
        ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ];
    schema
        .prefixes
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .chain(builtins)
        .filter_map(|(name, base)| {
            let prefix = Prefix::new(name.to_string().into_boxed_str())
                .map_err(|e| tracing::warn!(prefix = name, error = %e, "skipping invalid prefix"))
                .ok()?;
            let iri = Iri::new(base.to_string().into_boxed_str())
                .map_err(
                    |e| tracing::warn!(prefix = name, base, error = %e, "skipping bad base IRI"),
                )
                .ok()?;
            Some((prefix, iri))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Writer;
    use crate::linkml::{ClassDefinition, SlotDefinition};
    use std::fs;
    use tempfile::TempDir;

    /// Render `schema` through `ShaclWriter`, load the output into an
    /// independent `oxigraph` store, and return it — the same real-triple-
    /// store oracle the RDF writers use, applied to the shapes graph.
    fn render_to_store(schema: &SchemaDefinition) -> oxigraph::store::Store {
        let dir = TempDir::new().expect("temp dir");
        let out = dir.path().join("shapes.ttl");
        ShaclWriter::new().write(schema, &out).expect("write shacl");
        let ttl = fs::read_to_string(&out).expect("read shapes");
        let store = oxigraph::store::Store::new().expect("store");
        store
            .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
            .unwrap_or_else(|e| panic!("oxigraph rejected generated SHACL: {e}\n\n{ttl}"));
        store
    }

    fn ask(store: &oxigraph::store::Store, query: &str) -> bool {
        use oxigraph::sparql::{QueryResults, SparqlEvaluator};
        match SparqlEvaluator::new()
            .parse_query(query)
            .unwrap_or_else(|e| panic!("bad SPARQL: {e}\n\n{query}"))
            .on_store(store)
            .execute()
            .expect("query")
        {
            QueryResults::Boolean(b) => b,
            _ => panic!("expected ASK result"),
        }
    }

    const SH: &str = "http://www.w3.org/ns/shacl#";
    const EX: &str = "http://example.org/test";

    fn schema_with_constrained_class() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());

        let mut provider = ClassDefinition::new("Provider");
        provider.class_uri = Some(format!("{EX}#Provider"));
        schema.classes.insert("Provider".to_string(), provider);

        let mut deployment = ClassDefinition::new("Deployment");
        deployment.class_uri = Some(format!("{EX}#Deployment"));
        // A required, patterned string.
        let mut code = SlotDefinition::new("code");
        code.range = Some("string".to_string());
        code.required = true;
        code.pattern = Some("^[A-Z]+$".to_string());
        deployment.attributes.insert("code".to_string(), code);
        // A value-bounded float.
        let mut ratio = SlotDefinition::new("ratio");
        ratio.range = Some("float".to_string());
        ratio.minimum_value = Some(0.0);
        ratio.maximum_value = Some(1.0);
        deployment.attributes.insert("ratio".to_string(), ratio);
        // A single-valued class reference.
        let mut on_provider = SlotDefinition::new("on_provider");
        on_provider.range = Some("Provider".to_string());
        deployment
            .attributes
            .insert("on_provider".to_string(), on_provider);
        schema.classes.insert("Deployment".to_string(), deployment);
        schema
    }

    #[test]
    fn shacl_writer_format_id_is_shacl() {
        assert_eq!(ShaclWriter::new().format_id(), "shacl");
    }

    #[test]
    fn output_uses_the_sh_prefix_in_compact_form() {
        // The prefix map must actually reach the serializer — without it the
        // shapes graph emits verbose full `<...shacl#NodeShape>` IRIs instead
        // of compact `sh:NodeShape`. Proves `build_prefix_map`'s output is
        // wired in and non-empty.
        let dir = TempDir::new().unwrap();
        let out = dir.path().join("shapes.ttl");
        ShaclWriter::new()
            .write(&schema_with_constrained_class(), &out)
            .unwrap();
        let ttl = fs::read_to_string(&out).unwrap();
        assert!(
            ttl.contains("sh:NodeShape"),
            "expected compact `sh:` form (so the sh: prefix is declared); got:\n{ttl}"
        );
    }

    #[test]
    fn every_class_gets_a_node_shape_targeting_its_iri() {
        let store = render_to_store(&schema_with_constrained_class());
        for name in ["Provider", "Deployment"] {
            assert!(
                ask(
                    &store,
                    &format!(
                        "ASK {{ <{EX}#{name}Shape> a <{SH}NodeShape> ; <{SH}targetClass> <{EX}#{name}> }}"
                    )
                ),
                "expected a NodeShape targeting {name}"
            );
        }
    }

    #[test]
    fn base_slot_constraints_project_to_property_shapes() {
        let store = render_to_store(&schema_with_constrained_class());

        // required → sh:minCount 1, plus sh:pattern, on the `code` property shape.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <{EX}#DeploymentShape> <{SH}property> ?p . \
                     ?p <{SH}path> <{EX}#code> ; <{SH}minCount> 1 ; <{SH}pattern> \"^[A-Z]+$\" }}"
                )
            ),
            "required+pattern must project to sh:minCount + sh:pattern"
        );
        // value bounds → sh:minInclusive / sh:maxInclusive on `ratio`.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#ratio> ; <{SH}minInclusive> ?lo ; <{SH}maxInclusive> ?hi }}"
                )
            ),
            "value bounds must project to sh:minInclusive/maxInclusive"
        );
        // class-valued range → sh:class the target class IRI.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#on_provider> ; <{SH}class> <{EX}#Provider> }}"
                )
            ),
            "a class-range slot must project to sh:class"
        );
    }

    /// A class with the canonical conditional rule: when `status` =
    /// `actual`, `region` is required.
    fn schema_with_conditional_rule() -> SchemaDefinition {
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());

        let mut deployment = ClassDefinition::new("Deployment");
        deployment.class_uri = Some(format!("{EX}#Deployment"));
        let mut status = SlotDefinition::new("status");
        status.range = Some("string".to_string());
        deployment.attributes.insert("status".to_string(), status);
        let mut region = SlotDefinition::new("region");
        region.range = Some("string".to_string());
        deployment.attributes.insert("region".to_string(), region);

        deployment.rules.push(ClassRule {
            title: Some("actual-needs-region".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "status".to_string(),
                    SlotCondition {
                        equals_string: Some("actual".to_string()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "region".to_string(),
                    SlotCondition {
                        required: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Deployment".to_string(), deployment);
        schema
    }

    #[test]
    fn a_rule_projects_pre_and_post_condition_shapes() {
        let store = render_to_store(&schema_with_conditional_rule());
        // Precondition matcher: status must have value "actual".
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <{EX}#DeploymentShape/rule0/pre> <{SH}property> ?p . \
                     ?p <{SH}path> <{EX}#status> ; <{SH}hasValue> \"actual\" }}"
                )
            ),
            "precondition should project equals_string → sh:hasValue"
        );
        // Postcondition: region required → sh:minCount 1.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <{EX}#DeploymentShape/rule0/post> <{SH}property> ?p . \
                     ?p <{SH}path> <{EX}#region> ; <{SH}minCount> 1 }}"
                )
            ),
            "postcondition should project required → sh:minCount 1"
        );
    }

    #[test]
    fn a_rule_wires_the_conditional_as_sh_or_not_pre_post() {
        let store = render_to_store(&schema_with_conditional_rule());
        const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
        // The class shape carries sh:or of a 2-element list: [sh:not pre], post.
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ \
                     <{EX}#Deployment> ^<{SH}targetClass> ?shape . \
                     ?shape <{SH}or> ?l0 . \
                     ?l0 <{RDF}first> ?notpre ; <{RDF}rest> ?l1 . \
                     ?notpre <{SH}not> <{EX}#DeploymentShape/rule0/pre> . \
                     ?l1 <{RDF}first> <{EX}#DeploymentShape/rule0/post> ; <{RDF}rest> <{RDF}nil> \
                     }}"
                )
            ),
            "the rule must wire sh:or ( [sh:not pre] post ) as an RDF list"
        );
    }

    #[test]
    fn cardinality_projects_to_min_and_max_count() {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some(format!("{EX}#Thing"));
        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.minimum_cardinality = Some(1);
        tags.maximum_cardinality = Some(5);
        thing.attributes.insert("tags".to_string(), tags);
        schema.classes.insert("Thing".to_string(), thing);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#tags> ; <{SH}minCount> 1 ; <{SH}maxCount> 5 }}"
                )
            ),
            "cardinality must project to sh:minCount/sh:maxCount"
        );
    }

    #[test]
    fn required_and_minimum_cardinality_reconcile_to_one_min_count() {
        // `required` and `minimum_cardinality` are two spellings of the same
        // lower bound. Emitting both a `sh:minCount 1` (from `required`) and a
        // `sh:minCount 2` (from the cardinality) yields a self-contradictory
        // shape. They must reconcile to a single count, with the explicit
        // cardinality winning — matching the HTML/effective-cardinality view.
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some(format!("{EX}#Thing"));
        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.required = true;
        tags.minimum_cardinality = Some(2);
        thing.attributes.insert("tags".to_string(), tags);
        schema.classes.insert("Thing".to_string(), thing);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                &format!("ASK {{ ?p <{SH}path> <{EX}#tags> ; <{SH}minCount> 2 }}")
            ),
            "the effective lower bound (2) must be the emitted sh:minCount"
        );
        assert!(
            !ask(
                &store,
                &format!("ASK {{ ?p <{SH}path> <{EX}#tags> ; <{SH}minCount> 1 }}")
            ),
            "the `required`-derived sh:minCount 1 must not coexist with the cardinality's"
        );
    }

    #[test]
    fn a_slot_with_no_lower_bound_emits_no_min_count() {
        // An optional slot (neither `required` nor `minimum_cardinality`) has
        // an effective lower bound of zero, which is the absence of a
        // constraint — it must emit no `sh:minCount` at all, not `sh:minCount 0`.
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some(format!("{EX}#Thing"));
        let mut note = SlotDefinition::new("note");
        note.range = Some("string".to_string());
        thing.attributes.insert("note".to_string(), note);
        schema.classes.insert("Thing".to_string(), thing);

        let store = render_to_store(&schema);
        assert!(
            !ask(
                &store,
                &format!("ASK {{ ?p <{SH}path> <{EX}#note> ; <{SH}minCount> ?n }}")
            ),
            "a slot with no lower bound must emit no sh:minCount"
        );
    }

    #[test]
    fn a_rule_with_an_empty_condition_side_emits_no_conditional() {
        // A rule whose precondition (or postcondition) carries no
        // slot_conditions has no conditional to express — it must emit no
        // `sh:or` at all, not a degenerate one pointing at an empty shape.
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Deployment");
        c.class_uri = Some(format!("{EX}#Deployment"));
        let mut region = SlotDefinition::new("region");
        region.range = Some("string".to_string());
        c.attributes.insert("region".to_string(), region);
        c.rules.push(ClassRule {
            title: Some("empty-pre".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: Default::default(), // empty side
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "region".to_string(),
                    SlotCondition {
                        required: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Deployment".to_string(), c);

        let store = render_to_store(&schema);
        assert!(
            !ask(&store, &format!("ASK {{ ?s <{SH}or> ?o }}")),
            "an empty-conditioned rule must emit no sh:or"
        );
    }

    #[test]
    fn a_rule_condition_uses_the_slots_own_uri_for_sh_path() {
        // The precondition slot has a distinct `slot_uri`; its property
        // shape's `sh:path` must be that URI (resolved from the *right*
        // slot's definition), not the `{ontology}#{name}` fallback.
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Deployment");
        c.class_uri = Some(format!("{EX}#Deployment"));
        let mut status = SlotDefinition::new("status");
        status.range = Some("string".to_string());
        status.slot_uri = Some(format!("{EX}#statusProp"));
        c.attributes.insert("status".to_string(), status);
        let mut region = SlotDefinition::new("region");
        region.range = Some("string".to_string());
        c.attributes.insert("region".to_string(), region);
        c.rules.push(ClassRule {
            title: Some("r".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "status".to_string(),
                    SlotCondition {
                        equals_string: Some("actual".to_string()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "region".to_string(),
                    SlotCondition {
                        required: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Deployment".to_string(), c);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <{EX}#DeploymentShape/rule0/pre> <{SH}property> ?p . ?p <{SH}path> <{EX}#statusProp> }}"
                )
            ),
            "a rule condition must resolve the referenced slot's own slot_uri for sh:path"
        );
    }

    #[test]
    fn equals_number_on_an_integer_range_uses_an_integer_typed_hasvalue() {
        // `sh:hasValue` is RDF term equality (datatype-sensitive). An
        // integer-range slot's instance data is `"N"^^xsd:integer`, so the
        // hasValue literal must be xsd:integer too — a default xsd:double
        // literal could never match, silently inverting the rule. In SPARQL
        // the plain literal `1` is xsd:integer, so this pattern matches only
        // if the stored term is xsd:integer.
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Task");
        c.class_uri = Some(format!("{EX}#Task"));
        let mut priority = SlotDefinition::new("priority");
        priority.range = Some("integer".to_string());
        c.attributes.insert("priority".to_string(), priority);
        let mut region = SlotDefinition::new("region");
        region.range = Some("string".to_string());
        c.attributes.insert("region".to_string(), region);
        c.rules.push(ClassRule {
            title: Some("p1-needs-region".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "priority".to_string(),
                    SlotCondition {
                        equals_number: Some(1.0),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "region".to_string(),
                    SlotCondition {
                        required: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Task".to_string(), c);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                &format!("ASK {{ <{EX}#TaskShape/rule0/pre/priority> <{SH}hasValue> 1 }}")
            ),
            "equals_number on an integer range must emit an xsd:integer hasValue"
        );
    }

    #[test]
    fn a_postcondition_equals_number_is_typed_from_its_own_slots_range() {
        // The postcondition slot must resolve to *its own* definition — the
        // resolved range is what types the `equals_number` hasValue. `amount`
        // is integer, `label` is string; a lookup that matched any other slot
        // would type `amount`'s hasValue from `label` (xsd:double) and fail
        // the xsd:integer ASK below.
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Gate");
        c.class_uri = Some(format!("{EX}#Gate"));
        let mut amount = SlotDefinition::new("amount");
        amount.range = Some("integer".to_string());
        c.attributes.insert("amount".to_string(), amount);
        let mut label = SlotDefinition::new("label");
        label.range = Some("string".to_string());
        c.attributes.insert("label".to_string(), label);
        c.rules.push(ClassRule {
            title: Some("labelled-gates-set-amount".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "label".to_string(),
                    SlotCondition {
                        equals_string: Some("open".to_string()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "amount".to_string(),
                    SlotCondition {
                        equals_number: Some(5.0),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Gate".to_string(), c);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                &format!("ASK {{ <{EX}#GateShape/rule0/post/amount> <{SH}hasValue> 5 }}")
            ),
            "postcondition equals_number on an integer slot must emit an xsd:integer hasValue"
        );
    }

    #[test]
    fn a_rule_naming_a_missing_slot_is_skipped_not_emitted_with_a_phantom_iri() {
        // A condition referencing a slot the class doesn't have must NOT
        // emit a shape over a fabricated `{ontology}#{slot}` path (which
        // would reject all valid data). The rule is dropped and reported.
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        use crate::rdf_serializers::shacl_skipped_rules;
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Deployment");
        c.class_uri = Some(format!("{EX}#Deployment"));
        let mut status = SlotDefinition::new("status");
        status.range = Some("string".to_string());
        c.attributes.insert("status".to_string(), status);
        c.rules.push(ClassRule {
            title: Some("ghost-ref".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "status".to_string(),
                    SlotCondition {
                        equals_string: Some("actual".to_string()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: Some(RuleConditions {
                slot_conditions: [(
                    "ghost".to_string(),
                    SlotCondition {
                        required: true,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
        });
        schema.classes.insert("Deployment".to_string(), c);

        // No conditional shape emitted (no sh:or), and no phantom `#ghost`
        // path anywhere in the graph.
        let store = render_to_store(&schema);
        assert!(
            !ask(&store, &format!("ASK {{ ?s <{SH}or> ?o }}")),
            "a rule naming a missing slot must emit no conditional shape"
        );
        assert!(
            !ask(&store, &format!("ASK {{ ?p <{SH}path> <{EX}#ghost> }}")),
            "no shape may reference a fabricated property IRI for a missing slot"
        );
        // ...and it's reported, named by its title.
        let skipped = shacl_skipped_rules(&schema);
        assert_eq!(skipped.len(), 1, "got: {skipped:?}");
        assert_eq!(skipped[0].class, "Deployment");
        assert_eq!(skipped[0].rule, "ghost-ref");
    }

    #[test]
    fn a_one_sided_rule_is_skipped_and_reported() {
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition};
        use crate::rdf_serializers::shacl_skipped_rules;
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut c = ClassDefinition::new("Deployment");
        c.class_uri = Some(format!("{EX}#Deployment"));
        let mut status = SlotDefinition::new("status");
        status.range = Some("string".to_string());
        c.attributes.insert("status".to_string(), status);
        c.rules.push(ClassRule {
            title: None, // untitled → labeled by index
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: [(
                    "status".to_string(),
                    SlotCondition {
                        equals_string: Some("actual".to_string()),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            }),
            postconditions: None,
        });
        schema.classes.insert("Deployment".to_string(), c);

        let store = render_to_store(&schema);
        assert!(
            !ask(&store, &format!("ASK {{ ?s <{SH}or> ?o }}")),
            "a one-sided rule must emit no conditional shape"
        );
        let skipped = shacl_skipped_rules(&schema);
        assert_eq!(skipped.len(), 1, "got: {skipped:?}");
        assert_eq!(skipped[0].rule, "rule #0");
    }

    #[test]
    fn a_scalar_slot_projects_to_sh_datatype() {
        let store = render_to_store(&schema_with_constrained_class());
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ ?p <{SH}path> <{EX}#code> ; <{SH}datatype> <http://www.w3.org/2001/XMLSchema#string> }}"
                )
            ),
            "a plain scalar slot must carry sh:datatype"
        );
    }

    #[test]
    fn every_shacl_path_has_an_owl_property_declaration() {
        // The SHACL shapes and the OWL ontology must describe the same
        // vocabulary: every `sh:path` in the shapes graph must name a property
        // the OWL output declares. An attribute-style class exercises the case
        // that used to break this — SHACL resolves effective slots, but the
        // OWL writer walked only top-level `slots:`, so a `sh:path` could point
        // at a property the OWL graph never declared.
        use crate::owl_writer::OwlWriter;
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some(EX.to_string());
        let mut order = ClassDefinition::new("Order");
        order.class_uri = Some(format!("{EX}#Order"));
        let mut amount = SlotDefinition::new("amount");
        amount.range = Some("integer".to_string());
        order.attributes.insert("amount".to_string(), amount);
        let mut label = SlotDefinition::new("label");
        label.range = Some("string".to_string());
        order.attributes.insert("label".to_string(), label);
        schema.classes.insert("Order".to_string(), order);

        let dir = TempDir::new().expect("temp dir");
        let owl_path = dir.path().join("ontology.ttl");
        let shapes_path = dir.path().join("shapes.ttl");
        OwlWriter::new()
            .write(&schema, &owl_path)
            .expect("write owl");
        ShaclWriter::new()
            .write(&schema, &shapes_path)
            .expect("write shacl");

        // Both graphs into one store, so a cross-graph query can check that
        // each shape path resolves to an OWL declaration.
        let store = oxigraph::store::Store::new().expect("store");
        for path in [owl_path, shapes_path] {
            let ttl = fs::read_to_string(&path).expect("read ttl");
            store
                .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
                .unwrap_or_else(|e| panic!("oxigraph rejected TTL: {e}\n\n{ttl}"));
        }

        assert!(
            !ask(
                &store,
                &format!("ASK {{ ?shape <{SH}path> ?p FILTER NOT EXISTS {{ ?p a ?t }} }}")
            ),
            "every sh:path must resolve to a declared OWL property"
        );
    }
}
