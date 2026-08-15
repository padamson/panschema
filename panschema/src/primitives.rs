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
    xsd_datatype(name).map(str::to_string)
}

/// [`xsd_datatype_iri`] as the static it is: every IRI in this table is a
/// fixed, known-valid absolute IRI, so per-value callers need neither an
/// allocation nor an IRI re-validation.
pub fn xsd_datatype(name: &str) -> Option<&'static str> {
    Some(match canonical_primitive(name)? {
        "integer" => "http://www.w3.org/2001/XMLSchema#integer",
        "float" => "http://www.w3.org/2001/XMLSchema#float",
        "double" => "http://www.w3.org/2001/XMLSchema#double",
        "decimal" => "http://www.w3.org/2001/XMLSchema#decimal",
        "boolean" => "http://www.w3.org/2001/XMLSchema#boolean",
        "date" => "http://www.w3.org/2001/XMLSchema#date",
        "datetime" => "http://www.w3.org/2001/XMLSchema#dateTime",
        "time" => "http://www.w3.org/2001/XMLSchema#time",
        "uri" | "uriorcurie" => "http://www.w3.org/2001/XMLSchema#anyURI",
        "ncname" => "http://www.w3.org/2001/XMLSchema#NCName",
        // `string` and the remaining LinkML identifier types (`curie`,
        // `jsonpointer`, …) have no dedicated XSD datatype; `xsd:string` is
        // their canonical lexical space.
        _ => "http://www.w3.org/2001/XMLSchema#string",
    })
}

/// The typed-literal parts a **conforming** value takes under `range`:
/// `Some((lexical, datatype IRI))` exactly when `range` canonicalizes to a
/// built-in primitive, the value's kind matches it, the lexical form is
/// faithful in that datatype's lexical space, and the datatype's value space
/// can hold it. `None` for everything else — a non-primitive range, or a
/// value the datatype cannot stand behind (`5.7` under `integer`, `NaN`
/// under `decimal`, `tomorrow` under `date`, an f32-overflowing value under
/// `float`) — and the caller keeps the authored value-kind form, so nothing
/// vanishes and the schema's shapes report the mismatch visibly instead of
/// this table minting an ill-formed or silently converted literal.
///
/// Lexical gates stop at the datatype's grammar (field ranges included);
/// calendar-level validity (a February 30th) is a validator concern, not a
/// serialization one.
pub fn range_typed_literal<'a>(
    range: &str,
    scalar: &'a crate::instances::ScalarValue,
) -> Option<(std::borrow::Cow<'a, str>, &'static str)> {
    use crate::instances::ScalarValue;
    use std::borrow::Cow;
    let primitive = canonical_primitive(range)?;
    if !kind_matches(primitive, scalar) {
        return None;
    }
    let lexical: Cow<'a, str> = match (primitive, scalar) {
        // Rust's Display spells infinities `inf`, outside XSD's lexical
        // space; NaN's spellings coincide but takes this arm for the same
        // contract.
        ("float" | "double", ScalarValue::Float(f)) if !f.is_finite() => {
            Cow::Borrowed(if f.is_nan() {
                "NaN"
            } else if f.is_sign_positive() {
                "INF"
            } else {
                "-INF"
            })
        }
        // xsd:float's value space is single precision: a magnitude beyond
        // f32::MAX would be read back as INF by a conforming processor.
        ("float", ScalarValue::Float(f)) if f.abs() > f64::from(f32::MAX) => return None,
        // xsd:decimal has no lexical form for non-finite values at all.
        ("decimal", ScalarValue::Float(f)) if !f.is_finite() => return None,
        ("date", ScalarValue::String(s)) => is_xsd_date(s).then_some(Cow::Borrowed(s.as_str()))?,
        ("datetime", ScalarValue::String(s)) => {
            is_xsd_datetime(s).then_some(Cow::Borrowed(s.as_str()))?
        }
        ("time", ScalarValue::String(s)) => {
            is_xsd_time(strip_timezone(s)).then_some(Cow::Borrowed(s.as_str()))?
        }
        (_, ScalarValue::String(s)) => Cow::Borrowed(s.as_str()),
        // The plain spellings defer to the one shared scalar-to-text
        // conversion, so RDF lexical forms cannot drift from the spelling
        // every other surface shows for the same value. This includes the
        // integral float the kind gate admits under `integer`: Display
        // spells it without a fraction, so `"5.0"^^xsd:integer` cannot be
        // minted.
        _ => Cow::Owned(crate::instances::scalar_to_display(scalar)),
    };
    Some((lexical, xsd_datatype(primitive)?))
}

