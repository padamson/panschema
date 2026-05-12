use std::fs;
use std::path::Path;
use std::process::Command;

/// Write a `panschema-publish.toml` + main schema file into `pkg_dir`.
/// Mirrors the v0.3 unified package shape (slice 1 retrofit): every
/// path source is a directory containing a publish file + the main file.
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
    assert!(
        manifest.contains("[generate.sample_schema]"),
        "manifest should have a starter `[generate.sample_schema]` block: {manifest}"
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

/// `panschema add --no-generate-config` skips the starter `[generate.<name>]` block.
#[test]
fn add_no_generate_config_skips_generate_block() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");
    fs::write(consumer.join("panschema.toml"), "[schemas]\n").expect("write manifest");

    let status = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("add")
        .arg("./sample-pkg")
        .arg("--no-generate-config")
        .current_dir(consumer)
        .status()
        .expect("Failed to execute panschema");
    assert!(status.success());

    let manifest = fs::read_to_string(consumer.join("panschema.toml")).expect("read manifest");
    assert!(
        manifest.contains("sample_schema"),
        "manifest should contain `sample_schema`"
    );
    assert!(
        !manifest.contains("[generate.sample_schema]"),
        "no-generate-config should suppress the starter block"
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
    assert!(
        dir.join("panschema-publish.toml").exists(),
        "publish.toml should still be written"
    );
}
