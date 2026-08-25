// Every `tests/*.rs` file compiles and links as its own test binary
// against the debug lib; folding the codegen and dogfood suites in as
// submodules keeps this to one integration binary (plus the
// browser-dependent `e2e`), so an edit-test cycle pays one link, not
// three.
mod dogfood;
mod migrate;
mod rust_writer;

use std::fs;
use std::path::Path;
use std::process::Command;

/// Write a `panschema-publish.toml` + main schema file into `pkg_dir`.
/// The one spelling of `panschema-publish.toml` every package fixture
/// uses; a format change edits this and reaches them all.
fn publish_toml(name: &str, version: &str, main_filename: &str) -> String {
    format!(
        r#"[schema]
name = "{name}"
version = "{version}"
linkml = "1.7.0"

[files]
main = "{main_filename}"
"#
    )
}

/// Mirrors the unified package shape: every path source is a directory
/// containing a publish file + the main file.
fn write_pkg(pkg_dir: &Path, name: &str, version: &str, main_filename: &str, schema_body: &str) {
    fs::create_dir_all(pkg_dir).expect("mkdir pkg");
    fs::write(
        pkg_dir.join("panschema-publish.toml"),
        publish_toml(name, version, main_filename),
    )
    .expect("write publish toml");
    fs::write(pkg_dir.join(main_filename), schema_body).expect("write schema body");
}

/// Convenience: write a package whose main file is a copy of the static
/// `sample_schema.yaml` fixture. Returns the absolute `pkg_dir` path.
fn write_sample_pkg(parent: &Path, dirname: &str) -> std::path::PathBuf {
    let pkg = parent.join(dirname);
    write_pkg(
        &pkg,
        "sample_schema",
        "1.0.0",
        "sample_schema.yaml",
        &fs::read_to_string("tests/fixtures/sample_schema.yaml").expect("read sample schema"),
    );
    pkg
}

