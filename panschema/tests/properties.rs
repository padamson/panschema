//! Properties checked over *generated* schemas.
//!
//! The example suite covers the schemas someone thought to write down, and
//! mutation testing checks those examples assert enough — but it perturbs
//! the code and reuses the same inputs, so neither can find a schema shape
//! nobody considered. These properties do, and shrink a failure to a
//! minimal counterexample.
//!
//! Two properties, each chosen because it has a real oracle rather than
//! being a restatement of the implementation:
//!
//! - **Round-trip** — write a schema to Turtle, read it back, and the
//!   constructs the RDF layer models should survive. The oracle is the
//!   input. This is what catches writer/reader disagreements: unit tests
//!   on either side share the author's assumptions about the format, and
//!   a round-trip does not.
//! - **Determinism** — the same schema rendered twice is byte-identical.
//!   Not a tautology: `verify`'s regenerate-and-diff gate and the
//!   checksums a migration runner stores both depend on it, and one format
//!   does not satisfy it (see `JSON_LD_IS_NOT_BYTE_STABLE`).
//!
//! See `docs/features/42-generative-and-fuzz-testing.md`.

use panschema::io::FormatRegistry;
use panschema::linkml::{
    ClassDefinition, EnumDefinition, PermissibleValue, SchemaDefinition, SlotDefinition,
};
use proptest::prelude::*;

/// JSON-LD output differs run to run (serializer ordering), so it is left
/// out of the determinism property. Recorded here rather than silently
/// skipped: it is a known open finding, not a property this format is
/// exempt from wanting.
const JSON_LD_IS_NOT_BYTE_STABLE: &str = "jsonld";

/// The exclusion above is a claim, and this keeps it honest in both
/// directions: the format must exist (so the exclusion is not vestigial)
/// and must not be in the byte-stable list (so nobody adds it back without
/// deleting the finding).
#[test]
fn the_json_ld_exclusion_is_current() {
    assert!(
        FormatRegistry::with_defaults()
            .writer_for_format(JSON_LD_IS_NOT_BYTE_STABLE)
            .is_some(),
        "the excluded format no longer exists; delete the exclusion"
    );
    assert!(
        !BYTE_STABLE_FORMATS.contains(&JSON_LD_IS_NOT_BYTE_STABLE),
        "jsonld is in the byte-stable list but its output differs run to \
         run; fix the serializer ordering before claiming stability"
    );
}

/// Formats whose output is a pure function of the schema. `html` is
/// excluded because it writes a directory of assets rather than one file,
/// and `instance-graph-json` because it describes an A-box this generator
/// does not produce.
const BYTE_STABLE_FORMATS: &[&str] = &[
    "ttl",
    "rdfxml",
    "ntriples",
    "shacl",
    "rust",
    "postgres",
    "json-schema",
    "openapi",
    "graph-json",
];

/// Names are drawn from a small alphabet on purpose. The property is about
/// schema *shape*, and identifier escaping (reserved words, punctuation)
/// already has targeted example tests — letting the generator roam there
/// would rediscover those instead of exploring structure.
fn class_name() -> impl Strategy<Value = String> {
    (0usize..6).prop_map(|i| format!("Class{i}"))
}

fn slot_name() -> impl Strategy<Value = String> {
    (0usize..6).prop_map(|i| format!("slot{i}"))
}

fn enum_name() -> impl Strategy<Value = String> {
    (0usize..3).prop_map(|i| format!("Enum{i}"))
}

