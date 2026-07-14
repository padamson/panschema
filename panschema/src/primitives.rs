//! The LinkML built-in primitive types, their common aliases, and their XSD
//! mappings — one shared table so every writer (RDF/OWL, SHACL, Postgres,
//! Rust) and the dangling-reference diagnostic agree on what a "primitive"
//! is and how an alias like `int`/`bool`/`str` canonicalizes.

/// Canonicalize a range name to its LinkML built-in primitive, resolving the
/// common aliases (`int`→`integer`, `bool`→`boolean`, `str`→`string`).
/// Returns `None` when `name` is not a built-in primitive — it's a class,
/// enum, `types:` entry, or a typo — so callers can guard instead of
/// fabricating output for it.
pub fn canonical_primitive(name: &str) -> Option<&'static str> {
    Some(match name {
        "string" | "str" => "string",
        "integer" | "int" => "integer",
        "boolean" | "bool" => "boolean",
        "float" => "float",
        "double" => "double",
        "decimal" => "decimal",
        "time" => "time",
        "date" => "date",
        "datetime" => "datetime",
        "date_or_datetime" => "date_or_datetime",
        "uriorcurie" => "uriorcurie",
        "curie" => "curie",
        "uri" => "uri",
        "ncname" => "ncname",
        "objectidentifier" => "objectidentifier",
        "nodeidentifier" => "nodeidentifier",
        "jsonpointer" => "jsonpointer",
        "jsonpath" => "jsonpath",
        "sparqlpath" => "sparqlpath",
        _ => return None,
    })
}

/// The absolute XSD datatype IRI for a LinkML primitive range, or `None` when
/// `name` is not a built-in primitive. Callers emit no datatype for `None`
/// rather than fabricating a nonexistent `xsd:{name}` IRI (the finding behind
/// the earlier `xsd:DeploymentStatus` / `xsd:int` bugs).
pub fn xsd_datatype_iri(name: &str) -> Option<String> {
    let xsd = "http://www.w3.org/2001/XMLSchema#";
    let local = match canonical_primitive(name)? {
        "string" => "string",
        "integer" => "integer",
        "float" => "float",
        "double" => "double",
        "decimal" => "decimal",
        "boolean" => "boolean",
        "date" => "date",
        "datetime" => "dateTime",
        "time" => "time",
        "uri" | "uriorcurie" => "anyURI",
        "ncname" => "NCName",
        // The remaining LinkML identifier types have no dedicated XSD
        // datatype; `xsd:string` is their canonical lexical space.
        _ => "string",
    };
    Some(format!("{xsd}{local}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_common_aliases() {
        assert_eq!(canonical_primitive("int"), Some("integer"));
        assert_eq!(canonical_primitive("bool"), Some("boolean"));
        assert_eq!(canonical_primitive("str"), Some("string"));
        assert_eq!(canonical_primitive("integer"), Some("integer"));
    }

    #[test]
    fn non_primitive_is_none() {
        assert_eq!(canonical_primitive("Warehouse"), None);
        assert_eq!(canonical_primitive("MyEnum"), None);
    }

    #[test]
    fn alias_maps_to_the_canonical_xsd_datatype_not_a_fabrication() {
        // `int` must resolve to xsd:integer, never a nonexistent xsd:int.
        assert_eq!(
            xsd_datatype_iri("int").as_deref(),
            Some("http://www.w3.org/2001/XMLSchema#integer")
        );
        assert_eq!(
            xsd_datatype_iri("datetime").as_deref(),
            Some("http://www.w3.org/2001/XMLSchema#dateTime")
        );
    }

    #[test]
    fn non_primitive_has_no_xsd_datatype() {
        assert_eq!(xsd_datatype_iri("Warehouse"), None);
    }
}