#[test]
fn class_card_surfaces_mixins_slots_and_resolved_xrefs() {
    let output_dir = std::env::temp_dir().join("panschema_class_card_dogfood");
    let _ = fs::remove_dir_all(&output_dir);
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/class_card_dogfood.yaml",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html =
        fs::read_to_string(output_dir.join("index.html")).expect("Failed to read index.html");
    let doc_card = extract_class_card(&html, "Document");
    assert!(
        doc_card.contains(r##"href="#class-Auditable""##),
        "Document card missing anchor to Auditable mixin; got:\n{doc_card}"
    );
    assert!(
        doc_card.contains(r##"href="#class-Publishable""##),
        "Document card missing anchor to Publishable mixin; got:\n{doc_card}"
    );
    assert!(
        doc_card.contains(r##"href="#enum-Status""##),
        "Document card missing resolved Status xref; got:\n{doc_card}"
    );
    assert!(
        !doc_card.contains("[[Status]]"),
        "literal [[Status]] should not remain; got:\n{doc_card}"
    );
    assert!(doc_card.contains("Slots"), "missing Slots section");
    assert!(
        doc_card.contains("title") && doc_card.contains("body"),
        "Document slots not surfaced; got:\n{doc_card}"
    );

    let report_card = extract_class_card(&html, "Report");
    assert!(
        report_card.contains("refined here"),
        "Report card missing 'refined here' flag for body slot_usage override; got:\n{report_card}"
    );

    assert!(
        html.contains("cco") && html.contains("https://www.commoncoreontologies.org/"),
        "cco prefix declaration missing from rendered HTML"
    );
    assert!(
        html.contains("obo") && html.contains("http://purl.obolibrary.org/obo/"),
        "obo prefix declaration missing from rendered HTML"
    );
}

fn extract_class_card<'a>(html: &'a str, class_id: &str) -> &'a str {
    let anchor = format!(r##"id="class-{class_id}""##);
    let start = html
        .find(&anchor)
        .unwrap_or_else(|| panic!("`{class_id}` class card not found"));
    let end = html[start..]
        .find("</article>")
        .map(|n| start + n)
        .unwrap_or_else(|| panic!("`{class_id}` class card has no closing tag"));
    &html[start..end]
}

/// Parse a `window.<name> = <json>;` assignment embedded in generated HTML,
/// returning the JSON value. Robust to trailing content after the value
/// (parses the first JSON value following the marker).
fn extract_json_assignment(html: &str, name: &str) -> serde_json::Value {
    let marker = format!("window.{name} = ");
    let start = html.find(&marker).expect("assignment marker present") + marker.len();
    serde_json::Deserializer::from_str(&html[start..])
        .into_iter::<serde_json::Value>()
        .next()
        .expect("a JSON value follows the marker")
        .expect("the embedded JSON parses")
}

#[test]
fn class_card_and_graph_hover_agree_on_slot_usage_refined_range() {
    // Cross-writer consistency: a slot refined via `slot_usage` must
    // show the refined range in BOTH the HTML class card and the
    // graph hover payload embedded in the same page. Both writers
    // resolve through the shared resolver, and this pins that
    // neither regresses to the slot's global un-refined definition.
    let schema_yaml = r#"
id: https://example.org/xwriter
name: xwriter
prefixes:
  linkml: https://w3id.org/linkml/
default_range: string
classes:
  Activity:
    attributes:
      wasGeneratedBy:
        range: Activity
  QuestionFormation:
    is_a: Activity
  Question:
    is_a: Activity
    slot_usage:
      wasGeneratedBy:
        range: QuestionFormation
"#;
    let tmp = std::env::temp_dir().join("panschema_xwriter_consistency");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.yaml");
    fs::write(&schema_path, schema_yaml).unwrap();
    let output_dir = tmp.join("out");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html = fs::read_to_string(output_dir.join("index.html")).expect("read index.html");

    // HTML side: Question's card lists wasGeneratedBy with the
    // refined range as the linked class.
    let question_card = extract_class_card(&html, "Question");
    assert!(
        question_card.contains("wasGeneratedBy"),
        "Question card must list the refined slot; got: {question_card}"
    );
    assert!(
        question_card.contains(r##"href="#class-QuestionFormation""##),
        "Question card must link the refined range QuestionFormation; got: {question_card}"
    );

    // Graph side: the embedded graph JSON's kindMetadata for
    // class:Question carries the same refined range.
    let marker = "window.__PANSCHEMA_GRAPH_DATA__ = ";
    let start = html.find(marker).expect("embedded graph JSON") + marker.len();
    let end = html[start..].find(";\n").map(|n| start + n).unwrap();
    let graph: serde_json::Value =
        serde_json::from_str(&html[start..end]).expect("graph JSON parses");
    let question_node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "class:Question")
        .expect("class:Question node");
    let slots = question_node["kind_metadata"]["slots"].as_array().unwrap();
    let was_generated_by = slots
        .iter()
        .find(|s| s["name"] == "wasGeneratedBy")
        .expect("wasGeneratedBy in hover slots");
    assert_eq!(
        was_generated_by["range"], "QuestionFormation",
        "hover payload must carry the refined range, matching the class card"
    );

    let _ = fs::remove_dir_all(tmp);
}

#[test]
fn every_graph_node_has_a_matching_html_card() {
    // The graph hover reuses each node's rendered HTML card, looked up by
    // `id="<kind>-<name>"`; the JS `buildCompactNodeHover` is only a thin
    // last resort. This pins the invariant that makes that reuse safe:
    // every graph node id `<kind>:<name>` has a matching card element, so
    // the fallback is never the real render path.
    let output_dir = std::env::temp_dir().join("panschema_node_card_correspondence");
    let _ = fs::remove_dir_all(&output_dir);
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");
    let html = fs::read_to_string(output_dir.join("index.html")).expect("read index.html");

    // Pull the embedded graph object (`window.__PANSCHEMA_GRAPH_DATA__ =
    // {...};`). A streaming parse reads exactly the first JSON value, so a
    // `;` inside a description string can't truncate it.
    let marker = "window.__PANSCHEMA_GRAPH_DATA__ = ";
    let start = html.find(marker).expect("embedded graph data") + marker.len();
    let graph: serde_json::Value = serde_json::Deserializer::from_str(&html[start..])
        .into_iter()
        .next()
        .expect("a graph JSON value")
        .expect("valid graph JSON");

    let nodes = graph["nodes"].as_array().expect("nodes array");
    assert!(!nodes.is_empty(), "reference schema should produce nodes");
    for node in nodes {
        let id = node["id"].as_str().expect("node id string");
        let (kind, name) = id.split_once(':').expect("node id is `<kind>:<name>`");
        let card_id = format!("id=\"{kind}-{name}\"");
        assert!(
            html.contains(&card_id),
            "graph node `{id}` has no matching HTML card (`{card_id}`) — the hover would fall back",
        );
    }
}

#[test]
fn generates_documentation_from_reference_ontology() {
    let output_dir = std::env::temp_dir().join("panschema_integration_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");

    let index_path = output_dir.join("index.html");
    assert!(index_path.exists(), "index.html was not generated");

    let html = fs::read_to_string(&index_path).expect("Failed to read index.html");

    // Verify key content
    assert!(
        html.contains("panschema Reference Ontology"),
        "Missing ontology title"
    );
    assert!(
        html.contains("http://example.org/panschema/reference"),
        "Missing ontology IRI"
    );
    assert!(html.contains("0.2.0"), "Missing version");
    assert!(
        html.contains("A reference ontology for testing"),
        "Missing description"
    );

    // Verify graph visualization is included
    assert!(
        html.contains("__PANSCHEMA_GRAPH_DATA__"),
        "Missing graph data JSON"
    );
    assert!(
        html.contains("graph-visualization"),
        "Missing graph visualization section"
    );
    assert!(
        html.contains("graph-canvas"),
        "Missing graph canvas element"
    );

    // Verify graph data contains expected nodes
    assert!(
        html.contains("class:Animal"),
        "Missing Animal class in graph data"
    );
    assert!(
        html.contains("class:Dog"),
        "Missing Dog class in graph data"
    );
    assert!(
        html.contains("subclass_of"),
        "Missing subclass_of edges in graph data"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn classes_section_renders_is_a_hierarchy_with_flat_toggle() {
    // The reference ontology's Animal → Mammal → Dog chain must come
    // out as semantically nested lists, with Person (no is_a, no
    // descendants) flat alongside; the Flat/Tree toggle and the
    // alphabetical order ranks the flat view sorts by are part of the
    // same page.
    let output_dir = std::env::temp_dir().join("panschema_class_tree_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html = fs::read_to_string(output_dir.join("index.html")).expect("read index.html");

    // Semantic nesting: each level of the chain opens a child <ul>
    // before the next card appears.
    let tree_start = html.find(r#"<ul class="class-tree">"#).expect("tree root");
    let animal = html.find(r##"id="class-Animal""##).expect("Animal card");
    let mammal = html.find(r##"id="class-Mammal""##).expect("Mammal card");
    let dog = html.find(r##"id="class-Dog""##).expect("Dog card");
    assert!(tree_start < animal && animal < mammal && mammal < dog);
    assert!(
        html[animal..mammal].contains(r#"<ul class="class-tree-children">"#),
        "Mammal must open inside Animal's child list"
    );
    assert!(
        html[mammal..dog].contains(r#"<ul class="class-tree-children">"#),
        "Dog must open inside Mammal's child list"
    );

    // Each class renders exactly one card, so #class-Foo anchors keep
    // working in both views.
    for id in ["Animal", "Mammal", "Dog", "Cat", "Pet", "Person"] {
        let anchor = format!(r##"id="class-{id}""##);
        assert_eq!(
            html.matches(&anchor).count(),
            1,
            "exactly one card for {id}"
        );
    }

    // Disconnected root: Person sits at the tree's top level. The Animal
    // subtree (Mammal → {Cat, Dog}, then Pet) fully closes before
    // Person's top-level <li>; Pet, Animal's last child, emits the final
    // `</ul></li>` that closes Animal's level.
    let pet = html.find(r##"id="class-Pet""##).expect("Pet card");
    let person = html.find(r##"id="class-Person""##).expect("Person card");
    assert!(
        dog < pet && pet < person,
        "Pet nests under Animal before Person"
    );
    assert!(
        html[pet..person].contains("</ul></li>"),
        "the Animal subtree must close before Person's top-level entry"
    );

    // Flat view sorts by --flat-order rank; ranks follow alphabetical
    // order: Animal, Cat, Dog, Mammal, Person, Pet.
    for (id, rank) in [
        ("Animal", 0),
        ("Cat", 1),
        ("Dog", 2),
        ("Mammal", 3),
        ("Person", 4),
        ("Pet", 5),
    ] {
        let card = html.find(&format!(r##"id="class-{id}""##)).unwrap();
        let node_start = html[..card].rfind("<li class=\"class-tree-node\"").unwrap();
        assert!(
            html[node_start..card].contains(&format!("--flat-order: {rank}")),
            "{id} must carry alphabetical rank {rank}"
        );
    }

    // The Flat/Tree toggle ships with the page and defaults to tree.
    assert!(
        html.contains(r#"data-view="tree""#),
        "tree is the default view"
    );
    assert!(
        html.contains(r#"class="view-toggle-btn" data-view="flat""#),
        "flat toggle button present"
    );
    assert!(
        html.contains("panschema-classes-view"),
        "view preference persists via localStorage key"
    );

    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn generates_documentation_from_linkml_yaml() {
    let output_dir = std::env::temp_dir().join("panschema_yaml_integration_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/sample_schema.yaml",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");

    let index_path = output_dir.join("index.html");
    assert!(index_path.exists(), "index.html was not generated");

    let html = fs::read_to_string(&index_path).expect("Failed to read index.html");

    // Verify key content from YAML schema
    assert!(
        html.contains("Sample LinkML Schema"),
        "Missing schema title"
    );
    assert!(
        html.contains("https://example.org/sample"),
        "Missing schema IRI"
    );
    assert!(html.contains("1.0.0"), "Missing version");
    assert!(
        html.contains("A sample schema for testing"),
        "Missing description"
    );

    // Verify classes are rendered
    assert!(html.contains("Person"), "Missing Person class");
    assert!(html.contains("Organization"), "Missing Organization class");
    assert!(html.contains("A human being"), "Missing Person description");

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn owl_roundtrip_preserves_schema() {
    use panschema::io::FormatRegistry;
    use std::path::PathBuf;

    let input_path = PathBuf::from("tests/fixtures/reference.ttl");
    let output_dir = std::env::temp_dir().join("panschema_owl_roundtrip_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("roundtrip.ttl");

    let registry = FormatRegistry::with_defaults();

    // Read the reference ontology
    let reader = registry
        .reader_for_path(&input_path)
        .expect("Should find TTL reader");
    let schema = reader.read(&input_path).expect("Should parse TTL file");

    // Write to TTL
    let writer = registry
        .writer_for_format("ttl")
        .expect("Should find TTL writer");
    writer
        .write(&schema, &output_path)
        .expect("Should write TTL file");

    // Verify the output file exists and is parseable
    assert!(output_path.exists(), "Output TTL file should exist");

    // Read back the written file
    let schema2 = reader
        .read(&output_path)
        .expect("Should parse written TTL file");

    // Verify key data is preserved
    assert_eq!(schema.name, schema2.name);
    assert_eq!(schema.title, schema2.title);
    assert_eq!(schema.version, schema2.version);
    assert_eq!(schema.classes.len(), schema2.classes.len());
    assert_eq!(schema.slots.len(), schema2.slots.len());

    // Enriched constructs must survive Turtle → IR → Turtle → IR. Without
    // the reader parsing each construct back, the writer's output would be
    // silently dropped on read-back and these assertions would fail.

    // owl:deprecated → deprecated flag (RDF carries only the boolean, so
    // the note is empty but present).
    let pet = schema2.classes.get("Pet").expect("Pet class preserved");
    assert!(
        pet.deprecated.is_some(),
        "owl:deprecated must survive round-trip"
    );

    // skos:altLabel → aliases; rdfs:seeAlso → see_also.
    let person = schema2.classes.get("Person").expect("Person preserved");
    let mut aliases = person.aliases.clone();
    aliases.sort();
    assert_eq!(
        aliases,
        vec!["Human", "Individual"],
        "skos:altLabel must survive round-trip"
    );
    assert_eq!(
        person.see_also,
        vec!["http://xmlns.com/foaf/0.1/Person"],
        "rdfs:seeAlso must survive round-trip"
    );

    // skos:exactMatch → exact_mappings (on a class and a slot).
    assert_eq!(
        person.exact_mappings,
        vec!["http://schema.org/Person"],
        "class skos:exactMatch must survive round-trip"
    );
    let owns = schema2.slots.get("owns").expect("owns slot preserved");
    assert_eq!(
        owns.exact_mappings,
        vec!["http://purl.org/dc/terms/relation"],
        "slot skos:exactMatch must survive round-trip"
    );

    // skos:closeMatch → close_mappings.
    let cat = schema2.classes.get("Cat").expect("Cat class preserved");
    assert_eq!(
        cat.close_mappings,
        vec!["http://dbpedia.org/resource/Cat"],
        "skos:closeMatch must survive round-trip"
    );

    // owl:SymmetricProperty / owl:TransitiveProperty → characteristic bools.
    let related = schema2.slots.get("relatedTo").expect("relatedTo preserved");
    assert!(
        related.symmetric && related.transitive,
        "OWL property characteristics must survive round-trip"
    );

    // owl:inverseOf → inverse.
    assert_eq!(
        owns.inverse.as_deref(),
        Some("hasOwner"),
        "owl:inverseOf must survive round-trip"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn no_graph_flag_disables_graph_visualization() {
    let output_dir = std::env::temp_dir().join("panschema_no_graph_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
            "--no-graph",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");

    let index_path = output_dir.join("index.html");
    assert!(index_path.exists(), "index.html was not generated");

    let html = fs::read_to_string(&index_path).expect("Failed to read index.html");

    // Verify graph visualization is NOT included
    assert!(
        !html.contains("__PANSCHEMA_GRAPH_DATA__"),
        "Graph data should not be present with --no-graph"
    );
    assert!(
        !html.contains("graph-visualization"),
        "Graph visualization section should not be present with --no-graph"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn generate_instances_renders_linkml_data_as_the_instance_graph() {
    let output_dir = std::env::temp_dir().join("panschema_linkml_instances_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html = fs::read_to_string(output_dir.join("index.html")).expect("read index.html");

    // The instance graph is embedded from the LinkML data file, even though
    // the schema declares no OWL individuals — a canvas and its A-box data.
    assert!(
        html.contains("instance-graph-canvas"),
        "the instance-graph canvas should be present"
    );
    assert!(
        html.contains("__PANSCHEMA_INSTANCE_GRAPHS__"),
        "the LinkML A-box should be embedded as instance-graph data"
    );

    // Each record became a typed node; each class-ranged scalar an edge.
    let graphs = extract_json_assignment(&html, "__PANSCHEMA_INSTANCE_GRAPHS__");
    let data = &graphs.as_array().expect("payload array")[0]["data"];
    assert_eq!(
        data["nodes"].as_array().expect("nodes").len(),
        4,
        "two wines + two wineries → four instance nodes"
    );
    assert_eq!(
        data["edges"].as_array().expect("edges").len(),
        2,
        "each wine's produced_by is a reference edge to its winery"
    );

    // A record's identifier-keyed reference resolves to the target node's id.
    let edges = data["edges"].as_array().unwrap();
    assert!(
        edges
            .iter()
            .any(|e| e["source"] == "individual:chateauMorgon"
                && e["target"] == "individual:morgonEstate"
                && e["label"] == "produced_by"),
        "the produced_by edge should connect the wine to its winery, got {edges:?}"
    );

    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn generate_reports_conformance_violations_in_the_instance_data() {
    // A duplicate identifier is a conformance violation that the
    // reference-integrity check can't see. Embedding an A-box into an output
    // must report it, not just dangling references — otherwise a broken
    // exemplar publishes onto a docs site silently.
    let dir = std::env::temp_dir().join("panschema_instance_conformance_test");
    let _ = fs::remove_dir_all(&dir);

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_duplicate_id.yaml",
            "--output",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(out.status.success(), "non-strict generation should succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("chateauMorgon") && stderr.contains("more than one record"),
        "the duplicate identifier should warn, naming the id; got: {stderr}"
    );

    // Under --strict the same violation is a hard failure, as a dangling
    // reference already is.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_duplicate_id.yaml",
            "--output",
            dir.to_str().unwrap(),
            "--strict",
        ])
        .output()
        .expect("run panschema");
    assert!(
        !out.status.success(),
        "--strict must fail on a non-conforming A-box"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn generate_carries_several_curated_instance_graphs() {
    let output_dir = std::env::temp_dir().join("panschema_multi_instances_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_preview.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html = fs::read_to_string(output_dir.join("index.html")).expect("read index.html");

    // Both graphs are declared, so the reader gets a selector naming each by
    // its file stem, and a content panel per dataset.
    assert!(
        html.contains(r#"role="tablist""#),
        "several datasets must offer a selector"
    );
    assert!(
        html.contains(">wine_instances_preview") && html.contains(">wine_instances"),
        "each dataset is named after its file; got selector-less page"
    );
    assert_eq!(
        html.matches(r#"class="instance-dataset-panel""#).count(),
        2,
        "one content panel per declared dataset"
    );
    assert!(
        html.contains(r#"data-instance-dataset="1" hidden>"#),
        "only the first dataset shows before the reader picks another"
    );

    // Individuals from both A-boxes are in the page, each in its own panel.
    assert!(
        html.contains("Preview Pinot") && html.contains("Château Morgon"),
        "both datasets' individual cards must render"
    );

    // Each payload entry carries its own graph, in declaration order.
    let graphs = extract_json_assignment(&html, "__PANSCHEMA_INSTANCE_GRAPHS__");
    let entries = graphs.as_array().expect("payload array");
    assert_eq!(entries.len(), 2, "one payload per declared dataset");
    assert_eq!(entries[0]["name"], "wine_instances_preview");
    assert_eq!(
        entries[0]["data"]["nodes"].as_array().expect("nodes").len(),
        2,
        "the preview carries only its own wine and winery"
    );
    assert_eq!(entries[1]["name"], "wine_instances");
    assert_eq!(
        entries[1]["data"]["nodes"].as_array().expect("nodes").len(),
        4,
        "the worked example carries its own four records"
    );

    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn single_a_box_formats_reject_several_instances_files() {
    let dir = std::env::temp_dir().join("panschema_multi_instances_reject_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");

    // ttl folds one A-box into the emitted graph; several files have no
    // unambiguous meaning, so the build must say so rather than pick one.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_preview.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--format",
            "ttl",
            "--output",
            dir.join("out.ttl").to_str().unwrap(),
        ])
        .output()
        .expect("Failed to execute panschema");
    assert!(
        !out.status.success(),
        "several A-boxes for a single-graph format must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("single instance graph") && stderr.contains("HTML"),
        "the error should explain the limit and the alternative; got: {stderr}"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn instances_flag_warns_only_for_formats_that_ignore_it() {
    let dir = std::env::temp_dir().join("panschema_instances_warn_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");

    // With --instances on a format that consumes neither the graph nor the
    // A-box (e.g. rust), the flag is ignored — warn so the omission isn't
    // silent.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--format",
            "rust",
            "--output",
            dir.join("with.rs").to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--instances only affects the HTML, RDF, and instance-graph-json"),
        "a format that ignores --instances should warn; got: {stderr}"
    );

    // An RDF format consumes --instances (the A-box emits), so no warning.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--format",
            "ttl",
            "--output",
            dir.join("with.ttl").to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("only affects"),
        "an RDF format with --instances should not warn; got: {stderr}"
    );

    // The same format without --instances does not warn either.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--format",
            "ttl",
            "--output",
            dir.join("without.ttl").to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("--instances"),
        "no --instances warning should appear without the flag; got: {stderr}"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn validate_command_exit_code_reflects_conformance() {
    // Conforming data validates clean and exits zero.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--data",
            "tests/fixtures/wine_instances.yaml",
        ])
        .output()
        .expect("run panschema");
    assert!(
        out.status.success(),
        "conforming data should exit zero; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("conforms to"),
        "clean validation should report conformance"
    );

    // A dangling reference is a violation: non-zero exit, named on stderr.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--data",
            "tests/fixtures/wine_instances_dangling.yaml",
        ])
        .output()
        .expect("run panschema");
    assert!(
        !out.status.success(),
        "non-conforming data must exit non-zero"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("ghostWinery"),
        "the violation should name the dangling reference"
    );
}

#[test]
fn validate_reports_ids_that_mint_one_iri_across_two_data_files() {
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--data",
            "tests/fixtures/wine_instances.yaml",
            "--data",
            "tests/fixtures/wine_instances_second_dataset.yaml",
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("minted by more than one dataset")
            && stderr.contains("chateauMorgon")
            && stderr.contains("wine_instances.yaml")
            && stderr.contains("wine_instances_second_dataset.yaml"),
        "the collision names the id and both files; got: {stderr}"
    );
    assert!(
        out.status.success(),
        "sharing records across datasets can be deliberate, so it reports \
         rather than fails; stderr: {stderr}"
    );
}

#[test]
fn validate_reports_an_entity_two_scoped_datasets_each_defined() {
    // Scoping's inverse hazard: `aws` should have been one shared record, so
    // the two estates now hold two distinct providers. The collision check
    // cannot see this — after scoping there is no collision left to find.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/scoped_estate.yaml",
            "--data",
            "tests/fixtures/two_root_split_acme.yaml",
            "--data",
            "tests/fixtures/two_root_split_contoso.yaml",
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "a split is reported, not fatal");
    assert!(
        stderr.contains("`aws`") && stderr.contains("defined identically"),
        "the shared provider each estate defined locally is reported; got: {stderr}"
    );
    assert!(
        !stderr.contains("`api-gateway`"),
        "but the services that genuinely differ are not — warning about those \
         would fire on every separation scoping got right; got: {stderr}"
    );
}

#[test]
fn rendered_docs_carry_scoped_and_shared_iris_side_by_side() {
    // The consumer shape: one page holding a scoped estate and the shared
    // catalogue it references. The estate's record must be scoped under its
    // root, the shared record must not be, and the estate's reference must
    // resolve to the IRI the catalogue actually mints.
    let dir = std::env::temp_dir().join("panschema_scoped_render_test");
    let _ = fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/two_root_estate.yaml",
            "--instances",
            "tests/fixtures/two_root_estate_data.yaml",
            "--instances",
            "tests/fixtures/two_root_shared_data.yaml",
            "--output",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(
        out.status.success(),
        "generation should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = fs::read_to_string(dir.join("index.html")).expect("rendered page");
    assert!(
        html.contains("https://example.org/estate/acme/api-gateway"),
        "the estate's record renders scoped under its root"
    );
    assert!(
        html.contains("https://example.org/catalog/aws"),
        "the shared record renders in the shared namespace, unscoped"
    );
    assert!(
        !html.contains("estate/acme/catalog"),
        "and the CURIE-named shared record is never nested under a scope"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn validate_names_the_root_each_dataset_was_read_against() {
    // Both roots declare `id` and `name`, so only the collections decide. A
    // wrong-root read would conform vacuously with zero records, which is why
    // the reading and the count are on the success line.
    for (data, expected_root, records) in [
        (
            "two_root_catalog_data.yaml",
            "ProviderCatalog",
            "2 record(s)",
        ),
        ("two_root_estate_data.yaml", "Enterprise", "2 record(s)"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args([
                "validate",
                "--schema",
                "tests/fixtures/two_root_estate.yaml",
                "--data",
                &format!("tests/fixtures/{data}"),
            ])
            .output()
            .expect("run panschema");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{data} should conform; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            stdout.contains(expected_root) && stdout.contains(records),
            "{data} must report its root and record count; got: {stdout}"
        );
    }
}

#[test]
fn a_single_data_file_reports_no_collisions() {
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--data",
            "tests/fixtures/wine_instances.yaml",
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("minted by more than one dataset"),
        "one dataset can collide with nothing; got: {stderr}"
    );
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("conforms to"),
        "and the single-file report is unchanged"
    );
}

#[test]
fn cross_graph_reference_validates_clean_and_is_summarized() {
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--data",
            "tests/fixtures/wine_instances_cross_graph.yaml",
        ])
        .output()
        .expect("run panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "a reference into another graph is not a violation; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("names no instance"),
        "and must not be reported as dangling; got: {stderr}"
    );
    assert!(
        stderr.contains("cross-graph reference(s)") && stderr.contains("wine:morgonEstateGlobal"),
        "but it must be summarised, naming its target; got: {stderr}"
    );
}

#[test]
fn dangling_instance_reference_warns_and_fails_under_strict() {
    let dir = std::env::temp_dir().join("panschema_instance_dangling_test");
    let _ = fs::remove_dir_all(&dir);

    // A wine references a winery the data file doesn't define. Without
    // --strict, generation succeeds but warns, naming the dangling reference.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_dangling.yaml",
            "--output",
            dir.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(out.status.success(), "non-strict generation should succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ghostWinery") && stderr.contains("names no instance"),
        "the dangling instance reference should warn, naming the missing id; got: {stderr}"
    );

    // Under --strict the same dangling reference is a hard failure.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_dangling.yaml",
            "--output",
            dir.to_str().unwrap(),
            "--strict",
        ])
        .output()
        .expect("run panschema");
    assert!(
        !out.status.success(),
        "--strict must fail on a dangling instance reference"
    );

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn viz_mode_flag_is_recognized() {
    let output_dir = std::env::temp_dir().join("panschema_viz_mode_test");
    let _ = fs::remove_dir_all(&output_dir);

    // Test --viz-mode 2d
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
            "--viz-mode",
            "2d",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(
        status.success(),
        "panschema with --viz-mode 2d exited with error"
    );

    // Cleanup
    let _ = fs::remove_dir_all(&output_dir);

    // Test --viz-mode 3d
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
            "--viz-mode",
            "3d",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(
        status.success(),
        "panschema with --viz-mode 3d exited with error"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

// ========== RDF Format Integration Tests ==========

#[test]
fn instance_graph_json_renders_the_abox_as_its_own_artifact() {
    // One invocation, one named artifact (the pandoc model): the instance
    // graph has its own format id, and --output names exactly the file
    // produced — instance-kinded, nodes carrying the same minted IRIs the
    // RDF A-box uses.
    let out_file = std::env::temp_dir().join(format!(
        "panschema_instancegraph_{}.json",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--format",
            "instance-graph-json",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(
        output.status.success(),
        "instance-graph-json should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let instance_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_file).expect("read instance doc"))
            .expect("parse instance doc");
    assert_eq!(instance_doc["graph_kind"], "instance");
    assert_eq!(instance_doc["format_version"], "1.2");
    let uris: Vec<&str> = instance_doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n["uri"].as_str())
        .collect();
    assert!(
        uris.contains(&"https://example.org/wine/chateauMorgon"),
        "instance nodes should carry minted IRIs; got: {uris:?}"
    );
    let _ = fs::remove_file(&out_file);
}

#[test]
fn graph_json_stays_a_single_schema_document() {
    // The schema graph keeps its format id and single-document output —
    // supplying --instances doesn't graft an A-box onto it.
    let out_file = std::env::temp_dir().join(format!(
        "panschema_graphjson_plain_{}.json",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances.yaml",
            "--format",
            "graph-json",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ignored for format `graph-json`"),
        "graph-json should warn that --instances is ignored; got: {stderr}"
    );
    let schema_doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&out_file).expect("read")).expect("parse");
    assert_eq!(schema_doc["graph_kind"], "schema");
    assert!(
        schema_doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["node_type"] != "individual"),
        "the schema document must stay T-box-only"
    );
    let _ = fs::remove_file(&out_file);
}

#[test]
fn rdf_family_with_instances_emits_the_abox() {
    // Every RDF-family format accepts --instances and carries the A-box:
    // the minted individual IRI appears in each serialization, so a triple
    // store loading any of them sees the same knowledge graph.
    for format in ["ttl", "jsonld", "rdfxml", "ntriples"] {
        let out_file = std::env::temp_dir().join(format!(
            "panschema_abox_{}_{}.out",
            std::process::id(),
            format
        ));
        let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args([
                "generate",
                "--schema",
                "tests/fixtures/wine_catalog.yaml",
                "--instances",
                "tests/fixtures/wine_instances.yaml",
                "--format",
                format,
                "--output",
                out_file.to_str().unwrap(),
            ])
            .output()
            .expect("run panschema");
        assert!(
            output.status.success(),
            "generate --format {format} with instances should succeed; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let content = fs::read_to_string(&out_file).expect("read output");
        // Pretty TTL compacts the IRI to its CURIE form via the prefix map;
        // the other serializations carry it absolute.
        assert!(
            content.contains("https://example.org/wine/chateauMorgon")
                || content.contains("wine:chateauMorgon"),
            "{format} output should carry the minted individual IRI; got:\n{}",
            &content[..content.len().min(800)]
        );
        let _ = fs::remove_file(&out_file);
    }
}

#[test]
fn rdf_with_dangling_instance_reference_fails_under_strict() {
    let out_file = std::env::temp_dir().join(format!(
        "panschema_abox_dangling_{}.ttl",
        std::process::id()
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/wine_catalog.yaml",
            "--instances",
            "tests/fixtures/wine_instances_dangling.yaml",
            "--format",
            "ttl",
            "--strict",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .output()
        .expect("run panschema");
    assert!(
        !output.status.success(),
        "a dangling instance reference must fail the build under --strict"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ghostWinery")
            && stderr.contains("produced_by")
            && stderr.contains("names no instance"),
        "stderr should name the referring slot and the missing target; got: {stderr}"
    );
    let _ = fs::remove_file(&out_file);
}

#[test]
fn generates_jsonld_via_cli() {
    let output_dir = std::env::temp_dir().join("panschema_jsonld_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("output.jsonld");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_path.to_str().unwrap(),
            "--format",
            "jsonld",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");
    assert!(output_path.exists(), "JSON-LD file was not generated");

    let content = fs::read_to_string(&output_path).expect("Failed to read JSON-LD");

    // Verify it's valid JSON-LD with expected content
    // Note: sophia produces expanded JSON-LD without @context, using full IRIs
    assert!(content.contains("@id"), "Missing @id in JSON-LD");
    assert!(content.contains("@type"), "Missing @type in JSON-LD");
    assert!(
        content.contains("http://example.org/panschema/reference"),
        "Missing ontology IRI in JSON-LD"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn generates_rdfxml_via_cli() {
    let output_dir = std::env::temp_dir().join("panschema_rdfxml_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("output.rdf");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_path.to_str().unwrap(),
            "--format",
            "rdfxml",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");
    assert!(output_path.exists(), "RDF/XML file was not generated");

    let content = fs::read_to_string(&output_path).expect("Failed to read RDF/XML");

    // Verify it's valid RDF/XML with expected content
    assert!(
        content.contains("rdf:RDF") || content.contains("<RDF"),
        "Missing rdf:RDF root element"
    );
    assert!(
        content.contains("http://example.org/panschema/reference"),
        "Missing ontology IRI in RDF/XML"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn generates_ntriples_via_cli() {
    let output_dir = std::env::temp_dir().join("panschema_ntriples_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("output.nt");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_path.to_str().unwrap(),
            "--format",
            "ntriples",
        ])
        .status()
        .expect("Failed to execute panschema");

    assert!(status.success(), "panschema exited with error");
    assert!(output_path.exists(), "N-Triples file was not generated");

    let content = fs::read_to_string(&output_path).expect("Failed to read N-Triples");

    // Verify it contains N-Triples format (full URIs, no prefixes)
    assert!(
        content.contains("<http://example.org/panschema/reference>"),
        "Missing ontology IRI in N-Triples"
    );
    assert!(
        content.contains("<http://www.w3.org/2002/07/owl#Ontology>"),
        "Missing owl:Ontology type in N-Triples"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

#[test]
fn all_rdf_formats_produce_equivalent_content() {
    use panschema::io::FormatRegistry;
    use std::path::PathBuf;

    let input_path = PathBuf::from("tests/fixtures/reference.ttl");
    let output_dir = std::env::temp_dir().join("panschema_rdf_equivalence_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let registry = FormatRegistry::with_defaults();

    // Read the reference ontology
    let reader = registry
        .reader_for_path(&input_path)
        .expect("Should find TTL reader");
    let schema = reader.read(&input_path).expect("Should parse TTL file");

    // Write to all RDF formats
    let formats = vec![
        ("ttl", output_dir.join("output.ttl")),
        ("jsonld", output_dir.join("output.jsonld")),
        ("rdfxml", output_dir.join("output.rdf")),
        ("ntriples", output_dir.join("output.nt")),
    ];

    for (format, path) in &formats {
        let writer = registry
            .writer_for_format(format)
            .unwrap_or_else(|| panic!("Should find {} writer", format));
        writer
            .write(&schema, path)
            .unwrap_or_else(|_| panic!("Should write {} file", format));
        assert!(path.exists(), "{} file should exist", format);
    }

    // Read all files and verify they contain the same key data
    let ttl_content = fs::read_to_string(&formats[0].1).expect("Failed to read TTL");
    let jsonld_content = fs::read_to_string(&formats[1].1).expect("Failed to read JSON-LD");
    let rdfxml_content = fs::read_to_string(&formats[2].1).expect("Failed to read RDF/XML");
    let nt_content = fs::read_to_string(&formats[3].1).expect("Failed to read N-Triples");

    // All formats should contain the ontology IRI
    let ontology_iri = "http://example.org/panschema/reference";
    assert!(
        ttl_content.contains(ontology_iri),
        "TTL missing ontology IRI"
    );
    assert!(
        jsonld_content.contains(ontology_iri),
        "JSON-LD missing ontology IRI"
    );
    assert!(
        rdfxml_content.contains(ontology_iri),
        "RDF/XML missing ontology IRI"
    );
    assert!(
        nt_content.contains(&format!("<{}>", ontology_iri)),
        "N-Triples missing ontology IRI"
    );

    // All formats should reference the Animal class
    let animal_uri = "http://example.org/panschema/reference#Animal";
    assert!(ttl_content.contains(animal_uri), "TTL missing Animal class");
    assert!(
        jsonld_content.contains(animal_uri),
        "JSON-LD missing Animal class"
    );
    assert!(
        rdfxml_content.contains(animal_uri),
        "RDF/XML missing Animal class"
    );
    assert!(
        nt_content.contains(&format!("<{}>", animal_uri)),
        "N-Triples missing Animal class"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}

/// `panschema generate` (no --schema) discovers a `panschema.toml`, walks
/// `[schemas]`, and runs the HtmlWriter according to `[generate.<name>]`.
#[test]
fn manifest_driven_generate_runs_html_writer_for_path_source() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    // Place a v0.3 package (publish.toml + schema) at consumer/sample-pkg/.
    write_sample_pkg(consumer, "sample-pkg");

    // Write the manifest.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "docs/"
"#,
    )
    .expect("write manifest");

    // Run `panschema generate` from the consumer dir (no --schema).
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    // Output should land at consumer/docs/index.html (relative to the manifest).
    let index = consumer.join("docs").join("index.html");
    assert!(
        index.exists(),
        "expected manifest-driven generate to write {}",
        index.display()
    );

    let html = fs::read_to_string(&index).expect("read index.html");
    assert!(
        html.contains("Sample LinkML Schema"),
        "Missing schema title from manifest-generated HTML"
    );

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "docs/"
html_page_layout = "instances-first"
html_schema_sections = false
"#,
    )
    .expect("write manifest");
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "composed generate exited with error");
    let html = fs::read_to_string(&index).expect("read index.html");
    assert!(
        !html.contains(r#"<section id="classes""#),
        "html_schema_sections = false omits the schema reference"
    );

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "docs/"
html_page_layout = "sideways"
"#,
    )
    .expect("write manifest");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("Failed to execute panschema");
    assert!(!output.status.success(), "an unknown layout must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("sideways") && stderr.contains("instances-first"),
        "the error names the offending and accepted values; got: {stderr}"
    );
}

/// A repository that authors *data* against a schema published elsewhere: the
/// schema arrives through `[schemas]`, the A-boxes are local, and
/// `[generate.<name>].instances` renders the imported schema's docs featuring
/// them (ADR-009 decision 6).
/// `--version` has to distinguish a build from `main` from the last release,
/// or "rebuild from `main` to get the fix" is advice a consumer can't verify.
/// A tagged release (or a crates.io install, which has no git at all) reports
/// the bare version; anything else appends the commit it was built from.
#[test]
fn version_identifies_a_non_release_build_by_commit() {
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("--version")
        .output()
        .expect("run panschema");
    assert!(out.status.success(), "--version should succeed");
    let text = String::from_utf8_lossy(&out.stdout);
    let reported = text
        .trim()
        .strip_prefix("panschema ")
        .unwrap_or_else(|| panic!("unexpected --version output: {text}"));

    let crate_version = env!("CARGO_PKG_VERSION");
    let Some(suffix) = reported.strip_prefix(crate_version) else {
        panic!("--version must start with the crate version; got: {reported}");
    };
    if suffix.is_empty() {
        // A tagged release build, or a source tree with no git — both correct.
        return;
    }
    let sha = suffix
        .strip_prefix(" (")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or_else(|| panic!("build id must read ` (<sha>)`; got: {suffix:?}"));
    assert!(
        sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit()),
        "the build id should be an abbreviated commit sha; got: {sha:?}"
    );
}

#[test]
fn manifest_instances_render_the_local_a_boxes_with_the_imported_schema() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    // The schema is a dependency package, not a local file.
    let pkg = consumer.join("wine-pkg");
    fs::create_dir_all(&pkg).expect("mkdir pkg");
    fs::copy(
        "tests/fixtures/wine_catalog.yaml",
        pkg.join("wine_catalog.yaml"),
    )
    .expect("copy schema");
    fs::write(
        pkg.join("panschema-publish.toml"),
        publish_toml("wine", "1.0.0", "wine_catalog.yaml"),
    )
    .expect("write publish toml");

    // The data is this repository's own.
    let data = consumer.join("data");
    fs::create_dir_all(&data).expect("mkdir data");
    fs::copy(
        "tests/fixtures/wine_instances_preview.yaml",
        data.join("preview.yaml"),
    )
    .expect("copy preview");
    fs::copy(
        "tests/fixtures/wine_instances.yaml",
        data.join("catalog.yaml"),
    )
    .expect("copy catalog");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
wine = { path = "./wine-pkg" }

[generate.wine]
html = "site/"
instances = ["data/preview.yaml", "data/catalog.yaml"]
"#,
    )
    .expect("write manifest");

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("run panschema");
    assert!(
        out.status.success(),
        "manifest-driven generate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let html = fs::read_to_string(consumer.join("site").join("index.html")).expect("read index");
    assert!(
        html.contains(r#"role="tablist""#),
        "both declared A-boxes should sit behind the selector"
    );
    assert!(
        html.contains(">preview") && html.contains(">catalog"),
        "each dataset is labelled by its file stem; got a page without both"
    );
    assert!(
        html.contains("Preview Pinot") && html.contains("Château Morgon"),
        "individuals from both local A-boxes must render"
    );
    assert!(
        html.contains(r#"data-instance-dataset="1" hidden>"#),
        "the first declared dataset opens"
    );
}

/// `resolve_against` closes the cross-graph loop inside one manifest: a
/// benchmark entry's external references must equal IRIs the sibling
/// entry's datasets mint. Resolving references get a counting note; an
/// unresolved one warns and, under `--strict`, fails the run; a sibling
/// that isn't a `[schemas]` entry is a configuration error.
#[test]
fn manifest_resolve_against_checks_cross_graph_references() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let catalog_pkg = consumer.join("catalog-pkg");
    write_pkg(
        &catalog_pkg,
        "catalog",
        "1.0.0",
        "catalog.yaml",
        "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers]\n  Provider:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  providers: {range: Provider, multivalued: true}\n",
    );

    let bench_pkg = consumer.join("bench-pkg");
    write_pkg(
        &bench_pkg,
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, anchors]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  anchors: {range: DomainRecord, multivalued: true}\n",
    );

    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    // Two references into the sibling's namespace, one deliberately
    // outside every declared graph: the outsider must stay unchecked
    // rather than failing the run.
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - cat:aws\n  - https://example.org/catalog/est1\n  - https://schema.org/Thing\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let mut args = vec!["generate"];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .current_dir(consumer)
            .output()
            .expect("run panschema")
    };

    let ok = run(&["--strict"]);
    assert!(
        ok.status.success(),
        "resolving references pass even under --strict: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(
        String::from_utf8_lossy(&ok.stderr)
            .contains("2 of 2 cross-graph reference(s) into `catalog` namespace(s) resolve"),
        "the note counts only sibling-namespace references; got:\n{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    // A reference the sibling doesn't mint: warned, then refused under
    // --strict.
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - https://example.org/catalog/nope\n",
    )
    .unwrap();
    let lax = run(&[]);
    assert!(
        lax.status.success(),
        "without --strict an unresolved reference is a warning: {}",
        String::from_utf8_lossy(&lax.stderr)
    );
    let lax_err = String::from_utf8_lossy(&lax.stderr);
    assert!(
        lax_err.contains("no resolve-against dataset mints")
            && lax_err.contains("0 of 1 cross-graph reference(s)"),
        "the warning names the failure and the note still counts; got:\n{lax_err}"
    );
    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict must fail on an unresolved cross-graph reference"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("do not resolve against"),
        "got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );

    // A sibling declared in [schemas] but carrying no [generate] block is
    // a data state, not a config error: the run says why nothing can
    // resolve and proceeds (strict still fails on the unresolved refs).
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    let scaffolded = run(&[]);
    assert!(
        scaffolded.status.success(),
        "a dataset-less sibling must not block the manifest without --strict: {}",
        String::from_utf8_lossy(&scaffolded.stderr)
    );
    assert!(
        String::from_utf8_lossy(&scaffolded.stderr)
            .contains("resolve_against `catalog` declares no `instances`"),
        "the note names the dataset-less sibling; got:\n{}",
        String::from_utf8_lossy(&scaffolded.stderr)
    );

    // An entry resolving against itself is a configuration error.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
bench = { path = "./bench-pkg" }

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["bench"]
"#,
    )
    .unwrap();
    let self_ref = run(&[]);
    assert!(
        !self_ref.status.success(),
        "resolve_against naming the entry itself is an error"
    );
    assert!(
        String::from_utf8_lossy(&self_ref.stderr).contains("names the entry itself"),
        "got:\n{}",
        String::from_utf8_lossy(&self_ref.stderr)
    );

    // A sibling that isn't declared at all is a configuration error.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
bench = { path = "./bench-pkg" }

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["ghost"]
"#,
    )
    .unwrap();
    let misconfigured = run(&[]);
    assert!(
        !misconfigured.status.success(),
        "an unknown resolve_against target is an error"
    );
    assert!(
        String::from_utf8_lossy(&misconfigured.stderr).contains("not a [schemas] entry"),
        "got:\n{}",
        String::from_utf8_lossy(&misconfigured.stderr)
    );
}

const UNION_CATALOG_SCHEMA: &str = "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers]\n  Provider:\n    slots: [id, weight]\nslots:\n  id: {identifier: true}\n  weight: {range: integer}\n  providers: {range: Provider, multivalued: true}\n";

const UNION_BENCH_SCHEMA: &str = "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, anchors]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  anchors: {range: DomainRecord, multivalued: true}\n";

fn run_in(consumer: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(args)
        .current_dir(consumer)
        .output()
        .expect("run panschema")
}

/// `[check.<name>].instances` and `[generate.<name>].instances` are a
/// union everywhere: conformance covers both lists, and a sibling's
/// minted universe spans everything it declares.
#[test]
fn check_and_generate_instance_lists_are_a_union() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    write_pkg(
        &consumer.join("bench-pkg"),
        "bench",
        "1.0.0",
        "bench.yaml",
        UNION_BENCH_SCHEMA,
    );
    // The generate-listed dataset carries the violation and mints the
    // referenced record; the check-listed one is clean.
    fs::write(
        consumer.join("full.yaml"),
        "id: est1\nproviders:\n  - {id: aws, weight: heavy}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("extra.yaml"),
        "id: est2\nproviders:\n  - {id: gcp}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - cat:aws\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
instances = ["full.yaml"]

[check.catalog]
instances = ["extra.yaml"]

[check.bench]
instances = ["bench-data.yaml"]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    let out = run_in(consumer, &["validate"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("weight") && stderr.contains("expects an integer"),
        "the generate-listed dataset is conformance-checked too; got:\n{stderr}"
    );
    assert!(
        stderr.contains("1 of 1 cross-graph reference(s) into `catalog` namespace(s) resolve"),
        "the sibling's minted universe includes its generate-listed dataset; got:\n{stderr}"
    );
}

/// `generate --strict` checks at least what it ships: a `[check]` list
/// cannot narrow the gate below the `[generate]` datasets.
#[test]
fn generate_strict_checks_what_it_ships() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    write_pkg(
        &consumer.join("bench-pkg"),
        "bench",
        "1.0.0",
        "bench.yaml",
        UNION_BENCH_SCHEMA,
    );
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    // The shipped dataset references a record no catalog dataset mints;
    // the check-listed one is clean.
    fs::write(
        consumer.join("ship.yaml"),
        "id: b1\nanchors:\n  - cat:ghost\n",
    )
    .unwrap();
    fs::write(
        consumer.join("curated.yaml"),
        "id: b2\nanchors:\n  - cat:aws\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["ship.yaml"]

[check.bench]
instances = ["curated.yaml"]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    let out = run_in(consumer, &["generate", "--strict"]);
    assert!(
        !out.status.success(),
        "the shipped dataset's dangling cross-graph reference must fail the gate; got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A dataset that ingests nothing is a reported finding, not a vacuous
/// pass: manifest mode matches flag mode's refusal of non-mapping data.
#[test]
fn a_dataset_that_ingests_nothing_is_not_vacuously_clean() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    fs::write(consumer.join("catalog-data.yaml"), "- id: aws\n").unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        "[schemas]\ncatalog = { path = \"./catalog-pkg\" }\n\n[generate.catalog]\ninstances = [\"catalog-data.yaml\"]\n",
    )
    .unwrap();
    let warned = run_in(consumer, &["validate"]);
    assert!(
        String::from_utf8_lossy(&warned.stderr).contains("mapping"),
        "the non-mapping dataset is reported; got:\n{}",
        String::from_utf8_lossy(&warned.stderr)
    );
    let strict = run_in(consumer, &["validate", "--strict"]);
    assert!(!strict.status.success(), "--strict fails on it");
}

/// `--strict` reports everything before failing: one entry's check
/// failure must not hide another entry's findings.
#[test]
fn strict_reports_all_entries_before_failing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    write_pkg(
        &consumer.join("bench-pkg"),
        "bench",
        "1.0.0",
        "bench.yaml",
        UNION_BENCH_SCHEMA,
    );
    // alpha (bench sorts first): a check failure. zeta (catalog): a
    // conformance violation.
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - cat:ghost\n",
    )
    .unwrap();
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws, weight: heavy}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
bench = { path = "./bench-pkg" }
catalog = { path = "./catalog-pkg" }

[generate.catalog]
instances = ["catalog-data.yaml"]

[check.bench]
instances = ["bench-data.yaml"]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    let strict = run_in(consumer, &["validate", "--strict"]);
    let stderr = String::from_utf8_lossy(&strict.stderr);
    assert!(!strict.status.success());
    assert!(
        stderr.contains("does not resolve") || stderr.contains("no resolve-against dataset mints"),
        "the check finding is reported; got:\n{stderr}"
    );
    assert!(
        stderr.contains("expects an integer"),
        "the later entry's conformance violation is reported too, not hidden \
         by the earlier check failure; got:\n{stderr}"
    );
}

/// A `[check.<name>]` naming no `[schemas]` entry is a configuration
/// error in both commands — the silent-no-op class the `[check]` section
/// exists to eliminate.
#[test]
fn an_orphan_check_entry_is_a_configuration_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    fs::write(consumer.join("catalog-data.yaml"), "id: est1\n").unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[check.bnech]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    for args in [["validate"].as_slice(), ["generate"].as_slice()] {
        let out = run_in(consumer, args);
        assert!(
            !out.status.success(),
            "{args:?} accepts a check entry that nothing will ever run"
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("bnech"),
            "{args:?} names the orphan; got:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// `generate`'s skip note for a check-only entry says where the policy
/// does run, so a generate-gated CI pipeline is not silently uncovered.
#[test]
fn generate_names_the_unrun_check_policy() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    write_pkg(
        &consumer.join("bench-pkg"),
        "bench",
        "1.0.0",
        "bench.yaml",
        UNION_BENCH_SCHEMA,
    );
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - cat:aws\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[check.bench]
instances = ["bench-data.yaml"]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();
    let out = run_in(consumer, &["generate"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[check.bench]") && stderr.contains("validate"),
        "the skip note points at the check policy's home; got:\n{stderr}"
    );
}

/// Bare `validate --strict` promotes the schema-level findings `generate
/// --strict` refuses — the check verb covers what the build verb would
/// reject.
#[test]
fn bare_validate_strict_promotes_schema_level_findings() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("pkg"),
        "g",
        "1.0.0",
        "g.yaml",
        "id: https://example.org/g\nname: g\nclasses:\n  Thing:\n    is_a: Ghost\n    slots: [id]\nslots:\n  id: {identifier: true}\n",
    );
    fs::write(
        consumer.join("panschema.toml"),
        "[schemas]\ng = { path = \"./pkg\" }\n",
    )
    .unwrap();
    let warned = run_in(consumer, &["validate"]);
    assert!(
        warned.status.success(),
        "schema findings warn without --strict: {}",
        String::from_utf8_lossy(&warned.stderr)
    );
    let strict = run_in(consumer, &["validate", "--strict"]);
    assert!(
        !strict.status.success(),
        "the dangling schema reference fails --strict; got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );
}

/// Bare `validate` runs the cross-dataset overlap checks flag mode runs:
/// the same IRI minted by two declared datasets is reported.
#[test]
fn bare_validate_reports_cross_dataset_overlap() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        UNION_CATALOG_SCHEMA,
    );
    fs::write(
        consumer.join("a.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("b.yaml"),
        "id: est2\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        "[schemas]\ncatalog = { path = \"./catalog-pkg\" }\n\n[generate.catalog]\ninstances = [\"a.yaml\", \"b.yaml\"]\n",
    )
    .unwrap();
    let out = run_in(consumer, &["validate"]);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("minted by more than one dataset"),
        "the overlap note flag mode prints appears in manifest mode too; got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Bare `validate` reads the manifest and checks everything declared —
/// conformance, cross-graph resolution, stated absences — writing
/// nothing; `--strict` promotes the warnings.
#[test]
fn bare_validate_checks_the_whole_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let catalog_pkg = consumer.join("catalog-pkg");
    write_pkg(
        &catalog_pkg,
        "catalog",
        "1.0.0",
        "catalog.yaml",
        "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers]\n  Provider:\n    slots: [id, weight, sponsor]\nslots:\n  id: {identifier: true}\n  weight: {range: integer}\n  sponsor: {range: Provider}\n  providers: {range: Provider, multivalued: true}\n",
    );

    let bench_pkg = consumer.join("bench-pkg");
    write_pkg(
        &bench_pkg,
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, unconnected]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  unconnected:\n    range: DomainRecord\n    multivalued: true\n    annotations:\n      asserts_absence:\n        value: null\n",
    );

    // A conformance violation in the catalog (string at an integer slot)
    // and a contradicted absence claim in the bench (gcp cites aws).
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws, weight: heavy}\n  - {id: gcp, sponsor: aws}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nunconnected:\n  - cat:aws\n",
    )
    .unwrap();
    // The bench entry has no [generate] block at all: check policy alone
    // must be reachable without declaring any output.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
instances = ["catalog-data.yaml"]

[check.bench]
instances = ["bench-data.yaml"]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let mut args = vec!["validate"];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .current_dir(consumer)
            .output()
            .expect("run panschema")
    };

    let warned = run(&[]);
    let stderr = String::from_utf8_lossy(&warned.stderr);
    assert!(
        warned.status.success(),
        "warnings do not fail an unpromoted run: {stderr}"
    );
    assert!(
        stderr.contains("weight") && stderr.contains("expects an integer"),
        "the catalog's conformance violation is reported; got:\n{stderr}"
    );
    assert!(
        stderr.contains("stated absence does not hold"),
        "the bench's contradicted claim is reported; got:\n{stderr}"
    );
    assert!(
        !consumer.join("catalog.ttl").exists() && !consumer.join("bench.ttl").exists(),
        "validate writes nothing"
    );

    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict promotes the same findings: {}",
        String::from_utf8_lossy(&strict.stderr)
    );
}

/// `require_namespace_coverage` closes the typo'd-namespace hole: an
/// external reference landing in no covered namespace warns instead of
/// passing as an outside vocabulary, and `--strict` fails on it.
#[test]
fn namespace_coverage_flags_references_outside_every_sibling() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("catalog-pkg"),
        "catalog",
        "1.0.0",
        "catalog.yaml",
        "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers]\n  Provider:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  providers: {range: Provider, multivalued: true}\n",
    );
    write_pkg(
        &consumer.join("bench-pkg"),
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, anchors]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  anchors: {range: DomainRecord, multivalued: true}\n",
    );
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n",
    )
    .unwrap();
    // One anchor resolves into the covered namespace; the other lands in a
    // typo'd lookalike that no sibling owns.
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nanchors:\n  - cat:aws\n  - https://example.org/catalog-typo/gcp\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
instances = ["catalog-data.yaml"]

[check.bench]
instances = ["bench-data.yaml"]
resolve_against = ["catalog"]
require_namespace_coverage = true
"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let mut args = vec!["validate"];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .current_dir(consumer)
            .output()
            .expect("run panschema")
    };
    let warned = run(&[]);
    let stderr = String::from_utf8_lossy(&warned.stderr);
    assert!(
        warned.status.success(),
        "warnings only without --strict: {stderr}"
    );
    assert!(
        stderr.contains("catalog-typo") && stderr.contains("no namespace covered"),
        "the uncovered reference is named; got:\n{stderr}"
    );
    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict fails on the uncovered reference"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("land in no covered namespace"),
        "got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );
}

/// The cross-graph keys live under `[check.<name>]`; the old
/// `[generate.<name>]` placement is a parse error, not a silent no-op.
#[test]
fn resolve_keys_under_generate_are_a_parse_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    let pkg = consumer.join("pkg");
    write_pkg(
        &pkg,
        "g",
        "1.0.0",
        "g.yaml",
        "id: https://example.org/g\nname: g\nclasses:\n  Root:\n    tree_root: true\n    slots: [id]\nslots:\n  id: {identifier: true}\n",
    );
    fs::write(
        consumer.join("panschema.toml"),
        "[schemas]\ng = { path = \"./pkg\" }\n\n[generate.g]\nttl = \"g.ttl\"\nresolve_against = [\"other\"]\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("run panschema");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown field") && stderr.contains("resolve_against"),
        "the misplacement is a parse error naming the key; got:\n{stderr}"
    );
}

#[test]
fn manifest_declines_checks_against_a_collided_sibling() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let catalog_pkg = consumer.join("catalog-pkg");
    write_pkg(
        &catalog_pkg,
        "catalog",
        "1.0.0",
        "catalog.yaml",
        "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers]\n  Provider:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  providers: {range: Provider, multivalued: true}\n",
    );

    let bench_pkg = consumer.join("bench-pkg");
    write_pkg(
        &bench_pkg,
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, unconnected]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  unconnected:\n    range: DomainRecord\n    multivalued: true\n    annotations:\n      asserts_absence:\n        value: null\n",
    );

    // The catalog container reuses a contained record's id, so the sibling
    // loads without a container and its citations are unreliable.
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: aws\nproviders:\n  - {id: aws}\n  - {id: gcp}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nunconnected:\n  - cat:gcp\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("run panschema");
    assert!(
        !out.status.success(),
        "a collided sibling cannot be resolved against"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("collides with a record's id"),
        "the refusal names the collision; got:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The schema-declared absence claim end to end: the `asserts_absence`
/// annotation on the slot drives the check — holds, contradicted,
/// `--strict`-refused — and a manifest still carrying the retired
/// `verify_absences` key is refused.
#[test]
fn schema_declared_absence_claims_are_checked() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let catalog_pkg = consumer.join("catalog-pkg");
    write_pkg(
        &catalog_pkg,
        "catalog",
        "1.0.0",
        "catalog.yaml",
        "id: https://example.org/catalog\nname: catalog\ndefault_prefix: cat\nprefixes:\n  cat: https://example.org/catalog/\nclasses:\n  Estate:\n    tree_root: true\n    slots: [id, providers, pairings]\n  Provider:\n    slots: [id]\n  Pairing:\n    slots: [id, a, b]\nslots:\n  id: {identifier: true}\n  providers: {range: Provider, multivalued: true}\n  pairings: {range: Pairing, multivalued: true}\n  a: {range: Provider}\n  b: {range: Provider}\n",
    );

    let bench_pkg = consumer.join("bench-pkg");
    write_pkg(
        &bench_pkg,
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\n  cat: https://example.org/catalog/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, questions]\n  Question:\n    slots: [id, unconnected, unreferenced]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  questions: {range: Question, multivalued: true}\n  unconnected:\n    range: DomainRecord\n    multivalued: true\n    annotations:\n      asserts_absence:\n        value: null\n  unreferenced:\n    range: uri\n    multivalued: true\n    annotations:\n      asserts_absence:\n        value: null\n",
    );

    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n  - {id: gcp}\n  - {id: silent}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("bench-data.yaml"),
        "id: b1\nquestions:\n  - {id: q1, unconnected: [cat:aws, cat:gcp], unreferenced: [cat:silent]}\n",
    )
    .unwrap();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["catalog"]
"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let mut args = vec!["generate"];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .current_dir(consumer)
            .output()
            .expect("run panschema")
    };

    let holds = run(&["--strict"]);
    assert!(
        holds.status.success(),
        "no pairing joins the anchors, so the claim holds: {}",
        String::from_utf8_lossy(&holds.stderr)
    );
    assert!(
        String::from_utf8_lossy(&holds.stderr)
            .contains("2 of 2 stated absence claim(s) hold against `catalog`"),
        "every annotated slot's claim is counted; got:\n{}",
        String::from_utf8_lossy(&holds.stderr)
    );

    // A pairing joining the two anchors contradicts the claim.
    fs::write(
        consumer.join("catalog-data.yaml"),
        "id: est1\nproviders:\n  - {id: aws}\n  - {id: gcp}\n  - {id: silent}\npairings:\n  - {id: p1, a: aws, b: gcp}\n",
    )
    .unwrap();
    let contradicted = run(&[]);
    assert!(
        contradicted.status.success(),
        "without --strict a contradicted claim is a warning: {}",
        String::from_utf8_lossy(&contradicted.stderr)
    );
    assert!(
        String::from_utf8_lossy(&contradicted.stderr).contains("the stated absence does not hold"),
        "the warning names the contradiction; got:\n{}",
        String::from_utf8_lossy(&contradicted.stderr)
    );
    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict must refuse a contradicted absence claim"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("stated absence claim(s) do not hold"),
        "got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );

    // The binding lives on the schema now; a manifest still carrying the
    // retired key is refused, naming it.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]

[check.bench]
resolve_against = ["catalog"]
verify_absences = { slot = "unconnected" }
"#,
    )
    .unwrap();
    let retired_key = run(&[]);
    assert!(
        !retired_key.status.success(),
        "the retired manifest key is refused"
    );
    assert!(
        String::from_utf8_lossy(&retired_key.stderr).contains("verify_absences"),
        "the refusal names the retired key; got:\n{}",
        String::from_utf8_lossy(&retired_key.stderr)
    );

    // Declared claims nothing verifies are noted: without
    // resolve_against, the consumer would otherwise silently weaken the
    // contract to nothing.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
catalog = { path = "./catalog-pkg" }
bench = { path = "./bench-pkg" }

[generate.catalog]
ttl = "catalog.ttl"
instances = ["catalog-data.yaml"]

[generate.bench]
ttl = "bench.ttl"
instances = ["bench-data.yaml"]
"#,
    )
    .unwrap();
    let unverified = run(&[]);
    assert!(
        unverified.status.success(),
        "declared-but-unverified claims are a note, not an error: {}",
        String::from_utf8_lossy(&unverified.stderr)
    );
    assert!(
        String::from_utf8_lossy(&unverified.stderr).contains("declares 2 absence-claim slot(s)"),
        "the note counts the unverified declarations; got:\n{}",
        String::from_utf8_lossy(&unverified.stderr)
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// A defective `asserts_absence` declaration — here a `via_slot` no
/// class carries — warns at load and fails `--strict`, on the generate
/// path and the bare-validate path alike.
#[test]
fn a_defective_absence_declaration_fails_strict() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    let pkg = consumer.join("pkg");
    write_pkg(
        &pkg,
        "bench",
        "1.0.0",
        "bench.yaml",
        "id: https://example.org/bench\nname: bench\ndefault_prefix: bench\nprefixes:\n  bench: https://example.org/bench/\nclasses:\n  Bench:\n    tree_root: true\n    slots: [id, unconnected]\n  DomainRecord:\n    slots: [id]\nslots:\n  id: {identifier: true}\n  unconnected:\n    range: DomainRecord\n    multivalued: true\n    annotations:\n      asserts_absence:\n        value:\n          via_slot: ghost\n",
    );
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
bench = { path = "./pkg" }

[generate.bench]
ttl = "bench.ttl"
"#,
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(args)
            .current_dir(consumer)
            .output()
            .expect("run panschema")
    };
    let warned = run(&["generate"]);
    assert!(
        warned.status.success(),
        "without --strict the defect is a warning: {}",
        String::from_utf8_lossy(&warned.stderr)
    );
    assert!(
        String::from_utf8_lossy(&warned.stderr).contains("asserts_absence"),
        "the warning names the declaration; got:\n{}",
        String::from_utf8_lossy(&warned.stderr)
    );
    for args in [&["generate", "--strict"][..], &["validate", "--strict"][..]] {
        let strict = run(args);
        assert!(
            !strict.status.success(),
            "`{}` refuses the defective declaration",
            args.join(" ")
        );
    }
    let _ = fs::remove_dir_all(&tmp);
}

/// A manifest naming instance data that isn't there fails loudly, naming the
/// schema and the path, rather than quietly publishing a T-box-only page.
#[test]
fn manifest_instances_path_that_does_not_exist_fails() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "docs/"
instances = ["data/absent.yaml"]
"#,
    )
    .expect("write manifest");

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("run panschema");
    assert!(
        !out.status.success(),
        "a missing instances path must fail the build"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("sample_schema") && stderr.contains("absent.yaml"),
        "the error should name the schema and the missing file; got: {stderr}"
    );
}

/// `panschema generate` against a manifest that lists `[schemas]` but
/// has NO `[generate.<name>]` blocks prints a "No outputs generated"
/// hint and still exits cleanly. Catches the `!produced_anything`
/// guard from flipping to `produced_anything` (which would print the
/// hint only when outputs WERE generated — exact-opposite bug).
#[test]
fn manifest_driven_generate_prints_hint_when_no_generate_block() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_sample_pkg(consumer, "sample-pkg");
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No outputs generated"),
        "stderr should suggest adding a generate block; got:\n{stderr}"
    );
}

/// `panschema generate --schema X --format html` (without `--no-graph`)
/// prints a "Graph visualization:" line to stderr describing the viz
/// mode. Catches the `format == "html" && !no_graph` predicate from
/// being inverted or flipped to `||`.
#[test]
fn cli_generate_html_prints_graph_visualization_mode() {
    let output_dir = std::env::temp_dir().join("panschema_viz_mode_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("mkdir");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Graph visualization:"),
        "html format without --no-graph should announce the viz mode; got:\n{stderr}"
    );

    // Inverse: with `--no-graph`, the announcement is suppressed.
    let output_dir2 = std::env::temp_dir().join("panschema_viz_mode_test_2");
    let _ = fs::remove_dir_all(&output_dir2);
    fs::create_dir_all(&output_dir2).expect("mkdir");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir2.to_str().unwrap(),
            "--no-graph",
        ])
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Graph visualization:"),
        "--no-graph should suppress the viz mode announcement; got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&output_dir);
    let _ = fs::remove_dir_all(&output_dir2);
}

/// `panschema generate --format ttl` (or `rust`, or any format that
/// projects neither construct) for a schema with `rules` and `unique_keys`
/// warns that neither will appear in that output — both are IR-modeled (so
/// the unmodeled-construct guard stays silent), but the requested writer
/// doesn't project them, and that gap must not be silent either. The warning
/// names the format actually requested — an earlier version of this warning
/// hardcoded "RDF/OWL" even when the requested format was `rust`, which
/// has nothing to do with RDF. `--format html` gets no such warning,
/// since the HTML writer renders both.
#[test]
fn cli_generate_non_html_warns_unprojected_constructs() {
    let schema_yaml = r#"
id: https://example.org/unprojected-gap
name: unprojected_gap
default_range: string
classes:
  Deployment:
    attributes:
      status:
        range: string
    rules:
      - description: an actual deployment must name its environment
        preconditions:
          slot_conditions:
            status:
              equals_string: actual
  Offering:
    attributes:
      service_type:
        range: string
      offered_by:
        range: string
    unique_keys:
      k:
        unique_key_slots: [service_type, offered_by]
"#;
    let tmp = std::env::temp_dir().join("panschema_unprojected_gap_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.yaml");
    fs::write(&schema_path, schema_yaml).unwrap();

    for format in ["ttl", "rust"] {
        let out_path = tmp.join(format!("out_{format}"));
        let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args([
                "generate",
                "--schema",
                schema_path.to_str().unwrap(),
                "--output",
                out_path.to_str().unwrap(),
                "--format",
                format,
            ])
            .output()
            .expect("panschema");
        assert!(output.status.success(), "format {format} should succeed");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Deployment") && stderr.contains("rules"),
            "{format} should warn that Deployment's rules aren't emitted; got:\n{stderr}"
        );
        assert!(
            stderr.contains("Offering") && stderr.contains("unique_keys"),
            "{format} should warn that Offering's unique_keys aren't emitted; got:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("`{format}`")),
            "{format}'s warning must name the actually-requested format; got:\n{stderr}"
        );
        assert!(
            stderr.lines().any(|l| l.contains("declares `rules`")
                && l.contains("the `shacl` format carries them as shapes")),
            "{format}'s rules warning points at the projection that carries them; got:\n{stderr}"
        );
        assert!(
            stderr
                .lines()
                .any(|l| l.contains("declares `unique_keys`") && !l.contains("shacl")),
            "the shapes pointer belongs to `rules` alone; got:\n{stderr}"
        );
        assert!(
            !stderr.contains("RDF/OWL"),
            "{format}'s warning must not hardcode RDF/OWL; got:\n{stderr}"
        );
    }

    let html_output = tmp.join("html_out");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            html_output.to_str().unwrap(),
            "--format",
            "html",
        ])
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("does not emit"),
        "html format renders both rules and unique_keys, so it must not warn about the gap; got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// `panschema generate --strict` exits non-zero on a dangling reference (here
/// a slot `range` naming no class/enum/type/primitive), not just warns. The
/// same schema without `--strict` succeeds with a warning naming the missing
/// reference.
#[test]
fn cli_generate_strict_fails_on_a_dangling_reference() {
    let schema_yaml = r#"
id: https://example.org/dangling
name: dangling
classes:
  Order:
    slots: [ships_to]
slots:
  ships_to:
    range: Warehouse
"#;
    let tmp = std::env::temp_dir().join("panschema_strict_dangling_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.yaml");
    fs::write(&schema_path, schema_yaml).unwrap();

    let run = |extra: &[&str]| {
        let out_path = tmp.join("out");
        let mut args = vec![
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
            "--format",
            "ttl",
        ];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .output()
            .expect("panschema")
    };

    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict must fail on a dangling reference"
    );
    let strict_err = String::from_utf8_lossy(&strict.stderr);
    assert!(
        strict_err.contains("Warehouse"),
        "the failure must name the missing reference; got:\n{strict_err}"
    );

    let lax = run(&[]);
    assert!(
        lax.status.success(),
        "without --strict, a dangling reference is only a warning"
    );
    assert!(
        String::from_utf8_lossy(&lax.stderr).contains("Warehouse"),
        "without --strict, the dangling reference must still warn"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// A LinkML YAML schema that declares no `default_range` means
/// `default_range: string` per LinkML's derivation rules, so a rangeless
/// slot is string-typed everywhere — including the validator, which
/// rejects a wrong-kinded value at it exactly as it would under a declared
/// default.
#[test]
fn cli_validate_kind_checks_a_rangeless_slot_via_the_implicit_string_default() {
    let tmp = std::env::temp_dir().join("panschema_implicit_default_range_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.yaml");
    fs::write(
        &schema_path,
        "id: https://example.org/implicit\nname: implicit\nclasses:\n  Event:\n    tree_root: true\n    slots: [events]\n  Item:\n    slots: [id, note]\nslots:\n  id: {identifier: true}\n  events: {range: Item, multivalued: true}\n  note: {}\n",
    )
    .unwrap();
    let data_path = tmp.join("data.yaml");
    fs::write(&data_path, "events:\n  - id: i1\n    note: 42\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "validate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--data",
            data_path.to_str().unwrap(),
        ])
        .output()
        .expect("panschema");
    assert!(
        !out.status.success(),
        "an integer at an implicitly string-typed slot must not validate clean"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("expects a string"),
        "the report names the implicit expectation; got:\n{err}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// A rangeless Turtle property keeps its untyped-slot warning and its
/// `--strict` refusal when the Turtle file is *imported* by a YAML root —
/// the root's default (implicit or declared) never reaches a slot whose
/// own file carries no default, so a mixed schema behaves as the Turtle
/// file does standalone.
#[test]
fn cli_generate_strict_fails_on_a_rangeless_property_in_an_imported_turtle_file() {
    let tmp = std::env::temp_dir().join("panschema_strict_imported_ttl_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(
        tmp.join("vocab.ttl"),
        r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex: <https://example.org/vocab#> .

ex: a owl:Ontology .
ex:Widget a owl:Class .
ex:label a owl:DatatypeProperty ;
    rdfs:domain ex:Widget .
"#,
    )
    .unwrap();
    let schema_path = tmp.join("root.yaml");
    fs::write(
        &schema_path,
        "id: https://example.org/root\nname: root\nimports:\n  - vocab\n",
    )
    .unwrap();

    let out_path = tmp.join("out");
    let strict = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
            "--format",
            "ttl",
            "--strict",
        ])
        .output()
        .expect("panschema");
    assert!(
        !strict.status.success(),
        "--strict must refuse the imported rangeless property"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("untyped slot"),
        "the failure must count the untyped slots; got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// An OWL/Turtle property without `rdfs:range` is an untyped slot like any
/// other: the outputs disagree on what it means, so `--strict` refuses it
/// and the warning's advice covers the Turtle spelling (`rdfs:range`), not
/// only YAML's. Without `--strict` it stays a warning.
#[test]
fn cli_generate_strict_fails_on_a_rangeless_turtle_property() {
    let schema_ttl = r#"
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex: <https://example.org/untyped#> .

ex: a owl:Ontology .
ex:Server a owl:Class .
ex:nickname a owl:DatatypeProperty ;
    rdfs:domain ex:Server .
"#;
    let tmp = std::env::temp_dir().join("panschema_strict_untyped_ttl_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.ttl");
    fs::write(&schema_path, schema_ttl).unwrap();

    let run = |extra: &[&str]| {
        let out_path = tmp.join("out");
        let mut args = vec![
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
            "--format",
            "ttl",
        ];
        args.extend_from_slice(extra);
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(&args)
            .output()
            .expect("panschema")
    };

    let strict = run(&["--strict"]);
    assert!(
        !strict.status.success(),
        "--strict must fail on a rangeless property"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("untyped slot"),
        "the failure must count the untyped slots; got:\n{}",
        String::from_utf8_lossy(&strict.stderr)
    );

    let lax = run(&[]);
    assert!(
        lax.status.success(),
        "without --strict, an untyped slot is only a warning"
    );
    let lax_err = String::from_utf8_lossy(&lax.stderr);
    assert!(
        lax_err.contains("nickname") && lax_err.contains("rdfs:range"),
        "the warning must name the slot and the Turtle remediation; got:\n{lax_err}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// `panschema generate` for a schema whose `unique_keys` names a slot the
/// class doesn't have warns about the unresolved reference — a structural
/// defect that would otherwise render a broken constraint silently. A key
/// naming only real slots produces no such warning.
#[test]
fn cli_generate_warns_unresolved_unique_key_slot() {
    let schema_yaml = r#"
id: https://example.org/unique-key-gap
name: unique_key_gap
default_range: string
classes:
  Offering:
    attributes:
      service_type:
        range: string
    unique_keys:
      k:
        unique_key_slots: [service_type, ghost]
"#;
    let tmp = std::env::temp_dir().join("panschema_unique_key_gap_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let schema_path = tmp.join("schema.yaml");
    fs::write(&schema_path, schema_yaml).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            schema_path.to_str().unwrap(),
            "--output",
            tmp.join("out").to_str().unwrap(),
        ])
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ghost") && stderr.contains("Offering") && stderr.contains("`k`"),
        "expected a warning naming the unresolved key slot; got:\n{stderr}"
    );
    assert!(
        !stderr.contains("service_type"),
        "a resolved key slot must not warn; got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

/// `panschema generate` fans out across every populated writer key in
/// `[generate.<name>]` — running `html` and `rust` in one invocation.
#[test]
fn manifest_driven_generate_runs_html_and_rust_for_path_source() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_sample_pkg(consumer, "sample-pkg");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "docs/"
rust = "src/generated/sample.rs"
"#,
    )
    .expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let html_index = consumer.join("docs").join("index.html");
    assert!(
        html_index.exists(),
        "expected html output at {}",
        html_index.display()
    );

    let rust_out = consumer.join("src").join("generated").join("sample.rs");
    assert!(
        rust_out.exists(),
        "expected rust output at {}",
        rust_out.display()
    );
    let body = fs::read_to_string(&rust_out).expect("read generated.rs");
    assert!(
        body.contains("@generated by panschema"),
        "rust output missing generated marker; got:\n{body}"
    );
}

/// `panschema generate` fans out across every writer key configurable in
/// `[generate.<name>]` — not just html/rust — so a consumer gets Postgres
/// DDL, SHACL shapes, the RDF family, and graph JSON from the same manifest
/// that gets its Rust types.
#[test]
fn manifest_driven_generate_runs_every_configured_writer() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_sample_pkg(consumer, "sample-pkg");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
html = "out/docs/"
rust = "out/schema.rs"
postgres = "out/schema.sql"
shacl = "out/shapes.shacl.ttl"
ttl = "out/schema.ttl"
jsonld = "out/schema.jsonld"
rdfxml = "out/schema.rdf"
ntriples = "out/schema.nt"
graph-json = "out/graph.json"
"#,
    )
    .expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    // Every configured format lands its file, each carrying a
    // schema-specific token proving real content — not an empty stub.
    let out = consumer.join("out");
    for (rel, needle) in [
        ("docs/index.html", "Person"),
        ("schema.rs", "Person"),
        ("schema.sql", "person"),
        ("shapes.shacl.ttl", "NodeShape"),
        ("schema.ttl", "Person"),
        ("schema.jsonld", "Person"),
        ("schema.rdf", "Person"),
        ("schema.nt", "Person"),
        ("graph.json", "Person"),
    ] {
        let path = out.join(rel);
        let body = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("expected output at {}: {e}", path.display()));
        assert!(
            body.contains(needle),
            "{rel} missing `{needle}`; got:\n{body}"
        );
    }
}

/// A layering app whose own schema `imports:` a sibling `[schemas]`
/// dependency by name resolves that dependency across the package boundary
/// (not as a local file) and merges it, so the app's generated Rust
/// contains both its own and the imported types.
#[test]
fn manifest_driven_generate_resolves_cross_package_import_by_dependency_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Base package: a Widget class in its own namespace.
    let base = root.join("base-pkg");
    write_pkg(
        &base,
        "base",
        "1.0.0",
        "base.yaml",
        r#"
name: base
id: https://example.org/base
prefixes:
  linkml: https://w3id.org/linkml/
  base: https://example.org/base/
default_range: string
classes:
  Widget:
    attributes:
      label:
        range: string
"#,
    );

    // App package: a Gadget class referencing base's Widget, importing base
    // by its dependency name (the `[schemas]` key), not a local path.
    let app = root.join("app-pkg");
    write_pkg(
        &app,
        "app",
        "1.0.0",
        "app.yaml",
        r#"
name: app
id: https://example.org/app
imports:
  - base
prefixes:
  linkml: https://w3id.org/linkml/
  app: https://example.org/app/
default_range: string
classes:
  Gadget:
    attributes:
      name:
        range: string
      widget:
        range: Widget
"#,
    );

    fs::write(
        root.join("panschema.toml"),
        r#"
[schemas]
app = { path = "./app-pkg" }
base = { path = "./base-pkg" }

[generate.app]
rust = "out/app.rs"
"#,
    )
    .expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(root)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema exited with error");

    let rust_out = root.join("out").join("app.rs");
    let body = fs::read_to_string(&rust_out).expect("read app.rs");
    // The app's own class and the cross-package imported class are both
    // present, and the imported type is Rust-usable (Gadget references it).
    assert!(
        body.contains("struct Gadget"),
        "app's own class missing; got:\n{body}"
    );
    assert!(
        body.contains("struct Widget"),
        "cross-package imported class missing; got:\n{body}"
    );
}

