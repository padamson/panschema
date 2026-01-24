use std::fs;
use std::process::Command;

#[test]
fn generates_documentation_from_reference_ontology() {
    let output_dir = std::env::temp_dir().join("rontodoc_integration_test");
    let _ = fs::remove_dir_all(&output_dir);

    let status = Command::new(env!("CARGO_BIN_EXE_rontodoc"))
        .args([
            "--input",
            "tests/fixtures/reference.ttl",
            "--output",
            output_dir.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute rontodoc");

    assert!(status.success(), "rontodoc exited with error");

    let index_path = output_dir.join("index.html");
    assert!(index_path.exists(), "index.html was not generated");

    let html = fs::read_to_string(&index_path).expect("Failed to read index.html");

    // Verify key content
    assert!(
        html.contains("Rontodoc Reference Ontology"),
        "Missing ontology title"
    );
    assert!(
        html.contains("http://example.org/rontodoc/reference"),
        "Missing ontology IRI"
    );
    assert!(html.contains("0.1.0"), "Missing version");
    assert!(
        html.contains("A reference ontology for testing"),
        "Missing description"
    );

    // Cleanup
    let _ = fs::remove_dir_all(output_dir);
}