/// A range that actually resolves: a LinkML primitive, or one of the
/// declared classes or enums. A dangling range is a separate, already-
/// diagnosed condition — generating one here would mean the properties
/// spent their time on schemas panschema deliberately rejects.
fn range_for(classes: Vec<String>, enums: Vec<String>) -> impl Strategy<Value = String> {
    let mut choices: Vec<String> = ["string", "integer", "boolean", "float", "date", "datetime"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    choices.extend(classes);
    choices.extend(enums);
    proptest::sample::select(choices)
}

fn arb_enum() -> impl Strategy<Value = (String, EnumDefinition)> {
    (enum_name(), proptest::collection::vec(0usize..5, 1..4)).prop_map(|(name, value_ids)| {
        let mut def = EnumDefinition::new(&name);
        for id in value_ids {
            let key = format!("value{id}");
            def.permissible_values.insert(
                key.clone(),
                PermissibleValue {
                    text: key,
                    description: None,
                    meaning: None,
                },
            );
        }
        (name, def)
    })
}

/// A schema whose parts refer only to parts it declares. Built in two
/// stages so slot ranges can name classes and enums the schema actually
/// has: names first, then bodies.
fn arb_schema() -> impl Strategy<Value = SchemaDefinition> {
    (
        proptest::collection::btree_set(class_name(), 1..5),
        proptest::collection::vec(arb_enum(), 0..3),
    )
        .prop_flat_map(|(class_names, enums)| {
            let classes: Vec<String> = class_names.into_iter().collect();
            let enum_names: Vec<String> = enums.iter().map(|(n, _)| n.clone()).collect();
            let per_class = proptest::collection::vec(
                proptest::collection::vec(
                    (
                        slot_name(),
                        range_for(classes.clone(), enum_names.clone()),
                        any::<bool>(),
                    ),
                    0..4,
                ),
                classes.len()..=classes.len(),
            );
            (Just(classes), Just(enums), per_class)
        })
        .prop_map(|(class_names, enums, per_class)| {
            let mut schema = SchemaDefinition::new("generated");
            schema.id = Some("https://example.org/generated".to_string());
            for (name, def) in enums {
                schema.enums.insert(name, def);
            }
            for (idx, (class_name, slots)) in class_names.iter().zip(per_class).enumerate() {
                let mut class = ClassDefinition::new(class_name);
                for (slot_name, range, required) in slots {
                    // Known limitation, excluded from generation rather than
                    // silently normalized: two classes declaring attributes
                    // with the same name are class-local in LinkML but mint
                    // ONE property IRI in RDF, so on read-back only one
                    // class keeps the slot — and two `rdfs:domain` triples
                    // on one property mean *intersection* in OWL, so the
                    // emitted semantics are wrong before the reader is even
                    // involved. Distinct IRIs per declaring class is an
                    // output-breaking change, tracked as its own finding.
                    // Prefixing the class index keeps generated attribute
                    // names class-unique so the properties probe everything
                    // else.
                    let slot_name = format!("c{idx}{slot_name}");
                    let mut slot = SlotDefinition::new(&slot_name);
                    slot.range = Some(range);
                    slot.required = required;
                    class.attributes.insert(slot_name, slot);
                }
                schema.classes.insert(class_name.clone(), class);
            }
            schema
        })
}

/// Render `schema` to `format` and return the bytes. HTML is excluded from
/// the caller's format list, so every writer here produces one file.
fn render(schema: &SchemaDefinition, format: &str) -> Vec<u8> {
    let registry = FormatRegistry::with_defaults();
    let writer = registry
        .writer_for_format(format)
        .unwrap_or_else(|| panic!("no writer registered for `{format}`"));
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join(format!("out.{format}"));
    writer.write(schema, &out).expect("write should succeed");
    std::fs::read(&out).expect("read back")
}

/// What the RDF layer is expected to preserve across a round-trip: the
/// class names, and each class's slot names. Ranges and facets are
/// deliberately *not* compared — see the property's own notes.
fn rdf_normal_form(schema: &SchemaDefinition) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = schema
        .classes
        .iter()
        .map(|(name, class)| {
            let mut slots: Vec<String> = effective_slot_names(class, schema);
            slots.sort();
            (name.clone(), slots)
        })
        .collect();
    out.sort();
    out
}

fn effective_slot_names(class: &ClassDefinition, schema: &SchemaDefinition) -> Vec<String> {
    panschema::linkml_resolve::resolve_effective_slots(class, schema)
        .into_keys()
        .collect()
}

proptest! {
    // Kept modest: these run in the ordinary suite on every push, and the
    // value is in covering shapes between the examples, not in a long
    // campaign. A long run belongs in the fuzzing slice.
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Every byte-stable writer is a pure function of the schema. `verify`'s
    /// regenerate-and-diff gate and a migration runner's stored checksum
    /// both break if this stops holding, and neither notices quietly.
    #[test]
    fn rendering_twice_produces_identical_bytes(schema in arb_schema()) {
        for format in BYTE_STABLE_FORMATS {
            let first = render(&schema, format);
            let second = render(&schema, format);
            prop_assert_eq!(
                first == second,
                true,
                "format `{}` is not byte-stable for this schema",
                format
            );
        }
    }

    /// A schema written to Turtle and read back keeps its classes and their
    /// slots. The oracle is the input schema, which is what makes this
    /// catch a writer/reader disagreement rather than restating either
    /// side's assumptions.
    #[test]
    fn turtle_round_trip_preserves_classes_and_slots(schema in arb_schema()) {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("round.ttl");
        let registry = FormatRegistry::with_defaults();
        registry
            .writer_for_format("ttl")
            .expect("ttl writer")
            .write(&schema, &out)
            .expect("write ttl");
        let read_back = registry
            .reader_for_path(&out)
            .expect("ttl reader")
            .read(&out)
            .expect("read ttl");

        // Known asymmetry, excluded deliberately and asserted so it stays
        // visible: an enum emits as `owl:Class` closed by `owl:oneOf`, and
        // the reader has no rule to rebuild an `EnumDefinition` from that
        // shape — so an enum returns as a class. Tracked as its own
        // finding; this assertion fails if the behaviour changes in either
        // direction, which is what keeps the exclusion from rotting into a
        // silent normalization.
        for enum_name in schema.enums.keys() {
            prop_assert!(
                read_back.classes.contains_key(enum_name),
                "known asymmetry: enum `{}` should come back as a class until \
                 the reader learns owl:oneOf",
                enum_name
            );
        }
        let round_tripped: Vec<_> = rdf_normal_form(&read_back)
            .into_iter()
            .filter(|(name, _)| !schema.enums.contains_key(name))
            .collect();

        prop_assert_eq!(
            round_tripped,
            rdf_normal_form(&schema),
            "classes and slots should survive a Turtle round-trip"
        );
    }
}
