use anyhow::{Context, Result};
use oxigraph::store::Store;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Loads an ontology from the specified path.
///
/// # Errors
/// Returns an error if the file cannot be opened or parsed.
pub fn load_ontology(path: &Path) -> Result<(Store, Vec<(String, String)>)> {
    let store = Store::new()?;

    let file = File::open(path)
        .with_context(|| format!("Failed to open ontology file: {}", path.display()))?;
    let reader = BufReader::new(file);

    store
        .load_graph(
            reader,
            oxigraph::io::GraphFormat::Turtle,
            oxigraph::model::GraphNameRef::DefaultGraph,
            None,
        )
        .context("Failed to parse ontology")?;

    let file = File::open(path)?;
    let prefixes = scan_prefixes(BufReader::new(file))?;

    Ok((store, prefixes))
}

fn scan_prefixes<R: std::io::BufRead>(reader: R) -> Result<Vec<(String, String)>> {
    let mut prefixes = Vec::new();
    let lines = reader.lines();

    // Supports both `@prefix` and `PREFIX` directives
    for line in lines {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with("@prefix") || trimmed.to_uppercase().starts_with("PREFIX") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                // Format: @prefix ns: <iri> .
                let mut prefix = parts[1].to_string();
                if prefix.ends_with(':') {
                    prefix.pop();
                }

                let mut iri = parts[2].to_string();
                // Clean up IRI: remove <, >, and trailing .
                if iri.starts_with('<') {
                    iri.remove(0);
                }
                if iri.ends_with('>') {
                    iri.pop();
                }
                if iri.ends_with('.') {
                    iri.pop();
                }
                if iri.ends_with('>') {
                    iri.pop();
                }

                prefixes.push((prefix, iri));
            }
        }
    }

    // Sort
    prefixes.sort();
    Ok(prefixes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_valid_ontology() -> Result<()> {
        // 1. Create a temporary Turtle file
        let mut temp_file = NamedTempFile::new()?;
        writeln!(
            temp_file,
            "<http://example.org/s> <http://example.org/p> <http://example.org/o> ."
        )?;

        // 2. Load it
        let (store, _) = load_ontology(temp_file.path())?;

        // 3. Verify content (1 quad in default graph)
        assert_eq!(store.len()?, 1);
        Ok(())
    }

    #[test]
    fn test_load_missing_file() {
        let path = Path::new("non_existent_file.ttl");
        let result = load_ontology(path);

        match result {
            Ok(_) => panic!("Should have failed to open file"),
            Err(e) => assert!(e.to_string().contains("Failed to open")),
        }
    }

    #[test]
    fn test_load_invalid_syntax() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "This is not turtle syntax")?;

        let result = load_ontology(temp_file.path());

        match result {
            Ok(_) => panic!("Should have failed to parse"),
            Err(e) => assert!(e.to_string().contains("Failed to parse")),
        }
        Ok(())
    }
    #[test]
    fn test_scan_prefixes() -> Result<()> {
        let input = r#"
            @title "Ignored".
            @prefix dc: <http://purl.org/dc/elements/1.1/> .
            PREFIX owl: <http://www.w3.org/2002/07/owl#>
            @prefix : <http://example.org/> .
            @prefix    space:    <http://space.com/>   .
            Just some other content
        "#;

        // Use Cursor as a Read implementation
        let reader = std::io::Cursor::new(input);
        let prefixes = scan_prefixes(reader)?;

        // Expected: sorted by prefix (empty string "" first)
        // "" -> http://example.org/
        // "dc" -> http://purl.org/dc/elements/1.1/
        // "owl" -> http://www.w3.org/2002/07/owl#
        // "space" -> http://space.com/

        assert_eq!(prefixes.len(), 4);
        assert_eq!(prefixes[0].0, "");
        assert_eq!(prefixes[0].1, "http://example.org/");

        assert_eq!(prefixes[1].0, "dc");
        assert_eq!(prefixes[1].1, "http://purl.org/dc/elements/1.1/");

        assert_eq!(prefixes[2].0, "owl");
        assert_eq!(prefixes[2].1, "http://www.w3.org/2002/07/owl#");

        assert_eq!(prefixes[3].0, "space");
        assert_eq!(prefixes[3].1, "http://space.com/");

        Ok(())
    }
}