/// An `imports:` entry that is neither a local file nor a declared
/// `[schemas]` dependency fails with a diagnostic that names the entry and
/// points at the package workflow — never a silent drop of the import.
#[test]
fn manifest_driven_generate_diagnoses_undeclared_cross_package_import() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    let app = root.join("app-pkg");
    write_pkg(
        &app,
        "app",
        "1.0.0",
        "app.yaml",
        r#"
name: app
id: https://example.org/app
imports:
  - ghost
default_range: string
classes:
  Gadget:
    attributes:
      name:
        range: string
"#,
    );

    fs::write(
        root.join("panschema.toml"),
        r#"
[schemas]
app = { path = "./app-pkg" }

[generate.app]
rust = "out/app.rs"
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(root)
        .output()
        .expect("Failed to execute panschema");

    assert!(
        !output.status.success(),
        "an undeclared import must fail the command, not drop silently"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ghost"),
        "diagnostic must name the unresolved entry; got:\n{stderr}"
    );
    assert!(
        stderr.contains("panschema fetch") && stderr.contains("[schemas]"),
        "diagnostic must point at the package workflow; got:\n{stderr}"
    );
}

/// A layering app importing two sibling schemas that both import a common
/// base (a diamond) merges every schema once: the app's own classes, both
/// siblings, and the shared base — the base deduplicated, no spurious
/// collision — and cross-import references resolve.
#[test]
fn manifest_driven_generate_merges_a_diamond_of_cross_package_imports() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    write_pkg(
        &root.join("base-pkg"),
        "base",
        "1.0.0",
        "base.yaml",
        "name: base\nid: https://example.org/base\ndefault_range: string\n\
         classes:\n  Base:\n    attributes:\n      a:\n        range: string\n",
    );
    // Two siblings, each importing base and referencing Base.
    write_pkg(
        &root.join("dep1-pkg"),
        "dep1",
        "1.0.0",
        "dep1.yaml",
        "name: dep1\nid: https://example.org/dep1\ndefault_range: string\n\
         imports:\n  - base\nclasses:\n  Dep1:\n    attributes:\n      b:\n        range: Base\n",
    );
    write_pkg(
        &root.join("dep2-pkg"),
        "dep2",
        "1.0.0",
        "dep2.yaml",
        "name: dep2\nid: https://example.org/dep2\ndefault_range: string\n\
         imports:\n  - base\nclasses:\n  Dep2:\n    attributes:\n      c:\n        range: Base\n",
    );
    // App imports both siblings and references each.
    write_pkg(
        &root.join("app-pkg"),
        "app",
        "1.0.0",
        "app.yaml",
        "name: app\nid: https://example.org/app\ndefault_range: string\n\
         imports:\n  - dep1\n  - dep2\nclasses:\n  App:\n    attributes:\n      \
         d1:\n        range: Dep1\n      d2:\n        range: Dep2\n",
    );

    fs::write(
        root.join("panschema.toml"),
        r#"
[schemas]
app = { path = "./app-pkg" }
dep1 = { path = "./dep1-pkg" }
dep2 = { path = "./dep2-pkg" }
base = { path = "./base-pkg" }

[generate.app]
rust = "out/app.rs"
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(root)
        .output()
        .expect("Failed to execute panschema");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "diamond generate failed; stderr:\n{stderr}"
    );

    let body = fs::read_to_string(root.join("out").join("app.rs")).expect("read app.rs");
    for class in ["struct App", "struct Dep1", "struct Dep2", "struct Base"] {
        assert!(
            body.contains(class),
            "diamond output missing `{class}`; got:\n{body}"
        );
    }
    // The shared base is merged once, not duplicated by the two importers
    // (no class name starts with "Base" other than `Base` itself).
    assert_eq!(
        body.matches("struct Base").count(),
        1,
        "shared base class must appear exactly once; got:\n{body}"
    );
    // A deduplicated diamond is silent — no incompatible-collision warning.
    assert!(
        !stderr.contains("defined differently"),
        "diamond dedup must not warn of a collision; stderr:\n{stderr}"
    );
}