/// `s` without a trailing XSD timezone (`Z` or `±hh:mm`), for validating the
/// date/time fields it qualifies.
fn strip_timezone(s: &str) -> &str {
    if let Some(rest) = s.strip_suffix('Z') {
        return rest;
    }
    // `split_at_checked` also rejects a split that falls inside a
    // multi-byte character, which no timezone-carrying lexical form has.
    let Some((head, tz)) = s.len().checked_sub(6).and_then(|i| s.split_at_checked(i)) else {
        return s;
    };
    // A bare offset with nothing before it qualifies no value.
    if head.is_empty() {
        return s;
    }
    let bytes = tz.as_bytes();
    if (bytes[0] == b'+' || bytes[0] == b'-')
        && bytes[3] == b':'
        && digit_field(&tz[1..3], 0, 14).is_some()
        && digit_field(&tz[4..6], 0, 59).is_some()
    {
        return head;
    }
    s
}

/// An exactly-two-digit field parsed and range-checked, `None` otherwise.
fn digit_field(s: &str, min: u32, max: u32) -> Option<u32> {
    if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let v: u32 = s.parse().ok()?;
    (min..=max).contains(&v).then_some(v)
}

/// `(-)YYYY(+)-MM-DD`, month and day range-checked, optional timezone.
fn is_xsd_date(s: &str) -> bool {
    let s = strip_timezone(s);
    let s = s.strip_prefix('-').unwrap_or(s);
    let Some((year, rest)) = s.split_once('-') else {
        return false;
    };
    if year.len() < 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let Some((month, day)) = rest.split_once('-') else {
        return false;
    };
    digit_field(month, 1, 12).is_some() && digit_field(day, 1, 31).is_some()
}

