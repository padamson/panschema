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
        // `string` and the remaining LinkML identifier types (`curie`,
        // `jsonpointer`, …) have no dedicated XSD datatype; `xsd:string` is
        // their canonical lexical space.
        _ => "string",
    };
    Some(format!("{xsd}{local}"))
}

/// The built-in primitive a range name ultimately denotes: the name itself
/// when it is a built-in scalar (aliases canonicalized), the base of a
/// custom `types:` entry's `typeof` chain, or — for a root type with no
/// `typeof:` — the primitive whose lexical space its `uri:` names. `None`
/// for classes, enums, unknown names, chains that never reach a primitive,
/// and cycles — a caller that enforces typing must skip rather than guess.
pub fn effective_primitive(
    schema: &crate::linkml::SchemaDefinition,
    range: &str,
) -> Option<&'static str> {
    let mut seen: Vec<&str> = Vec::new();
    let mut current = range;
    loop {
        if let Some(p) = canonical_primitive(current) {
            return Some(p);
        }
        if seen.contains(&current) {
            return None;
        }
        seen.push(current);
        let type_def = schema.types.get(current)?;
        match type_def.typeof_.as_deref() {
            Some(parent) => current = parent,
            None => return type_def.uri.as_deref().and_then(primitive_for_datatype_uri),
        }
    }
}

/// The LinkML primitive whose lexical space an XSD datatype reference
/// denotes — accepts a CURIE (`xsd:integer`) or an absolute IRI. `None`
/// for datatypes with no primitive counterpart.
fn primitive_for_datatype_uri(uri: &str) -> Option<&'static str> {
    let local = uri.rsplit(['#', ':', '/']).next()?;
    Some(match local {
        "string" | "normalizedString" | "token" => "string",
        "integer" | "int" | "long" | "short" | "byte" | "nonNegativeInteger"
        | "positiveInteger" | "nonPositiveInteger" | "negativeInteger" | "unsignedLong"
        | "unsignedInt" | "unsignedShort" | "unsignedByte" => "integer",
        "float" => "float",
        "double" => "double",
        "decimal" => "decimal",
        "boolean" => "boolean",
        "date" => "date",
        "dateTime" => "datetime",
        "time" => "time",
        "anyURI" => "uri",
        "NCName" => "ncname",
        _ => return None,
    })
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

    #[test]
    fn a_uri_only_type_resolves_to_the_primitive_its_datatype_names() {
        // Pin every datatype→primitive mapping (as a CURIE, plus one absolute
        // IRI) so dropping or misrouting an arm is caught, and pin that an
        // unmapped datatype resolves to nothing rather than a guess.
        use crate::linkml::{SchemaDefinition, TypeDefinition};
        let type_with_uri = |uri: &str| {
            let mut schema = SchemaDefinition::new("s");
            let mut type_def = TypeDefinition::new("T");
            type_def.uri = Some(uri.to_string());
            schema.types.insert("T".to_string(), type_def);
            schema
        };
        for (uri, primitive) in [
            ("xsd:string", "string"),
            ("xsd:normalizedString", "string"),
            ("xsd:token", "string"),
            ("xsd:integer", "integer"),
            ("xsd:int", "integer"),
            ("xsd:long", "integer"),
            ("xsd:short", "integer"),
            ("xsd:byte", "integer"),
            ("xsd:nonNegativeInteger", "integer"),
            ("xsd:positiveInteger", "integer"),
            ("xsd:nonPositiveInteger", "integer"),
            ("xsd:negativeInteger", "integer"),
            ("xsd:unsignedLong", "integer"),
            ("xsd:unsignedInt", "integer"),
            ("xsd:unsignedShort", "integer"),
            ("xsd:unsignedByte", "integer"),
            ("xsd:float", "float"),
            ("xsd:double", "double"),
            ("xsd:decimal", "decimal"),
            ("xsd:boolean", "boolean"),
            ("xsd:date", "date"),
            ("xsd:dateTime", "datetime"),
            ("xsd:time", "time"),
            ("xsd:anyURI", "uri"),
            ("xsd:NCName", "ncname"),
            ("http://www.w3.org/2001/XMLSchema#integer", "integer"),
        ] {
            assert_eq!(
                effective_primitive(&type_with_uri(uri), "T"),
                Some(primitive),
                "wrong primitive for `{uri}`"
            );
        }
        assert_eq!(
            effective_primitive(&type_with_uri("xsd:hexBinary"), "T"),
            None,
            "a datatype with no primitive counterpart is never guessed at"
        );
    }

    #[test]
    fn every_builtin_primitive_canonicalizes_to_itself() {
        // Each canonical primitive name resolves to itself (pins every arm so
        // dropping one is caught, not silently None).
        for p in [
            "string",
            "integer",
            "boolean",
            "float",
            "double",
            "decimal",
            "time",
            "date",
            "datetime",
            "date_or_datetime",
            "uriorcurie",
            "curie",
            "uri",
            "ncname",
            "objectidentifier",
            "nodeidentifier",
            "jsonpointer",
            "jsonpath",
            "sparqlpath",
        ] {
            assert_eq!(canonical_primitive(p), Some(p), "`{p}` must be a primitive");
        }
    }

    #[test]
    fn xsd_datatype_iri_is_canonical_for_each_primitive() {
        // Pin the exact XSD local name each primitive maps to, so dropping an
        // arm (which would fall back to `xsd:string`) is caught.
        let xsd = "http://www.w3.org/2001/XMLSchema#";
        for (name, local) in [
            ("string", "string"),
            ("integer", "integer"),
            ("float", "float"),
            ("double", "double"),
            ("decimal", "decimal"),
            ("boolean", "boolean"),
            ("date", "date"),
            ("datetime", "dateTime"),
            ("time", "time"),
            ("uri", "anyURI"),
            ("uriorcurie", "anyURI"),
            ("ncname", "NCName"),
            // identifier types with no dedicated XSD datatype → xsd:string
            ("curie", "string"),
            ("jsonpointer", "string"),
        ] {
            assert_eq!(
                xsd_datatype_iri(name),
                Some(format!("{xsd}{local}")),
                "wrong XSD datatype for `{name}`"
            );
        }
    }
}