/// Two dependencies that define the same element differently have no
/// principled winner (neither is the importing app), so a merge would be
/// order-dependent. The command must fail, naming both sources and the
/// element — never silently pick one by import order.
#[test]
fn manifest_driven_generate_errors_on_conflicting_cross_package_definitions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Two deps define `Shared` incompatibly; neither is the app.
    write_pkg(
        &root.join("dep1-pkg"),
        "dep1",
        "1.0.0",
        "dep1.yaml",
        "name: dep1\nid: https://example.org/dep1\ndefault_range: string\n\
         classes:\n  Shared:\n    description: from dep1\n    attributes:\n      a:\n        range: string\n",
    );
    write_pkg(
        &root.join("dep2-pkg"),
        "dep2",
        "1.0.0",
        "dep2.yaml",
        "name: dep2\nid: https://example.org/dep2\ndefault_range: string\n\
         classes:\n  Shared:\n    description: from dep2 (incompatible)\n    attributes:\n      b:\n        range: integer\n",
    );
    write_pkg(
        &root.join("app-pkg"),
        "app",
        "1.0.0",
        "app.yaml",
        "name: app\nid: https://example.org/app\ndefault_range: string\n\
         imports:\n  - dep1\n  - dep2\nclasses:\n  App:\n    attributes:\n      x:\n        range: string\n",
    );

    fs::write(
        root.join("panschema.toml"),
        r#"
[schemas]
app = { path = "./app-pkg" }
dep1 = { path = "./dep1-pkg" }
dep2 = { path = "./dep2-pkg" }

[generate.app]
rust = "out/app.rs"
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(root)
        .output()
        .expect("Failed to execute panschema");

    assert!(
        !output.status.success(),
        "a dep-vs-dep definitional conflict must fail, not silently pick by import order"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Shared"),
        "error must name the conflicting element; got:\n{stderr}"
    );
    assert!(
        stderr.contains("dep1.yaml") && stderr.contains("dep2.yaml"),
        "error must name both conflicting sources; got:\n{stderr}"
    );
}

