use std::fs;
use std::path::Path;
use std::process::Command;

/// Write a `panschema-publish.toml` + main schema file into `pkg_dir`.
/// Mirrors the unified package shape: every path source is a directory
/// containing a publish file + the main file.
fn write_pkg(pkg_dir: &Path, name: &str, version: &str, main_filename: &str, schema_body: &str) {
    fs::create_dir_all(pkg_dir).expect("mkdir pkg");
    let publish = format!(
        r#"[schema]
name = "{name}"
version = "{version}"
linkml = "1.7.0"

[files]
main = "{main_filename}"
"#
    );
    fs::write(pkg_dir.join("panschema-publish.toml"), publish).expect("write publish toml");
    fs::write(pkg_dir.join(main_filename), schema_body).expect("write schema body");
}

/// Convenience: write a package whose main file is a copy of the static
/// `sample_schema.yaml` fixture. Returns the absolute `pkg_dir` path.
fn write_sample_pkg(parent: &Path, dirname: &str) -> std::path::PathBuf {
    let pkg = parent.join(dirname);
    fs::create_dir_all(&pkg).expect("mkdir pkg");
    fs::copy(
        "tests/fixtures/sample_schema.yaml",
        pkg.join("sample_schema.yaml"),
    )
    .expect("copy sample schema");
    fs::write(
        pkg.join("panschema-publish.toml"),
        r#"[schema]
name = "sample_schema"
version = "1.0.0"
linkml = "1.7.0"

[files]
main = "sample_schema.yaml"
"#,
    )
    .expect("write publish toml");
    pkg
}

