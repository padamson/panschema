//! Identifier casing helpers shared across writers.
//!
//! `snake_case` and `pascal_case` are used by the Rust writer (field and
//! type names) and the Postgres writer (table/column/enum-type names).
//! They live here, in a neutral module, so a non-Rust writer doesn't have
//! to reach into `rust_writer` for a name transform.

/// Convert a LinkML identifier (typically lowerCamelCase) to snake_case
/// for use as a Rust field name. Lowercases existing characters and
/// inserts `_` before each uppercase letter that follows a lowercase one
/// or a digit. Handles consecutive uppercase by treating runs as a single
/// "word" (so `URL_path` → `url_path`, not `u_r_l_path`).
///
/// Examples:
/// - `wasGeneratedBy` → `was_generated_by`
/// - `id` → `id`
/// - `URL` → `url`
/// - `parseHTTPRequest` → `parse_http_request`
/// - `already_snake` → `already_snake`
pub fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let mut prev: Option<char> = None;
    let mut iter = name.chars().peekable();

    while let Some(c) = iter.next() {
        if c == '_' {
            out.push('_');
            prev = Some(c);
            continue;
        }
        if c.is_ascii_uppercase() {
            let next = iter.peek().copied();
            let prev_is_lower_or_digit =
                prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit());
            let prev_is_upper = prev.is_some_and(|p| p.is_ascii_uppercase());
            let next_is_lower = next.is_some_and(|n| n.is_ascii_lowercase());
            let needs_underscore = prev.is_some()
                && !out.ends_with('_')
                && (prev_is_lower_or_digit || (prev_is_upper && next_is_lower));
            if needs_underscore {
                out.push('_');
            }
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        } else {
            out.push(c);
        }
        prev = Some(c);
    }
    out
}

/// Convert an identifier (lowerCamelCase, snake_case, or already
/// PascalCase) to PascalCase. Used to derive a Rust type name from a
/// LinkML slot name (`wasDerivedFrom` → `WasDerivedFrom`).
pub fn pascal_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut capitalize_next = true;
    for c in name.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
            continue;
        }
        if capitalize_next {
            for upper in c.to_uppercase() {
                out.push(upper);
            }
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_lower_camel() {
        assert_eq!(snake_case("wasGeneratedBy"), "was_generated_by");
    }

    #[test]
    fn snake_case_already_snake() {
        assert_eq!(snake_case("already_snake"), "already_snake");
    }

    #[test]
    fn snake_case_single_lowercase() {
        assert_eq!(snake_case("id"), "id");
    }

    #[test]
    fn snake_case_all_caps_acronym() {
        assert_eq!(snake_case("URL"), "url");
    }

    #[test]
    fn snake_case_internal_acronym() {
        assert_eq!(snake_case("parseHTTPRequest"), "parse_http_request");
    }

    #[test]
    fn snake_case_with_digit() {
        assert_eq!(snake_case("foo2Bar"), "foo2_bar");
    }

    #[test]
    fn pascal_case_lower_camel_to_pascal() {
        assert_eq!(pascal_case("wasDerivedFrom"), "WasDerivedFrom");
    }

    #[test]
    fn pascal_case_snake_to_pascal() {
        assert_eq!(pascal_case("some_snake_name"), "SomeSnakeName");
    }

    #[test]
    fn pascal_case_already_pascal() {
        assert_eq!(pascal_case("UncertaintyModel"), "UncertaintyModel");
    }

    #[test]
    fn pascal_case_single_lowercase() {
        assert_eq!(pascal_case("id"), "Id");
    }
}