/// `panschema generate` with only a `rust` writer (no `html`) still
/// produces the rust file. Locks in the fan-out is independent per writer.
#[test]
fn manifest_driven_generate_runs_rust_writer_alone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_sample_pkg(consumer, "sample-pkg");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
rust = "sample.rs"
"#,
    )
    .expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success());

    let rust_out = consumer.join("sample.rs");
    assert!(
        rust_out.exists(),
        "rust output missing at {}",
        rust_out.display()
    );
    let body = fs::read_to_string(&rust_out).expect("read sample.rs");
    assert!(body.contains("@generated by panschema"));
    assert!(body.contains("Schema: sample_schema"));
}

/// `rust_time = "jiff"` in the manifest maps the generated module's
/// temporal fields to jiff types; an unsupported value fails the run
/// with a message naming the key, never a silent chrono fallback.
#[test]
fn manifest_rust_time_selects_the_jiff_mapping_and_rejects_typos() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let pkg = consumer.join("temporal-pkg");
    write_pkg(
        &pkg,
        "temporal",
        "1.0.0",
        "temporal.yaml",
        "name: temporal\nid: https://example.org/temporal\nclasses:\n  Event:\n    attributes:\n      id:\n        identifier: true\n        range: string\n      at:\n        range: datetime\n",
    );

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
temporal = { path = "./temporal-pkg" }