/// `hh:mm:ss` with an optional `.fraction` (timezone already stripped by the
/// caller). Hour 24 is admitted per XSD's midnight spelling.
fn is_xsd_time(s: &str) -> bool {
    let mut fields = s.splitn(3, ':');
    let (Some(h), Some(m), Some(sec)) = (fields.next(), fields.next(), fields.next()) else {
        return false;
    };
    let (whole, fraction) = match sec.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (sec, None),
    };
    digit_field(h, 0, 24).is_some()
        && digit_field(m, 0, 59).is_some()
        && digit_field(whole, 0, 59).is_some()
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// `<date>T<time>`, with the timezone (if any) on the time half.
fn is_xsd_datetime(s: &str) -> bool {
    match s.split_once('T') {
        Some((date, time)) => {
            // The date half carries no timezone of its own, so validate it
            // verbatim; the time half may end in one.
            !date.ends_with('Z') && is_xsd_date(date) && is_xsd_time(strip_timezone(time))
        }
        None => false,
    }
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

/// Whether a scalar's kind satisfies a primitive range, with JSON Schema
/// number semantics: an integer is a valid float/double/decimal, and an
/// integral float (`5.0`) is a valid integer. Everything outside the numeric
/// and boolean families — strings, dates, URIs — is string-kinded. Shared by
/// the validator and the writers so "conforms" means one thing everywhere.
pub fn kind_matches(primitive: &str, scalar: &crate::instances::ScalarValue) -> bool {
    use crate::instances::ScalarValue;
    match primitive {
        "integer" => match scalar {
            ScalarValue::Integer(_) => true,
            ScalarValue::Float(f) => f.fract() == 0.0,
            _ => false,
        },
        "float" | "double" | "decimal" => {
            matches!(scalar, ScalarValue::Integer(_) | ScalarValue::Float(_))
        }
        "boolean" => matches!(scalar, ScalarValue::Boolean(_)),
        _ => matches!(scalar, ScalarValue::String(_)),
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

    use crate::instances::ScalarValue;
    use std::borrow::Cow;

    fn typed(range: &str, scalar: &ScalarValue) -> Option<(String, &'static str)> {
        range_typed_literal(range, scalar).map(|(lexical, dt)| (lexical.into_owned(), dt))
    }

    #[test]
    fn a_kind_mismatch_yields_no_typed_literal() {
        // Some ⟺ conforming: callers need no separate kind check, and the
        // integral-float spelling can never round a non-integral value.
        assert_eq!(typed("integer", &ScalarValue::Float(5.7)), None);
        assert_eq!(typed("integer", &ScalarValue::String("abc".into())), None);
        assert_eq!(typed("string", &ScalarValue::Integer(42)), None);
        assert_eq!(typed("boolean", &ScalarValue::String("true".into())), None);
        assert_eq!(typed("integer", &ScalarValue::Float(f64::NAN)), None);
    }

    #[test]
    fn a_non_primitive_range_yields_no_typed_literal() {
        assert_eq!(typed("Warehouse", &ScalarValue::String("x".into())), None);
        assert_eq!(typed("MyEnum", &ScalarValue::String("x".into())), None);
    }

    #[test]
    fn conforming_values_take_the_ranges_datatype_and_canonical_spelling() {
        assert_eq!(
            typed("integer", &ScalarValue::Float(5.0)),
            Some(("5".to_string(), "http://www.w3.org/2001/XMLSchema#integer")),
            "an integral float spells as an integer"
        );
        assert_eq!(
            typed("float", &ScalarValue::Integer(4)),
            Some(("4".to_string(), "http://www.w3.org/2001/XMLSchema#float")),
        );
        assert_eq!(
            typed("decimal", &ScalarValue::Integer(4)),
            Some(("4".to_string(), "http://www.w3.org/2001/XMLSchema#decimal")),
        );
        assert_eq!(
            typed("boolean", &ScalarValue::Boolean(true)),
            Some((
                "true".to_string(),
                "http://www.w3.org/2001/XMLSchema#boolean"
            )),
        );
    }

    #[test]
    fn the_string_passthrough_borrows_rather_than_cloning() {
        let value = ScalarValue::String("hello".into());
        let (lexical, _) = range_typed_literal("string", &value).expect("conforms");
        assert!(
            matches!(lexical, Cow::Borrowed("hello")),
            "the dominant string case must not allocate"
        );
    }

    #[test]
    fn non_finite_floats_take_xsd_spellings_under_float_and_double() {
        // Rust's Display says `inf`, which is outside XSD's lexical space;
        // the XSD spellings are INF / -INF / NaN.
        for range in ["float", "double"] {
            assert_eq!(
                typed(range, &ScalarValue::Float(f64::INFINITY)).map(|(l, _)| l),
                Some("INF".to_string())
            );
            assert_eq!(
                typed(range, &ScalarValue::Float(f64::NEG_INFINITY)).map(|(l, _)| l),
                Some("-INF".to_string())
            );
            assert_eq!(
                typed(range, &ScalarValue::Float(f64::NAN)).map(|(l, _)| l),
                Some("NaN".to_string())
            );
        }
    }

    #[test]
    fn decimal_has_no_form_for_non_finite_values() {
        assert_eq!(typed("decimal", &ScalarValue::Float(f64::NAN)), None);
        assert_eq!(typed("decimal", &ScalarValue::Float(f64::INFINITY)), None);
    }

    #[test]
    fn finite_floats_keep_their_plain_spelling_under_every_float_family_range() {
        for range in ["float", "double", "decimal"] {
            assert_eq!(
                typed(range, &ScalarValue::Float(2.5)).map(|(l, _)| l),
                Some("2.5".to_string()),
                "a finite float under `{range}` spells as itself"
            );
        }
    }

    #[test]
    fn a_float_range_rejects_values_outside_single_precision() {
        assert_eq!(
            typed("float", &ScalarValue::Float(1e300)),
            None,
            "a conforming processor would read the literal back as INF"
        );
        assert!(
            typed("float", &ScalarValue::Float(3.0e38)).is_some(),
            "values inside f32's range keep the typed form"
        );
        assert!(
            typed("float", &ScalarValue::Float(f64::from(f32::MAX))).is_some(),
            "the boundary itself is representable, so it keeps the typed form"
        );
        assert!(
            typed("double", &ScalarValue::Float(1e300)).is_some(),
            "double's value space holds it"
        );
    }

    #[test]
    fn date_lexicals_are_gated_at_the_grammar() {
        let ok = ScalarValue::String("2024-06-01".into());
        assert!(typed("date", &ok).is_some());
        for good in [
            "2024-06-01Z",
            "2024-06-01+05:30",
            "-0044-03-15",
            "12024-06-01",
        ] {
            assert!(
                typed("date", &ScalarValue::String(good.into())).is_some(),
                "`{good}` is a valid xsd:date lexical"
            );
        }
        for bad in [
            "tomorrow",
            "2024-6-1",
            "2024-13-01",
            "2024-06-32",
            "2024-06",
            "202-06-01",
            // A multi-byte character near the end must not panic the
            // timezone probe's byte-offset split.
            "2024-06-0é",
        ] {
            assert_eq!(
                typed("date", &ScalarValue::String(bad.into())),
                None,
                "`{bad}` is outside xsd:date's lexical space"
            );
        }
    }

    #[test]
    fn time_and_datetime_lexicals_are_gated_at_the_grammar() {
        for good in ["09:30:00", "09:30:00.5Z", "24:00:00", "23:59:59-08:00"] {
            assert!(
                typed("time", &ScalarValue::String(good.into())).is_some(),
                "`{good}` is a valid xsd:time lexical"
            );
        }
        for bad in ["9:30", "25:00:00", "09:61:00", "09:30:00.", "noonish"] {
            assert_eq!(
                typed("time", &ScalarValue::String(bad.into())),
                None,
                "`{bad}` is outside xsd:time's lexical space"
            );
        }
        for good in ["2024-06-01T09:30:00", "2024-06-01T09:30:00.25+01:00"] {
            assert!(
                typed("datetime", &ScalarValue::String(good.into())).is_some(),
                "`{good}` is a valid xsd:dateTime lexical"
            );
        }
        for bad in [
            "2024-06-01 09:30:00",
            "2024-06-01T",
            "2024-06-01",
            "09:30:00",
        ] {
            assert_eq!(
                typed("datetime", &ScalarValue::String(bad.into())),
                None,
                "`{bad}` is outside xsd:dateTime's lexical space"
            );
        }
    }
}