#[test]
fn class_card_surfaces_mixins_slots_and_resolved_xrefs() {
    let output_dir = std::env::temp_dir().join("panschema_class_card_dogfood");
    let _ = fs::remove_dir_all(&output_dir);
    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
        html.contains("__PANSCHEMA_INSTANCE_GRAPH_DATA__"),
        "the LinkML A-box should be embedded as instance-graph data"
    );

    // Each record became a typed node; each class-ranged scalar an edge.
    let data = extract_json_assignment(&html, "__PANSCHEMA_INSTANCE_GRAPH_DATA__");
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
fn instances_flag_warns_only_for_non_html_formats() {
    let dir = std::env::temp_dir().join("panschema_instances_warn_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir");

    // With --instances on a non-HTML format, the flag is ignored — warn so
    // the omission isn't silent.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--input",
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
        stderr.contains("--instances only affects HTML output"),
        "a non-HTML format with --instances should warn; got: {stderr}"
    );

    // The same non-HTML format without --instances does not warn.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--input",
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
fn dangling_instance_reference_warns_and_fails_under_strict() {
    let dir = std::env::temp_dir().join("panschema_instance_dangling_test");
    let _ = fs::remove_dir_all(&dir);

    // A wine references a winery the data file doesn't define. Without
    // --strict, generation succeeds but warns, naming the dangling reference.
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--input",
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
            "--input",
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
            "--input",
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
            "--input",
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
fn generates_jsonld_via_cli() {
    let output_dir = std::env::temp_dir().join("panschema_jsonld_test");
    let _ = fs::remove_dir_all(&output_dir);
    fs::create_dir_all(&output_dir).expect("Failed to create output dir");

    let output_path = output_dir.join("output.jsonld");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--input",
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
            "--input",
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
            "--input",
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

/// `panschema generate` (no --input) discovers a `panschema.toml`, walks
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

    // Run `panschema generate` from the consumer dir (no --input).
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

/// `panschema generate --input X --format html` (without `--no-graph`)
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
            "--input",
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
            "--input",
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
                "--input",
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
            !stderr.contains("RDF/OWL"),
            "{format}'s warning must not hardcode RDF/OWL; got:\n{stderr}"
        );
    }

    let html_output = tmp.join("html_out");
    let output = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args([
            "generate",
            "--input",
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
            "--input",
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
            "--input",
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
    fs::create_dir_all(&base).expect("mkdir base");
    fs::write(
        base.join("base.yaml"),
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
    )
    .expect("write base schema");
    fs::write(
        base.join("panschema-publish.toml"),
        "[schema]\nname = \"base\"\nversion = \"1.0.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"base.yaml\"\n",
    )
    .expect("write base publish toml");

    // App package: a Gadget class referencing base's Widget, importing base
    // by its dependency name (the `[schemas]` key), not a local path.
    let app = root.join("app-pkg");
    fs::create_dir_all(&app).expect("mkdir app");
    fs::write(
        app.join("app.yaml"),
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
    )
    .expect("write app schema");
    fs::write(
        app.join("panschema-publish.toml"),
        "[schema]\nname = \"app\"\nversion = \"1.0.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"app.yaml\"\n",
    )
    .expect("write app publish toml");

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
    fs::create_dir_all(&app).expect("mkdir app");
    fs::write(
        app.join("app.yaml"),
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
    )
    .expect("write app schema");
    fs::write(
        app.join("panschema-publish.toml"),
        "[schema]\nname = \"app\"\nversion = \"1.0.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"app.yaml\"\n",
    )
    .expect("write app publish toml");

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

    // Helper: write a package dir with a schema file + publish toml.
    let write_pkg = |dir: &str, file: &str, name: &str, body: &str| {
        let pkg = root.join(dir);
        fs::create_dir_all(&pkg).expect("mkdir pkg");
        fs::write(pkg.join(file), body).expect("write schema");
        fs::write(
            pkg.join("panschema-publish.toml"),
            format!("[schema]\nname = \"{name}\"\nversion = \"1.0.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"{file}\"\n"),
        )
        .expect("write publish toml");
    };

    write_pkg(
        "base-pkg",
        "base.yaml",
        "base",
        "name: base\nid: https://example.org/base\ndefault_range: string\n\
         classes:\n  Base:\n    attributes:\n      a:\n        range: string\n",
    );
    // Two siblings, each importing base and referencing Base.
    write_pkg(
        "dep1-pkg",
        "dep1.yaml",
        "dep1",
        "name: dep1\nid: https://example.org/dep1\ndefault_range: string\n\
         imports:\n  - base\nclasses:\n  Dep1:\n    attributes:\n      b:\n        range: Base\n",
    );
    write_pkg(
        "dep2-pkg",
        "dep2.yaml",
        "dep2",
        "name: dep2\nid: https://example.org/dep2\ndefault_range: string\n\
         imports:\n  - base\nclasses:\n  Dep2:\n    attributes:\n      c:\n        range: Base\n",
    );
    // App imports both siblings and references each.
    write_pkg(
        "app-pkg",
        "app.yaml",
        "app",
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

    let write_pkg = |dir: &str, file: &str, name: &str, body: &str| {
        let pkg = root.join(dir);
        fs::create_dir_all(&pkg).expect("mkdir pkg");
        fs::write(pkg.join(file), body).expect("write schema");
        fs::write(
            pkg.join("panschema-publish.toml"),
            format!("[schema]\nname = \"{name}\"\nversion = \"1.0.0\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"{file}\"\n"),
        )
        .expect("write publish toml");
    };

    // Two deps define `Shared` incompatibly; neither is the app.
    write_pkg(
        "dep1-pkg",
        "dep1.yaml",
        "dep1",
        "name: dep1\nid: https://example.org/dep1\ndefault_range: string\n\
         classes:\n  Shared:\n    description: from dep1\n    attributes:\n      a:\n        range: string\n",
    );
    write_pkg(
        "dep2-pkg",
        "dep2.yaml",
        "dep2",
        "name: dep2\nid: https://example.org/dep2\ndefault_range: string\n\
         classes:\n  Shared:\n    description: from dep2 (incompatible)\n    attributes:\n      b:\n        range: integer\n",
    );
    write_pkg(
        "app-pkg",
        "app.yaml",
        "app",
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
        r#"[schema]
name = "reference"
version = "1.0.0"
linkml = "1.7.0"

[files]
main = "reference.ttl"
"#,
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
        r#"[schema]
name = "scimantic"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema/scimantic.yaml"
"#,
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
        format!(
            "[schema]\nname = \"x\"\nversion = \"{version}\"\nlinkml = \"1.7.0\"\n\n[files]\nmain = \"schema.yaml\"\n"
        ),
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
        r#"[schema]
name = "x"
version = "0.1.0"
linkml = "1.7.0"

[files]
main = "schema.yaml"
"#,
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
            "--input",
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