[generate.temporal]
rust = "temporal.rs"
rust_time = "jiff"
"#,
    )
    .expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success());
    let body = fs::read_to_string(consumer.join("temporal.rs")).expect("read temporal.rs");
    assert!(
        body.contains("jiff::Timestamp"),
        "datetime must map to jiff::Timestamp; got:\n{body}"
    );
    assert!(
        !body.contains("chrono::"),
        "no chrono type in a jiff module"
    );

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
temporal = { path = "./temporal-pkg" }

[generate.temporal]
rust = "temporal.rs"
rust_time = "chrono2"
"#,
    )
    .expect("rewrite manifest");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("Failed to execute panschema");
    assert!(
        !output.status.success(),
        "a typo'd rust_time must fail the run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rust_time") && stderr.contains("chrono2"),
        "the error names the key and the bad value; got: {stderr}"
    );
}

/// `generate --check` is the committed-codegen drift gate: it compares a
/// fresh generation against every declared output byte-for-byte, exits
/// non-zero naming what drifted, and writes nothing — a tampered output
/// stays tampered, and a missing one stays missing.
#[test]
fn generate_check_reports_drift_without_writing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
rust = "sample.rs"
"#,
    )
    .expect("write manifest");

    let check = |dir: &Path| {
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(["generate", "--check"])
            .current_dir(dir)
            .output()
            .expect("run panschema generate --check")
    };

    // Missing output: drift.
    let out = check(consumer);
    assert!(!out.status.success(), "a missing declared output is drift");

    // Freshly generated: clean.
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .status()
        .expect("run panschema generate");
    assert!(status.success());
    let out = check(consumer);
    assert!(
        out.status.success(),
        "an up-to-date output passes --check; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Tampered output: drift, named, and not rewritten.
    let rust_out = consumer.join("sample.rs");
    let tampered = format!(
        "{}\n// local edit\n",
        fs::read_to_string(&rust_out).unwrap()
    );
    fs::write(&rust_out, &tampered).expect("tamper output");
    let out = check(consumer);
    assert!(!out.status.success(), "a tampered output is drift");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("sample.rs"),
        "the drifted file is named; got: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&rust_out).expect("read back"),
        tampered,
        "--check must not rewrite the output"
    );
}

/// `panschema fetch` writes a lockfile with one entry per manifested schema;
/// `panschema verify` then succeeds against the unchanged on-disk content.
#[test]
fn fetch_writes_lockfile_and_verify_succeeds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_sample_pkg(consumer, "sample-pkg");
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }
"#,
    )
    .expect("write manifest");

    // fetch: should produce a lockfile.
    let fetch = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .status()
        .expect("run panschema fetch");
    assert!(fetch.success(), "panschema fetch failed");

    let lockfile_path = consumer.join("panschema.lock");
    assert!(lockfile_path.exists(), "lockfile was not created");
    let lockfile_text = fs::read_to_string(&lockfile_path).expect("read lockfile");
    assert!(
        lockfile_text.contains("sample_schema"),
        "lockfile missing schema name: {lockfile_text}"
    );
    assert!(
        lockfile_text.contains(r#"version = "1.0.0""#),
        "lockfile should now record the publish.toml version: {lockfile_text}"
    );
    assert!(
        lockfile_text.contains("sha256:"),
        "lockfile missing checksum prefix: {lockfile_text}"
    );

    // verify: should succeed because nothing changed.
    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .status()
        .expect("run panschema verify");
    assert!(
        verify.success(),
        "panschema verify failed against the just-written lockfile"
    );
}

/// `panschema verify` errors with a diff when the schema content changes
/// after `panschema fetch`.
#[test]
fn verify_detects_schema_drift_after_fetch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    let pkg = write_sample_pkg(consumer, "sample-pkg");
    let schema_file = pkg.join("sample_schema.yaml");
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }
"#,
    )
    .expect("write manifest");

    let fetch = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .status()
        .expect("run fetch");
    assert!(fetch.success());

    // Mutate the schema after fetch.
    let mut content = fs::read_to_string(&schema_file).expect("read schema");
    content.push_str("\n# drift\n");
    fs::write(&schema_file, content).expect("rewrite schema");

    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .output()
        .expect("run verify");
    assert!(
        !verify.status.success(),
        "verify should have failed on drifted content"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    assert!(
        stderr.contains("drift") || stderr.contains("sample_schema"),
        "stderr should explain the drift; got: {stderr}"
    );
}

/// The manager flow (fetch/verify/generate) dispatches input files by
/// extension to the same readers as `--input`. This proves a `.ttl`
/// schema flows end-to-end through the manager, not just YAML.
#[test]
fn manifest_flow_handles_ttl_input() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    // Package shape: dir with publish.toml + a .ttl main file.
    let pkg = consumer.join("ref-pkg");
    fs::create_dir_all(&pkg).expect("mkdir pkg");
    fs::copy("tests/fixtures/reference.ttl", pkg.join("reference.ttl")).expect("copy fixture");
    fs::write(
        pkg.join("panschema-publish.toml"),
        publish_toml("reference", "1.0.0", "reference.ttl"),
    )
    .expect("write publish toml");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
reference = { path = "./ref-pkg" }

[generate.reference]
html = "docs/"
"#,
    )
    .expect("write manifest");

    // fetch + verify should succeed against a TTL source.
    assert!(
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .arg("fetch")
            .current_dir(consumer)
            .status()
            .expect("fetch")
            .success(),
        "fetch failed for TTL source"
    );
    assert!(
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .arg("verify")
            .current_dir(consumer)
            .status()
            .expect("verify")
            .success(),
        "verify failed for TTL source"
    );

    // generate (no --input) should produce HTML from the TTL via OwlReader.
    assert!(
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .arg("generate")
            .current_dir(consumer)
            .status()
            .expect("generate")
            .success(),
        "generate failed for TTL source"
    );

    let html = fs::read_to_string(consumer.join("docs").join("index.html"))
        .expect("read generated index.html");
    assert!(
        html.contains("panschema Reference Ontology"),
        "TTL-sourced HTML missing reference ontology title"
    );
}

/// `panschema fetch` writes one lockfile entry per manifest schema, and
/// `panschema verify` validates all of them in one pass.
#[test]
fn fetch_and_verify_handle_multiple_schemas() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    write_pkg(
        &consumer.join("a-pkg"),
        "a",
        "0.1.0",
        "schema.yaml",
        "id: https://x/a\nname: a\n",
    );
    write_pkg(
        &consumer.join("b-pkg"),
        "b",
        "0.1.0",
        "schema.yaml",
        "id: https://x/b\nname: b\n",
    );

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
a = { path = "./a-pkg" }
b = { path = "./b-pkg" }
"#,
    )
    .expect("write manifest");

    let fetch = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .status()
        .expect("run fetch");
    assert!(fetch.success(), "fetch failed");

    let lockfile_text = fs::read_to_string(consumer.join("panschema.lock")).expect("read lock");
    assert!(
        lockfile_text.contains("name = \"a\""),
        "missing entry a: {lockfile_text}"
    );
    assert!(
        lockfile_text.contains("name = \"b\""),
        "missing entry b: {lockfile_text}"
    );

    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .status()
        .expect("run verify");
    assert!(verify.success(), "verify failed against fresh lockfile");
}

/// Adding a schema to the manifest after `fetch` (without re-fetching) must
/// be detected by `verify`.
#[test]
fn verify_detects_manifest_schema_missing_from_lockfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("a-pkg"),
        "a",
        "0.1.0",
        "schema.yaml",
        "id: https://x/a\nname: a\n",
    );

    // Fetch with one schema.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
a = { path = "./a-pkg" }
"#,
    )
    .expect("write manifest v1");
    let fetch = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .status()
        .expect("fetch");
    assert!(fetch.success());

    // Add a second schema to the manifest WITHOUT refetching.
    write_pkg(
        &consumer.join("b-pkg"),
        "b",
        "0.1.0",
        "schema.yaml",
        "id: https://x/b\nname: b\n",
    );
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
a = { path = "./a-pkg" }
b = { path = "./b-pkg" }
"#,
    )
    .expect("rewrite manifest v2");

    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .output()
        .expect("verify");
    assert!(
        !verify.status.success(),
        "verify should fail when manifest has schema not in lockfile"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    assert!(
        stderr.contains("`b`") && (stderr.contains("not in lockfile") || stderr.contains("fetch")),
        "stderr should call out the missing schema and suggest fetch; got: {stderr}"
    );
}

/// Removing a schema from the manifest after `fetch` (without re-fetching)
/// leaves a stale lockfile entry; `verify` should call it out.
#[test]
fn verify_detects_stale_lockfile_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_pkg(
        &consumer.join("a-pkg"),
        "a",
        "0.1.0",
        "schema.yaml",
        "id: https://x/a\nname: a\n",
    );
    write_pkg(
        &consumer.join("b-pkg"),
        "b",
        "0.1.0",
        "schema.yaml",
        "id: https://x/b\nname: b\n",
    );

    // Fetch with two schemas.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
a = { path = "./a-pkg" }
b = { path = "./b-pkg" }
"#,
    )
    .expect("write manifest v1");
    let fetch = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .status()
        .expect("fetch");
    assert!(fetch.success());

    // Drop b from the manifest WITHOUT refetching.
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
a = { path = "./a-pkg" }
"#,
    )
    .expect("rewrite manifest v2");

    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .output()
        .expect("verify");
    assert!(
        !verify.status.success(),
        "verify should fail with stale lockfile entry"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    assert!(
        stderr.contains("`b`") && stderr.contains("stale"),
        "stderr should call out the stale schema; got: {stderr}"
    );
}

/// `panschema verify` errors when no lockfile exists.
#[test]
fn verify_errors_when_no_lockfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
"#,
    )
    .expect("write manifest");

    let verify = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("verify")
        .current_dir(consumer)
        .output()
        .expect("run verify");
    assert!(
        !verify.status.success(),
        "verify should fail without lockfile"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr);
    assert!(
        stderr.contains("panschema.lock") || stderr.contains("fetch"),
        "stderr should suggest fetch; got: {stderr}"
    );
}

/// Manifest mode errors clearly when a `path:` schema doesn't exist.
#[test]
fn manifest_driven_generate_errors_on_missing_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
ghost = { path = "./does-not-exist" }

[generate.ghost]
html = "docs/"
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("generate")
        .current_dir(consumer)
        .output()
        .expect("Failed to execute panschema");
    assert!(
        !output.status.success(),
        "panschema should have failed on missing path"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not exist") || stderr.contains("ghost"),
        "stderr should explain the missing path; got: {stderr}"
    );
}

/// A path-source package without `panschema-publish.toml` should error
/// at resolve time (not just at fetch time).
#[test]
fn manifest_path_source_errors_on_missing_publish_toml() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    let pkg = consumer.join("naked-pkg");
    fs::create_dir_all(&pkg).expect("mkdir");
    fs::write(pkg.join("schema.yaml"), "name: x\n").expect("write yaml");
    // Note: no panschema-publish.toml.

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
x = { path = "./naked-pkg" }
"#,
    )
    .expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("fetch")
        .current_dir(consumer)
        .output()
        .expect("panschema");
    assert!(!output.status.success(), "expected failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("panschema-publish.toml"),
        "stderr should mention the missing publish file: {stderr}"
    );
}

// ---------------------------------------------------------------------
// Slice 4: `panschema add` CLI tests
//
// Path-source flow is exercised here via CLI subprocess; github-source
// flow lives at the lib level in `panschema::source::tests` (needs
// TarballSource trait injection, which CLI subprocesses can't do).
// ---------------------------------------------------------------------

/// `panschema add ./local-pkg` reads the package's publish.toml, writes
/// an entry to `panschema.toml` under the declared name, adds a starter
/// `[generate.<name>]` block, and runs fetch to produce the lockfile.
#[test]
fn add_path_source_updates_manifest_and_lockfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");

    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("./sample-pkg")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success(), "panschema add exited with error");

    let manifest = fs::read_to_string(consumer.join("panschema.toml")).expect("read manifest");
    assert!(
        manifest.contains("sample_schema"),
        "manifest should contain the publish.toml-declared name: {manifest}"
    );
    // `add` is "declare a dependency" only — `[generate.<name>]` is the
    // user's to write when they want codegen. `generate` itself prints
    // a helpful "no [generate.<name>] block; skipping" message for any
    // schema without one.
    assert!(
        !manifest.contains("[generate.sample_schema]"),
        "add must not auto-write a starter `[generate.<name>]` block: {manifest}"
    );
    assert!(
        consumer.join("panschema.lock").exists(),
        "fetch should have written panschema.lock"
    );
}

/// `--name <alias>` overrides the publish.toml-declared name.
#[test]
fn add_with_name_alias_overrides_inferred_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("./sample-pkg")
        .arg("--name")
        .arg("my-alias")
        .current_dir(consumer)
        .status()
        .expect("panschema");
    assert!(status.success());

    let manifest = fs::read_to_string(consumer.join("panschema.toml")).expect("read manifest");
    assert!(
        manifest.contains("my-alias"),
        "manifest should use the --name alias: {manifest}"
    );
    assert!(
        !manifest.contains("[schemas.sample_schema]"),
        "alias should override the publish.toml name; got: {manifest}"
    );
}

/// Running `panschema add` for a schema that's already present with the
/// same shape is a no-op (no manifest rewrite, fetch still re-runs).
#[test]
fn add_is_idempotent_for_same_shape() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_panschema"))
            .args(args)
            .current_dir(consumer)
            .status()
            .expect("panschema run")
    };
    assert!(run(&["add", "./sample-pkg"]).success());
    let after_first = fs::read_to_string(consumer.join("panschema.toml")).unwrap();

    assert!(run(&["add", "./sample-pkg"]).success());
    let after_second = fs::read_to_string(consumer.join("panschema.toml")).unwrap();
    assert_eq!(
        after_first, after_second,
        "second add of the same shape must not rewrite the manifest"
    );
}

/// `panschema add github:a/b` (no `@version`) errors at the SchemaSpec
/// parse boundary — before any side effect.
#[test]
fn add_errors_when_github_spec_has_no_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("github:x/y")
        .current_dir(consumer)
        .output()
        .expect("panschema run");
    assert!(
        !output.status.success(),
        "add should reject github source without version"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("version"),
        "stderr should explain the missing version: {stderr}"
    );
}

/// Unknown source protocol fails fast.
#[test]
fn add_errors_on_unknown_source_protocol() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("gitlab:foo/bar@0.1.0")
        .current_dir(consumer)
        .output()
        .expect("panschema run");
    assert!(!output.status.success(), "unknown protocol should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("protocol") || stderr.contains("gitlab"),
        "stderr should call out the unknown protocol: {stderr}"
    );
}

/// `panschema add` against a missing manifest must produce an error
/// message that includes a literal copy-paste shell command to create
/// the manifest. The exact wording matters: the previous "Create one"
/// hint proved too vague for first-time consumers.
#[test]
fn add_missing_manifest_error_includes_literal_init_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    // Deliberately *no* panschema.toml here.

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("github:foo/bar@1.0.0")
        .current_dir(consumer)
        .output()
        .expect("panschema run");
    assert!(
        !output.status.success(),
        "add should fail without a manifest"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("echo '[schemas]' > panschema.toml"),
        "stderr should include the copy-paste init command; got: {stderr}"
    );
}

/// `panschema add github:...` against a publish file whose
/// `[files].main` lives in a subdirectory (`schema/<name>.yaml` — the
/// layout `panschema init --from` produces and the producer guide
/// recommends) must succeed.
///
/// Pre-populates the panschema cache with an already-extracted package
/// and points the CLI at it via `PANSCHEMA_CACHE_ROOT`, so the test
/// exercises the post-fetch read-publish-spec path without any network
/// traffic. The regression: `add_schema` previously reached for the
/// publish file via `schema_path.parent()`, which for a subdirectory
/// `main` landed in `schema/` and produced ENOENT on read.
#[test]
fn add_github_source_succeeds_with_subdirectory_main_layout() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path().join("consumer");
    fs::create_dir_all(&consumer).expect("mkdir consumer");
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    // Pre-populate the cache so the github source short-circuits
    // (no network fetch). Cache layout matches
    // `~/.cache/panschema/github/<owner>/<repo>/<version>/<repo>-<version>/`.
    let cache_root = tmp.path().join("cache");
    let pkg_dir = cache_root
        .join("github")
        .join("test-owner")
        .join("scimantic")
        .join("0.1.0")
        .join("scimantic-0.1.0");
    fs::create_dir_all(pkg_dir.join("schema")).expect("mkdir cached schema/");
    fs::write(
        pkg_dir.join("panschema-publish.toml"),
        publish_toml("scimantic", "0.1.0", "schema/scimantic.yaml"),
    )
    .expect("write cached publish.toml");
    fs::write(
        pkg_dir.join("schema").join("scimantic.yaml"),
        "id: https://example.org/scimantic\nname: scimantic\n",
    )
    .expect("write cached schema");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("github:test-owner/scimantic@0.1.0")
        .current_dir(&consumer)
        .env("PANSCHEMA_CACHE_ROOT", &cache_root)
        .output()
        .expect("panschema run");
    assert!(
        output.status.success(),
        "add should succeed for subdirectory-main layout; \
         stdout: {} \nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let manifest = fs::read_to_string(consumer.join("panschema.toml")).expect("read manifest");
    assert!(
        manifest.contains("scimantic"),
        "manifest should record the schema name from publish.toml: {manifest}"
    );
    assert!(
        manifest.contains("github:test-owner/scimantic"),
        "manifest should record the github source: {manifest}"
    );
}

// ---------------------------------------------------------------------
// Slice 4.5: `panschema init` CLI tests (producer-side scaffolding).
// ---------------------------------------------------------------------

/// `panschema init --name X --version Y --main Z` writes a publish.toml
/// with those exact values.
#[test]
fn init_creates_publish_toml_with_explicit_args() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .arg("--name")
        .arg("my-schema")
        .arg("--version")
        .arg("0.3.1")
        .arg("--main")
        .arg("schema.yaml")
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).expect("read");
    assert!(body.contains(r#"name = "my-schema""#));
    assert!(body.contains(r#"version = "0.3.1""#));
    assert!(body.contains(r#"main = "schema.yaml""#));
}

/// `panschema init --from <linkml.yaml>` extracts name + version from the
/// LinkML file's metadata and pre-fills the publish.toml.
#[test]
fn init_from_existing_linkml_yaml_extracts_name_and_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(
        dir.join("my-schema.yaml"),
        "id: https://example.org/x\nname: \"derived-name\"\nversion: \"1.4.2\"\n",
    )
    .expect("write linkml");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .arg("--from")
        .arg("my-schema.yaml")
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success(), "init --from should succeed");

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).expect("read");
    assert!(body.contains(r#"name = "derived-name""#));
    assert!(body.contains(r#"version = "1.4.2""#));
    // --from also defaults `main` to the passed file.
    assert!(body.contains(r#"main = "my-schema.yaml""#));
}

/// `panschema init` with no args uses the CWD's basename + safe defaults.
#[test]
fn init_with_no_args_uses_dirname_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("widget-schema");
    fs::create_dir_all(&dir).expect("mkdir");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .current_dir(&dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).expect("read");
    assert!(
        body.contains(r#"name = "widget-schema""#),
        "default name should be CWD basename; got: {body}"
    );
    assert!(body.contains(r#"version = "0.1.0""#));
    assert!(body.contains(r#"main = "schema.yaml""#));
    assert!(body.contains(r#"linkml = "1.7.0""#));
}

/// Re-running `panschema init` over an existing publish.toml refuses
/// without `--force`.
#[test]
fn init_refuses_clobber_without_force() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("panschema-publish.toml"), "# placeholder\n").expect("seed");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .arg("--name")
        .arg("anything")
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(
        !output.status.success(),
        "init should refuse to overwrite existing file"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists") || stderr.contains("--force"),
        "stderr should mention the clobber refusal: {stderr}"
    );

    // The seed file is intact.
    assert_eq!(
        fs::read_to_string(dir.join("panschema-publish.toml")).unwrap(),
        "# placeholder\n"
    );
}

/// `--force` allows overwriting an existing publish.toml.
#[test]
fn init_force_overwrites_existing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("panschema-publish.toml"), "# placeholder\n").expect("seed");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .arg("--name")
        .arg("real")
        .arg("--version")
        .arg("0.1.0")
        .arg("--main")
        .arg("schema.yaml")
        .arg("--force")
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).expect("read");
    assert!(body.contains(r#"name = "real""#));
    assert!(!body.contains("placeholder"));
}

/// `init` warns when the configured main file doesn't exist yet but still
/// writes the publish.toml (validation is informational).
#[test]
fn init_warns_when_main_file_missing_but_still_writes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("init")
        .arg("--name")
        .arg("x")
        .arg("--version")
        .arg("0.1.0")
        .arg("--main")
        .arg("does-not-exist.yaml")
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(output.status.success(), "init should still succeed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning") && stderr.contains("does-not-exist.yaml"),
        "should print a warning about the missing main file: {stderr}"
    );
    // The two `if`/`else` branches of post-write validation both print
    // a "warning" but with different text: file-missing → "does not
    // exist yet"; reader-parse-failure → wraps the IO/parse error.
    // Asserting on the file-missing-specific phrase pins down WHICH
    // branch fired — so inverting the `!main_full.exists()` predicate
    // is caught even though both branches yield a "warning" stderr.
    assert!(
        stderr.contains("does not exist yet"),
        "should take the file-missing branch, not the parse-error branch: {stderr}"
    );
    assert!(
        dir.join("panschema-publish.toml").exists(),
        "publish.toml should still be written"
    );
}

// ---------------------------------------------------------------------
// Slice 4.6: `panschema release` CLI tests (producer-side version bump).
// ---------------------------------------------------------------------

/// Seed a temp dir with a minimal publish.toml at the given version.
fn seed_publish(dir: &Path, version: &str) {
    fs::write(
        dir.join("panschema-publish.toml"),
        publish_toml("x", version, "schema.yaml"),
    )
    .expect("write publish");
}

/// `release --level patch` bumps the version and prints the suggested
/// git commands; doesn't touch git itself.
#[test]
fn release_bump_only_updates_publish_toml_and_prints_suggestions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.3");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--level")
        .arg("patch")
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(output.status.success(), "release should succeed");

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert!(body.contains(r#"version = "0.1.4""#));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0.1.3 → 0.1.4"),
        "stdout should report the bump: {stdout}"
    );
    assert!(
        stdout.contains("git commit -am 'release: v0.1.4'"),
        "stdout should suggest the git commands: {stdout}"
    );
}

/// `--dry-run` prints the plan but doesn't change any files.
#[test]
fn release_dry_run_does_not_modify_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");
    let before = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--level")
        .arg("minor")
        .arg("--dry-run")
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(output.status.success());

    let after = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert_eq!(before, after, "dry-run must not modify the file");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry run") && stdout.contains("0.1.0 → 0.2.0"));
}

/// `--version <x.y.z>` sets an exact version.
#[test]
fn release_version_arg_sets_exact_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--version")
        .arg("0.5.0-rc1")
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert!(
        body.contains(r#"version = "0.5.0-rc1""#),
        "version arg should land verbatim: {body}"
    );
}

/// `--level major` from a 0.x.y version goes to 1.0.0 (literal semver,
/// matching cargo-release default).
#[test]
fn release_level_major_from_pre_1_0_goes_to_1_0_0() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.5.7");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--level")
        .arg("major")
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert!(body.contains(r#"version = "1.0.0""#));
}

/// `--version` with a non-semver value errors out and doesn't write.
#[test]
fn release_errors_on_invalid_semver_via_version_arg() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--version")
        .arg("not-a-semver")
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(!output.status.success());

    let body = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert!(body.contains(r#"version = "0.1.0""#), "file unchanged");
}

/// `release` errors clearly when there's no publish.toml in CWD.
#[test]
fn release_errors_when_publish_toml_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .arg("--level")
        .arg("patch")
        .current_dir(tmp.path())
        .output()
        .expect("panschema");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("panschema-publish.toml") || stderr.contains("panschema init"),
        "stderr should explain the missing file: {stderr}"
    );
}

/// `release` errors when neither `--level` nor `--version` is passed.
#[test]
fn release_errors_when_neither_level_nor_version_given() {
    let tmp = tempfile::tempdir().expect("tempdir");
    seed_publish(tmp.path(), "0.1.0");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("release")
        .current_dir(tmp.path())
        .output()
        .expect("panschema");
    assert!(!output.status.success());
}

/// `--git` in a clean git repo bumps + commits + tags.
///
/// Skipped automatically if `git` isn't on PATH.
#[test]
fn release_with_git_commits_and_tags() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("skipping: git not available");
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    // Init a git repo + first commit so the working tree is clean.
    Command::new("git")
        .arg("init")
        .arg("-q")
        .arg("-b")
        .arg("main")
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    seed_publish(dir, "0.1.0");
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(dir)
        .status()
        .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch", "--git"])
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success(), "release --git should succeed");

    // Tag should exist.
    let tags = Command::new("git")
        .arg("tag")
        .current_dir(dir)
        .output()
        .unwrap();
    let tag_list = String::from_utf8_lossy(&tags.stdout);
    assert!(
        tag_list.contains("v0.1.1"),
        "expected tag v0.1.1: {tag_list}"
    );

    // Latest commit message should reference the release.
    let log = Command::new("git")
        .args(["log", "-1", "--pretty=%s"])
        .current_dir(dir)
        .output()
        .unwrap();
    let last_msg = String::from_utf8_lossy(&log.stdout);
    assert!(
        last_msg.contains("release: v0.1.1"),
        "expected release commit; got: {last_msg}"
    );
}

/// `--git` refuses when the working tree has uncommitted changes
/// (beyond the bump itself).
#[test]
fn release_with_git_refuses_on_dirty_tree() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    seed_publish(dir, "0.1.0");
    // Untracked file = dirty tree.
    fs::write(dir.join("STRAY.txt"), "uncommitted").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch", "--git"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not clean") || stderr.contains("dirty"),
        "stderr should call out the dirty tree: {stderr}"
    );
}

/// `--git` refuses when the target tag already exists.
#[test]
fn release_with_git_refuses_when_tag_already_exists() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    seed_publish(dir, "0.1.0");
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(dir)
        .status()
        .unwrap();
    // Pre-create the tag we're about to try to make.
    Command::new("git")
        .args(["tag", "v0.1.1"])
        .current_dir(dir)
        .status()
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch", "--git"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "stderr should call out the existing tag: {stderr}"
    );

    // Critical: panschema's check runs BEFORE the publish.toml bump.
    // git itself would error on `git tag v0.1.1` if the check were
    // bypassed, with the same "already exists" message — but by then
    // publish.toml would already be bumped to 0.1.1 and committed.
    // Asserting the version is still 0.1.0 pins down WHICH layer
    // caught the error.
    let publish = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert!(
        publish.contains(r#"version = "0.1.0""#),
        "publish.toml must still be at 0.1.0 — the tag-exists check \
         should reject before the bump:\n{publish}"
    );
}

// ---------------------------------------------------------------------
// Slice 4.7: dogfood-driven fixes to `init` + `release` (2026-05-13).
// ---------------------------------------------------------------------

/// Fix 1: `release --version <V>` when publish.toml is already at V
/// errors out with a clear "nothing to bump" message and doesn't touch
/// any files.
#[test]
fn release_errors_on_noop_bump() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");
    let before = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--version", "0.1.0"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(!output.status.success(), "no-op bump should error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already") && stderr.contains("0.1.0"),
        "stderr should explain the no-op: {stderr}"
    );
    // File untouched.
    let after = fs::read_to_string(dir.join("panschema-publish.toml")).unwrap();
    assert_eq!(before, after);
}

/// Fix 2: tags created by `release --git` are annotated (the only kind
/// `git push --follow-tags` will push).
#[test]
fn release_with_git_creates_annotated_tag() {
    if Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    seed_publish(dir, "0.1.0");
    Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-qm", "initial"])
        .current_dir(dir)
        .status()
        .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch", "--git"])
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success());

    // An annotated tag has `tag` object-type; a lightweight tag points at
    // a commit directly. `git cat-file -t v0.1.1` returns either "tag" or
    // "commit".
    let kind = Command::new("git")
        .args(["cat-file", "-t", "v0.1.1"])
        .current_dir(dir)
        .output()
        .unwrap();
    let kind_str = String::from_utf8_lossy(&kind.stdout);
    assert_eq!(
        kind_str.trim(),
        "tag",
        "expected an annotated tag (so `git push --follow-tags` works); got: {kind_str}"
    );
}

/// Fix 3: refuse to release while the LinkML main file's `version:`
/// field disagrees with publish.toml.
#[test]
fn release_errors_on_linkml_version_drift() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    // publish.toml says 0.1.0...
    seed_publish(dir, "0.1.0");
    // ...but the LinkML main file says 0.9.0.
    fs::write(
        dir.join("schema.yaml"),
        "id: https://example.org/x\nname: x\nversion: \"0.9.0\"\n",
    )
    .expect("write linkml");

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(!output.status.success(), "drift should refuse the release");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("drift") || (stderr.contains("0.1.0") && stderr.contains("0.9.0")),
        "stderr should call out the version disagreement: {stderr}"
    );
}

/// Fix 3 corollary: release proceeds when versions agree.
#[test]
fn release_succeeds_when_linkml_version_matches_publish_toml() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");
    fs::write(
        dir.join("schema.yaml"),
        "id: https://example.org/x\nname: x\nversion: \"0.1.0\"\n",
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch"])
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success(), "matching versions should release cleanly");
}

/// Fix 3 corollary: LinkML files without a declared version skip the
/// drift check (no source of truth to compare).
#[test]
fn release_skips_drift_check_when_linkml_has_no_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    seed_publish(dir, "0.1.0");
    fs::write(
        dir.join("schema.yaml"),
        "id: https://example.org/x\nname: x\n",
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["release", "--level", "patch"])
        .current_dir(dir)
        .status()
        .expect("panschema");
    assert!(status.success(), "no version field → no check → success");
}

/// Fix 4: `panschema init` prints provenance for each field so users
/// can tell what was explicit vs derived from `--from` vs defaulted.
#[test]
fn init_output_shows_field_provenance() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["init", "--name", "explicit-name", "--version", "0.2.0"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("name") && stdout.contains("(explicit)"),
        "stdout should label `name` as explicit: {stdout}"
    );
    assert!(
        stdout.contains("version") && stdout.contains("(explicit)"),
        "stdout should label `version` as explicit: {stdout}"
    );
    assert!(
        stdout.contains("main") && stdout.contains("(default)"),
        "stdout should label `main` as default: {stdout}"
    );
    assert!(
        stdout.contains("linkml") && stdout.contains("default"),
        "stdout should label `linkml` as default: {stdout}"
    );
}

/// End-to-end exercise of the `panschema publish` subcommand: builds
/// a synthetic git repo with a tagged release, writes a manifest with
/// a `[publishing]` block, invokes the CLI, and confirms the per-tag
/// and `current/` outputs land where they should.
///
/// This is the integration-level counterpart to the unit tests in
/// `publish.rs::tests` — those exercise the library function;
/// this one exercises the CLI wrapper that's intentionally
/// `#[mutants::skip]`'d in `main.rs`.
#[test]
fn publish_reports_collisions_across_its_declared_instances_entries() {
    // publish already knows the full declared set, so the cross-dataset check
    // runs without being asked.
    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("git on PATH");
        assert!(status.success(), "git {args:?} failed");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();

    git(repo, &["init", "--initial-branch=main", "--quiet"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(
        repo.join("schema.yaml"),
        "id: https://example.org/v0.1.0\n\
         name: publish_collision_fixture\n\
         version: 0.1.0\n\
         prefixes:\n  schema: https://example.org/\n\
         default_prefix: schema\n\
         default_range: string\n\
         classes:\n\
        \x20 Catalog:\n    tree_root: true\n    attributes:\n\
        \x20     things: {range: Thing, multivalued: true}\n\
        \x20 Thing:\n    attributes:\n      id: {identifier: true}\n",
    )
    .unwrap();
    // Both datasets define `shared` — one individual once loaded together.
    fs::write(repo.join("preview.yaml"), "things:\n  - id: shared\n").unwrap();
    fs::write(
        repo.join("full.yaml"),
        "things:\n  - id: shared\n  - id: extra\n",
    )
    .unwrap();
    fs::write(
        repo.join("panschema-publish.toml"),
        r#"[schema]
name = "publish_collision_fixture"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "Preview"
data = "preview.yaml"

[[instances]]
name = "Worked example"
data = "full.yaml"
exemplar = true

[publishing]
versions = ["v0.1.0"]
current = "v0.1.0"
output_dir = "site"
"#,
    )
    .unwrap();
    git(repo, &["add", "."]);
    git(repo, &["commit", "-m", "release v0.1.0", "--quiet"]);
    git(repo, &["tag", "v0.1.0"]);

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(repo)
        .output()
        .expect("panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "deliberate overlap does not fail a publish; stderr: {stderr}"
    );
    assert!(
        stderr.contains("minted by more than one dataset")
            && stderr.contains("shared")
            && stderr.contains("preview.yaml")
            && stderr.contains("full.yaml"),
        "publish reports the overlap, naming both declared datasets; got: {stderr}"
    );
}

/// Run a git command in `cwd`, asserting success. Shared by the
/// publish fixtures that build tagged repos.
fn publish_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .expect("git on PATH");
    assert!(status.success(), "git {args:?} failed");
}

/// Lay one extracted `contract` package version into a local cache
/// tree, so `github:test-owner/contract` resolves offline via
/// `PANSCHEMA_CACHE_ROOT`. `marker_class` adds a version-identifying
/// class for pin assertions.
fn contract_cache_pkg(cache_root: &Path, version: &str, marker_class: Option<&str>) {
    let pkg = cache_root
        .join("github")
        .join("test-owner")
        .join("contract")
        .join(version)
        .join(format!("contract-{version}"));
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("panschema-publish.toml"),
        format!(
            "[schema]\nname = \"contract\"\nversion = \"{version}\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"contract.yaml\"\n"
        ),
    )
    .unwrap();
    let marker = marker_class
        .map(|m| format!("\x20 {m}:\n    attributes:\n      id: {{identifier: true}}\n"))
        .unwrap_or_default();
    fs::write(
        pkg.join("contract.yaml"),
        format!(
            "id: https://example.org/contract\n\
             name: contract\n\
             version: {version}\n\
             prefixes:\n  contract: https://example.org/contract/\n\
             default_prefix: contract\ndefault_range: string\n\
             classes:\n  Ledger:\n    tree_root: true\n    attributes:\n\
            \x20     records: {{range: Record, multivalued: true}}\n\
            \x20 Record:\n    attributes:\n      id: {{identifier: true}}\n{marker}"
        ),
    )
    .unwrap();
}

/// Initialize a git repo publishing a trivial own schema plus one
/// `records` dataset against the `contract` dependency, ready to tag.
fn init_contract_consumer(repo: &Path, versions: &str) {
    publish_git(repo, &["init", "--initial-branch=main", "--quiet"]);
    publish_git(repo, &["config", "user.email", "test@example.com"]);
    publish_git(repo, &["config", "user.name", "Test"]);
    publish_git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(
        repo.join("schema.yaml"),
        "id: https://example.org/own\nname: own_schema\nversion: 0.1.0\n",
    )
    .unwrap();
    fs::write(repo.join("records.yaml"), "records:\n  - id: r1\n").unwrap();
    fs::write(
        repo.join("panschema-publish.toml"),
        format!(
            r#"[schema]
name = "own_schema"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[[instances]]
name = "records"
data = "records.yaml"
schema = "contract"

[publishing]
versions = [{versions}]
current = "v0.1.0"
output_dir = "site"
"#
        ),
    )
    .unwrap();
    fs::write(
        repo.join("panschema.toml"),
        "[schemas.contract]\nsource = \"github:test-owner/contract\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
}

/// Each published ref renders its dependency page against the
/// dependency version that ref's own manifest pins — resolved from the
/// local cache only, with no network fetch — so a historical page shows
/// the contract as it was.
#[test]
fn publish_renders_each_ref_against_its_pinned_dependency() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let repo = repo.as_path();
    let cache_root = tmp.path().join("cache");
    contract_cache_pkg(&cache_root, "0.1.0", Some("ContractV1"));
    contract_cache_pkg(&cache_root, "0.2.0", Some("ContractV2"));

    init_contract_consumer(repo, "\"v0.1.0\", \"v0.2.0\"");
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "release v0.1.0", "--quiet"]);
    publish_git(repo, &["tag", "v0.1.0"]);
    fs::write(
        repo.join("panschema.toml"),
        "[schemas.contract]\nsource = \"github:test-owner/contract\"\nversion = \"0.2.0\"\n",
    )
    .unwrap();
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "bump contract to 0.2.0", "--quiet"]);
    publish_git(repo, &["tag", "v0.2.0"]);

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(repo)
        .env("PANSCHEMA_CACHE_ROOT", &cache_root)
        .output()
        .expect("panschema");
    assert!(
        out.status.success(),
        "publish must succeed from a warm cache; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let old_page = fs::read_to_string(repo.join("site/contract/v0.1.0/index.html"))
        .expect("dependency page exists at the ref pinning 0.1.0");
    assert!(
        old_page.contains("ContractV1") && !old_page.contains("ContractV2"),
        "the v0.1.0 page renders the dependency as pinned by that ref's manifest"
    );
    let new_page = fs::read_to_string(repo.join("site/contract/v0.2.0/index.html"))
        .expect("dependency page exists at the ref pinning 0.2.0");
    assert!(
        new_page.contains("ContractV2") && !new_page.contains("ContractV1"),
        "the v0.2.0 page renders the dependency as pinned by that ref's manifest"
    );
}

#[test]
fn publish_refuses_cached_content_that_fails_the_refs_lockfile() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let repo = repo.as_path();
    let cache_root = tmp.path().join("cache");
    contract_cache_pkg(&cache_root, "0.1.0", None);
    let good = panschema::lockfile::checksum_file(
        &cache_root
            .join("github/test-owner/contract/0.1.0/contract-0.1.0")
            .join("contract.yaml"),
    )
    .unwrap();

    init_contract_consumer(repo, "\"v0.1.0\", \"v0.2.0\", \"v0.3.0\", \"v0.4.0\"");
    let lock = |version: &str, checksum: &str| {
        format!(
            "[[schema]]\nname = \"contract\"\nversion = \"{version}\"\nsource = \"github:test-owner/contract\"\nchecksum = \"{checksum}\"\n"
        )
    };
    fs::write(repo.join("panschema.lock"), lock("0.1.0", &good)).unwrap();
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "release v0.1.0", "--quiet"]);
    publish_git(repo, &["tag", "v0.1.0"]);
    fs::write(
        repo.join("panschema.lock"),
        lock(
            "0.1.0",
            "sha256:1111111111111111111111111111111111111111111111111111111111111111",
        ),
    )
    .unwrap();
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "drift the lockfile", "--quiet"]);
    publish_git(repo, &["tag", "v0.2.0"]);
    fs::write(repo.join("panschema.lock"), "not = valid = toml").unwrap();
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "break the lockfile", "--quiet"]);
    publish_git(repo, &["tag", "v0.3.0"]);
    fs::write(repo.join("panschema.lock"), lock("0.9.9", &good)).unwrap();
    publish_git(repo, &["add", "."]);
    publish_git(
        repo,
        &["commit", "-m", "stale the lockfile version", "--quiet"],
    );
    publish_git(repo, &["tag", "v0.4.0"]);

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(repo)
        .env("PANSCHEMA_CACHE_ROOT", &cache_root)
        .output()
        .expect("panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "a drifted ref skips its page, never fails the publish; stderr: {stderr}"
    );
    assert!(
        repo.join("site/contract/v0.1.0/index.html").exists(),
        "the ref whose lockfile matches renders its dependency page"
    );
    assert!(
        !repo.join("site/contract/v0.2.0").exists(),
        "the ref whose lockfile disagrees with the cache gets no dependency page"
    );
    assert!(
        stderr.contains("checksum") && stderr.contains("v0.2.0"),
        "the skip note names the checksum mismatch at the drifted ref; got: {stderr}"
    );
    assert!(
        !repo.join("site/contract/v0.3.0").exists()
            && stderr.contains("panschema.lock does not parse"),
        "a committed lockfile that fails to parse refuses the page instead of silently \
         disabling the gate; got: {stderr}"
    );
    assert!(
        !repo.join("site/contract/v0.4.0").exists()
            && stderr.contains("lockfile records version 0.9.9")
            && stderr.contains("stale"),
        "a lock disagreeing with its own ref's manifest is called stale, not blamed on \
         the cache; got: {stderr}"
    );
}

/// A cold cache does not fail the publish, but the skip note carries
/// the resolver's own message — naming `panschema fetch` as the fix —
/// rather than reading like the dependency was never declared.
#[test]
fn publish_with_a_cold_cache_names_the_fetch_fix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let repo = repo.as_path();
    let cache_root = tmp.path().join("empty-cache");

    init_contract_consumer(repo, "\"v0.1.0\"");
    publish_git(repo, &["add", "."]);
    publish_git(repo, &["commit", "-m", "release v0.1.0", "--quiet"]);
    publish_git(repo, &["tag", "v0.1.0"]);

    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(repo)
        .env("PANSCHEMA_CACHE_ROOT", &cache_root)
        .output()
        .expect("panschema");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "a cold cache skips the page, never fails the publish; stderr: {stderr}"
    );
    assert!(
        repo.join("site/v0.1.0/index.html").exists(),
        "the own page still publishes"
    );
    assert!(
        !repo.join("site/contract").exists(),
        "no dependency page can build from a cold cache"
    );
    assert!(
        stderr.contains("panschema fetch"),
        "the skip note names the fix; got: {stderr}"
    );
}

#[test]
fn cli_publish_builds_per_version_subdirs_and_current_alias() {
    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .status()
            .expect("git on PATH");
        assert!(status.success(), "git {args:?} failed");
    }

    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path();

    // Init a synthetic repo with one tagged release. Deterministic
    // identity so commits hash stably across CI runners and the local
    // dev box.
    git(repo, &["init", "--initial-branch=main", "--quiet"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(
        repo.join("schema.yaml"),
        "id: https://example.org/v0.1.0\n\
         name: cli_publish_fixture\n\
         version: 0.1.0\n\
         prefixes:\n  schema: https://example.org/\n\
         default_prefix: schema\n\
         classes:\n  Thing:\n    description: a thing\n",
    )
    .unwrap();
    git(repo, &["add", "schema.yaml"]);
    git(repo, &["commit", "-m", "release v0.1.0", "--quiet"]);
    git(repo, &["tag", "v0.1.0"]);

    // Manifest with [publishing]. Note `current = "v0.1.0"` — that's
    // the only legal value here (no other versions, no edge).
    fs::write(
        repo.join("panschema-publish.toml"),
        r#"[schema]
name = "cli_publish_fixture"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"

[publishing]
versions = ["v0.1.0"]
current = "v0.1.0"
output_dir = "site"
"#,
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(repo)
        .status()
        .expect("panschema");
    assert!(status.success(), "panschema publish exited with error");

    // Per-tag output exists.
    assert!(
        repo.join("site/v0.1.0/index.html").is_file(),
        "expected site/v0.1.0/index.html to exist"
    );
    // current/ alias is a byte-equal copy of the v0.1.0 output.
    let v01 = fs::read(repo.join("site/v0.1.0/index.html")).unwrap();
    let current = fs::read(repo.join("site/current/index.html")).unwrap();
    assert_eq!(
        current, v01,
        "current/index.html must be byte-equal to v0.1.0/index.html"
    );

    // Rendered output carries the version-cohort UX: the dropdown
    // names every cohort member, defaults to this page's version,
    // and the `current` page does NOT show the stale banner.
    let v01_html = String::from_utf8(v01).unwrap();
    assert!(
        v01_html.contains(r#"id="version-select""#),
        "rendered v0.1.0/index.html must include the version-select dropdown"
    );
    assert!(
        v01_html.contains(r#"value="v0.1.0" selected"#),
        "v0.1.0 dropdown must default-select its own version"
    );
    assert!(
        !v01_html.contains(r#"<div class="version-banner version-banner-stale""#),
        "v0.1.0 is the `current` version here; stale banner must not render"
    );
}

/// CLI exit-code contract: `panschema publish` against a manifest
/// without a `[publishing]` section fails fast and the error message
/// names the missing section.
#[test]
fn cli_publish_errors_when_publishing_section_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fs::write(
        tmp.path().join("panschema-publish.toml"),
        publish_toml("x", "0.1.0", "schema.yaml"),
    )
    .unwrap();
    fs::write(tmp.path().join("schema.yaml"), "id: x\nname: x\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("publish")
        .current_dir(tmp.path())
        .output()
        .expect("panschema");
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[publishing]"),
        "stderr should name the missing [publishing] section: {stderr}"
    );
}

/// Fix 4 corollary: `--from` provenance is labeled distinctly.
#[test]
fn init_output_shows_from_provenance_when_from_used() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(
        dir.join("schema.yaml"),
        "id: https://example.org/x\nname: from-name\nversion: \"3.1.4\"\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["init", "--from", "schema.yaml"])
        .current_dir(dir)
        .output()
        .expect("panschema");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("from-name") && stdout.contains("from --from"),
        "stdout should label name as `from --from`: {stdout}"
    );
    assert!(
        stdout.contains("3.1.4") && stdout.contains("from --from"),
        "stdout should label version as `from --from`: {stdout}"
    );
}

/// `panschema generate` on a root schema that `imports:` a local file
/// merges the import in before any writer runs: the generated HTML
/// renders a class card for a class defined only in the imported file,
/// alongside the root's own class.
#[test]
fn generate_merges_single_import() {
    let out_dir = std::env::temp_dir().join("panschema_generate_merges_single_import");
    let _ = fs::remove_dir_all(&out_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--schema",
            "tests/fixtures/imports/app.yaml",
            "--format",
            "html",
            "--no-graph",
            "--offline",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .status()
        .expect("run panschema generate");
    assert!(status.success(), "generate should succeed");

    let html = fs::read_to_string(out_dir.join("index.html")).expect("read index.html");
    assert!(
        html.contains(r##"id="class-Address""##),
        "class defined only in the import should render a card in the merged HTML"
    );
    assert!(
        html.contains(r##"id="class-Customer""##),
        "the root's own class should still render"
    );
    let _ = fs::remove_dir_all(&out_dir);
}
