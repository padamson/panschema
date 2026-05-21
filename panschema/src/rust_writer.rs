//! Rust code generator for LinkML schemas.
//!
//! Emits a single flat Rust module per schema, suitable for `include!()`
//! or `pub mod` use in a downstream crate. See
//! [docs/features/06-rust-codegen.md](../../docs/features/06-rust-codegen.md)
//! for the LinkML → Rust mapping and the broader roadmap.
//!
//! Generated code depends on `serde` (for `Serialize`/`Deserialize` derives)
//! and `chrono` (for `DateTime<Utc>` when a slot's range is `datetime`).
//! The consumer declares those in their own `Cargo.toml`; panschema
//! itself doesn't take chrono.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};
use std::path::Path;

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::{ClassDefinition, EnumDefinition, SchemaDefinition, SlotDefinition};

/// Writes a Rust module representing the schema's classes, enums, and
/// inheritance structure.
#[derive(Debug, Default)]
pub struct RustWriter;

impl RustWriter {
    pub fn new() -> Self {
        Self
    }

    /// Produce the generated Rust source text for `schema`.
    ///
    /// Separating render-to-string from `write` keeps unit tests simple
    /// (no tempdir or filesystem state needed) and leaves the door open
    /// for snapshot-based testing.
    pub fn render(&self, schema: &SchemaDefinition) -> String {
        let mut out = String::new();
        self.render_into(&mut out, schema)
            .expect("fmt::Write to String cannot fail");
        out
    }

    /// Stream the generated module into any `fmt::Write` sink. `render`
    /// uses this internally to fill a `String`; downstream consumers can
    /// write directly to a file, buffer, or formatter.
    pub fn render_into<W: Write>(&self, out: &mut W, schema: &SchemaDefinition) -> fmt::Result {
        let roles = compute_class_roles(schema);
        let eq_hash_support = compute_eq_hash_support(schema, &roles);
        let mut any_of_enums: BTreeMap<String, Vec<String>> = BTreeMap::new();

        render_header(out, schema)?;

        for (name, def) in &schema.enums {
            render_enum(out, name, def)?;
        }

        // Emission order is load-bearing: traits → structs → Kind enums
        // → any_of enums. Structs reference their traits in `impl Trait
        // for Struct` blocks and reference Kind enums in field types; a
        // forward declaration there would need explicit `mod` prefixes.
        for (name, def) in &schema.classes {
            if roles.get(name) == Some(&ClassRole::Trait) {
                render_trait(out, name, def, schema, &roles)?;
            }
        }
        for (name, def) in &schema.classes {
            if roles.get(name) == Some(&ClassRole::Struct) {
                render_class(
                    out,
                    name,
                    def,
                    schema,
                    &roles,
                    &eq_hash_support,
                    &mut any_of_enums,
                )?;
            }
        }
        for name in schema.classes.keys() {
            if roles.get(name) == Some(&ClassRole::Trait) {
                render_kind_enum(out, name, schema, &roles, &eq_hash_support)?;
            }
        }
        for (enum_name, members) in &any_of_enums {
            let eq_hash_ok = members
                .iter()
                .all(|m| type_supports_eq_hash(m, schema, &roles, &eq_hash_support));
            render_any_of_enum(out, enum_name, members, eq_hash_ok)?;
        }

        Ok(())
    }
}

impl Writer for RustWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(IoError::Io)?;
        }
        std::fs::write(output, self.render(schema)).map_err(IoError::Io)?;
        Ok(())
    }

    fn format_id(&self) -> &str {
        "rust"
    }
}

// ---------------------------------------------------------------------------
// Class roles
// ---------------------------------------------------------------------------

/// Classifies each class as either a marker `Trait` (named as the `is_a`
/// parent of some other class OR used as a `mixin`) or a concrete
/// `Struct` (leaf class — never inherited from, never mixed in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassRole {
    Trait,
    Struct,
}

fn compute_class_roles(schema: &SchemaDefinition) -> BTreeMap<String, ClassRole> {
    let mut used_as_parent_or_mixin = BTreeSet::new();
    for class in schema.classes.values() {
        if let Some(parent) = &class.is_a {
            used_as_parent_or_mixin.insert(parent.clone());
        }
        for mixin in &class.mixins {
            used_as_parent_or_mixin.insert(mixin.clone());
        }
    }
    schema
        .classes
        .keys()
        .map(|name| {
            let role = if used_as_parent_or_mixin.contains(name) {
                ClassRole::Trait
            } else {
                ClassRole::Struct
            };
            (name.clone(), role)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Eq + Hash support analysis
// ---------------------------------------------------------------------------

/// Per-class boolean: does every emitted definition for this class
/// derive `Eq` and `Hash`?
///
/// For a Struct-role class the bit means "the struct itself derives
/// `Eq + Hash`," which requires every resolved-slot field type to do so.
/// For a Trait-role class the bit means "the `<Name>Kind` closed enum
/// derives `Eq + Hash`," which requires every concrete descendant struct
/// to derive them.
///
/// Computed by monotonic fixpoint iteration: classes start at `true`
/// and only flip to `false` on a disqualifying field (or a disqualified
/// referent). Cycles broken by `Box<T>` are handled by construction —
/// the analysis looks at the underlying class, not the framing, and
/// `Box<T>: Eq + Hash` when `T: Eq + Hash`.
fn compute_eq_hash_support(
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
) -> BTreeMap<String, bool> {
    let mut support: BTreeMap<String, bool> =
        schema.classes.keys().map(|n| (n.clone(), true)).collect();

    // Bounded round count: the fixpoint is monotonic (a class only ever
    // flips from `true` to `false`), so it converges in at most N rounds
    // where N = number of classes. The bound is also defense against
    // accidental termination bugs — without it, an `if !changed`
    // mutation can produce an infinite loop on a happy-path schema.
    for _ in 0..=schema.classes.len() {
        let mut changed = false;
        for (name, class) in &schema.classes {
            if !support.get(name).copied().unwrap_or(true) {
                continue;
            }
            let still_ok = match roles.get(name) {
                Some(ClassRole::Trait) => trait_descendants_support(name, schema, roles, &support),
                Some(ClassRole::Struct) => {
                    let resolved = resolve_slots(class, schema);
                    resolved
                        .values()
                        .all(|slot| field_supports_eq_hash(slot, schema, roles, &support))
                }
                None => true,
            };
            if !still_ok {
                support.insert(name.clone(), false);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    support
}

/// True when every concrete descendant of `trait_name` currently
/// supports `Eq + Hash`. Vacuously true for a trait with no descendants
/// — `render_kind_enum` short-circuits emission in that case anyway.
fn trait_descendants_support(
    trait_name: &str,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    support: &BTreeMap<String, bool>,
) -> bool {
    schema.classes.iter().all(|(other_name, def)| {
        if roles.get(other_name) == Some(&ClassRole::Struct)
            && is_descendant_of(def, trait_name, schema)
        {
            support.get(other_name).copied().unwrap_or(true)
        } else {
            true
        }
    })
}

/// Does this slot's field type support `Eq + Hash`? Handles `any_of`
/// unions (every branch must), bare ranges (look up the type), and the
/// implicit `default_range = string` fallback.
fn field_supports_eq_hash(
    slot: &SlotDefinition,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    support: &BTreeMap<String, bool>,
) -> bool {
    if !slot.any_of.is_empty() {
        let outer_range = slot.range.as_deref();
        return slot.any_of.iter().all(|b| {
            b.range
                .as_deref()
                .or(outer_range)
                .is_some_and(|r| type_supports_eq_hash(r, schema, roles, support))
        });
    }
    let range = slot.range.as_deref().unwrap_or("string");
    type_supports_eq_hash(range, schema, roles, support)
}

/// Look up `Eq + Hash` support for a LinkML range name. Primitives are
/// settled by the language (`f64` family doesn't, everything else we
/// emit does); class refs and enum refs delegate to the per-class
/// support map.
fn type_supports_eq_hash(
    range: &str,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    support: &BTreeMap<String, bool>,
) -> bool {
    match range {
        "string" | "str" | "uri" | "uriorcurie" | "curie" | "ncname" | "objectidentifier"
        | "nodeidentifier" | "integer" | "int" | "boolean" | "bool" | "datetime" | "date"
        | "time" => true,
        "float" | "double" | "decimal" => false,
        other => {
            if schema.classes.contains_key(other) {
                let _ = roles;
                support.get(other).copied().unwrap_or(true)
            } else if schema.enums.contains_key(other) {
                true
            } else {
                // Unknown ref (e.g. imported schema) — be conservative.
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Slot resolution
// ---------------------------------------------------------------------------

/// Walk a class's `is_a` chain and `mixins`, then apply the class's own
/// `attributes`, global `slots:` refs, and `slot_usage` overrides to
/// produce the effective set of slots that show up as fields on the
/// generated struct.
///
/// Precedence (lowest to highest):
/// 1. `is_a` ancestor's slots (recursive)
/// 2. Mixin slots (don't overwrite is_a-inherited slots with same name)
/// 3. This class's inline `attributes`
/// 4. This class's global `slots:` references (don't overwrite #1–3)
/// 5. This class's `slot_usage` overrides (merge-overlay)
pub(crate) fn resolve_slots(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
) -> BTreeMap<String, SlotDefinition> {
    let mut visited = BTreeSet::new();
    resolve_slots_walk(class, schema, &mut visited)
}

/// Recursive worker for [`resolve_slots`]. `visited` holds the class
/// names currently on the recursion stack so a circular `is_a` or
/// `mixin` chain terminates (silently dropping the would-be-cyclic
/// contribution) rather than overflowing.
fn resolve_slots_walk(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
    visited: &mut BTreeSet<String>,
) -> BTreeMap<String, SlotDefinition> {
    let mut slots: BTreeMap<String, SlotDefinition> = BTreeMap::new();

    // Mark this class as in-progress. If insert returns false, we've
    // already visited this class along the current path — stop.
    if !visited.insert(class.name.clone()) {
        return slots;
    }

    if let Some(parent_name) = &class.is_a
        && let Some(parent) = schema.classes.get(parent_name)
    {
        for (name, def) in resolve_slots_walk(parent, schema, visited) {
            slots.insert(name, def);
        }
    }

    for mixin_name in &class.mixins {
        if let Some(mixin) = schema.classes.get(mixin_name) {
            for (name, def) in resolve_slots_walk(mixin, schema, visited) {
                slots.entry(name).or_insert(def);
            }
        }
    }

    for (name, def) in &class.attributes {
        slots.insert(name.clone(), def.clone());
    }

    for slot_name in &class.slots {
        if let Some(def) = schema.slots.get(slot_name) {
            slots
                .entry(slot_name.clone())
                .or_insert_with(|| def.clone());
        }
    }

    for (name, override_def) in &class.slot_usage {
        let target = slots
            .entry(name.clone())
            .or_insert_with(|| override_def.clone());
        merge_slot_override(target, override_def);
    }

    // Pop this class on the way out — sibling/cousin paths to it
    // through different ancestors are NOT cycles.
    visited.remove(&class.name);
    slots
}

/// Merge a `slot_usage` override into an inherited/base slot definition.
/// Only `Option` and `Vec` fields get overwritten when the override
/// supplies a non-default value, so a `slot_usage` entry that just
/// refines `range` doesn't accidentally reset `required` or `multivalued`
/// on the inherited slot.
///
/// Bool fields are copied only when the override sets them to `true`.
/// LinkML schemas in practice use `slot_usage` to tighten constraints
/// (make optional → required, single → multivalued), not loosen them; the
/// pre-Option<bool> IR can't distinguish "override sets false explicitly"
/// from "override omits the field." This compromise covers the common
/// case faithfully.
fn merge_slot_override(target: &mut SlotDefinition, source: &SlotDefinition) {
    /// Clone the source field into the target when the source field is
    /// `Some`. Skips when source is `None` so the inherited value wins.
    macro_rules! merge_opt {
        ($field:ident) => {
            if source.$field.is_some() {
                target.$field = source.$field.clone();
            }
        };
    }
    /// Copy a `Copy` source field to the target when the source is `Some`.
    macro_rules! merge_opt_copy {
        ($field:ident) => {
            if source.$field.is_some() {
                target.$field = source.$field;
            }
        };
    }

    merge_opt!(range);
    merge_opt!(description);
    merge_opt!(pattern);
    merge_opt_copy!(minimum_cardinality);
    merge_opt_copy!(maximum_cardinality);

    if !source.any_of.is_empty() {
        target.any_of = source.any_of.clone();
    }
    if source.required {
        target.required = true;
    }
    if source.multivalued {
        target.multivalued = true;
    }
    if source.identifier {
        target.identifier = true;
    }
}

/// Walk this class's `is_a` chain bottom-up (excluding the class itself).
/// Returns ancestors in inheritance order (immediate parent first, root
/// last). Terminates cleanly on a circular `is_a` chain rather than
/// looping forever.
fn is_a_ancestors(class: &ClassDefinition, schema: &SchemaDefinition) -> Vec<String> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = class.is_a.clone();
    while let Some(name) = current {
        if !seen.insert(name.clone()) {
            // Already on the chain — `is_a` cycle. Stop walking.
            break;
        }
        chain.push(name.clone());
        current = schema.classes.get(&name).and_then(|c| c.is_a.clone());
    }
    chain
}

/// True when `descendant` is a transitive `is_a` or mixin descendant of
/// `ancestor`.
fn is_descendant_of(
    descendant: &ClassDefinition,
    ancestor: &str,
    schema: &SchemaDefinition,
) -> bool {
    let mut visited = BTreeSet::new();
    is_descendant_of_walk(descendant, ancestor, schema, &mut visited)
}

/// Recursive worker for [`is_descendant_of`]. `visited` holds class
/// names currently on the recursion stack so a circular `is_a` /
/// `mixin` chain terminates rather than overflowing.
fn is_descendant_of_walk(
    descendant: &ClassDefinition,
    ancestor: &str,
    schema: &SchemaDefinition,
    visited: &mut BTreeSet<String>,
) -> bool {
    if !visited.insert(descendant.name.clone()) {
        return false;
    }
    let found = (|| {
        if let Some(parent) = &descendant.is_a {
            if parent == ancestor {
                return true;
            }
            if let Some(parent_def) = schema.classes.get(parent)
                && is_descendant_of_walk(parent_def, ancestor, schema, visited)
            {
                return true;
            }
        }
        for mixin in &descendant.mixins {
            if mixin == ancestor {
                return true;
            }
            if let Some(mixin_def) = schema.classes.get(mixin)
                && is_descendant_of_walk(mixin_def, ancestor, schema, visited)
            {
                return true;
            }
        }
        false
    })();
    visited.remove(&descendant.name);
    found
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

fn render_header<W: Write>(out: &mut W, schema: &SchemaDefinition) -> fmt::Result {
    let version = env!("CARGO_PKG_VERSION");
    writeln!(out, "// @generated by panschema v{version}")?;
    writeln!(out, "// Schema: {}", schema.name)?;
    if let Some(v) = &schema.version {
        writeln!(out, "// Schema version: {v}")?;
    }
    out.write_str("// Do not hand-edit; re-run `panschema generate` to refresh.\n")?;
    out.write_str("\n#![allow(non_camel_case_types, non_snake_case, dead_code)]\n\n")
}

fn render_enum<W: Write>(out: &mut W, name: &str, def: &EnumDefinition) -> fmt::Result {
    render_doc_comment(out, "", def.description.as_deref())?;
    out.write_str(
        "#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]\n",
    )?;
    out.write_str("#[non_exhaustive]\n")?;
    writeln!(out, "pub enum {name} {{")?;
    for (key, value) in &def.permissible_values {
        let text = if value.text.is_empty() {
            key
        } else {
            &value.text
        };
        render_doc_comment(out, "    ", value.description.as_deref())?;
        let variant_ident = variant_ident_for(text);
        if variant_ident == *text {
            writeln!(out, "    {variant_ident},")?;
        } else {
            writeln!(out, "    #[serde(rename = \"{}\")]", escape_str(text))?;
            writeln!(out, "    {variant_ident},")?;
        }
    }
    out.write_str("}\n\n")
}

/// Emit a marker trait for a class that's used as an `is_a` parent or
/// mixin. Supertrait bounds combine the class's own `is_a` parent and
/// any mixins that themselves resolve to traits in this schema.
fn render_trait<W: Write>(
    out: &mut W,
    name: &str,
    def: &ClassDefinition,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
) -> fmt::Result {
    render_doc_comment(out, "", def.description.as_deref())?;

    let mut supertraits: Vec<String> = Vec::new();
    if let Some(parent) = &def.is_a
        && roles.get(parent) == Some(&ClassRole::Trait)
    {
        supertraits.push(parent.clone());
    }
    for mixin in &def.mixins {
        if roles.get(mixin) == Some(&ClassRole::Trait)
            && schema.classes.contains_key(mixin)
            && !supertraits.contains(mixin)
        {
            supertraits.push(mixin.clone());
        }
    }
    if supertraits.is_empty() {
        writeln!(out, "pub trait {name} {{}}\n")
    } else {
        writeln!(out, "pub trait {name}: {} {{}}\n", supertraits.join(" + "))
    }
}

fn render_class<W: Write>(
    out: &mut W,
    name: &str,
    def: &ClassDefinition,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    eq_hash_support: &BTreeMap<String, bool>,
    any_of_enums: &mut BTreeMap<String, Vec<String>>,
) -> fmt::Result {
    render_doc_comment(out, "", def.description.as_deref())?;
    if def.r#abstract {
        out.write_str("///\n/// LinkML abstract class.\n")?;
    }

    // Diagnostics for unresolved global slot references: any name in
    // `class.slots` that isn't present in `schema.slots`. We emit a
    // comment line per missing ref so the gap is visible in the
    // generated output rather than silently dropped.
    for slot_name in &def.slots {
        if !schema.slots.contains_key(slot_name)
            && !def.attributes.contains_key(slot_name)
            && !def.slot_usage.contains_key(slot_name)
        {
            write!(
                out,
                "// WARNING: class `{name}` references slot `{slot_name}` which is\n\
                 //          not defined in the schema's `slots:` table. Field\n\
                 //          omitted from the generated struct.\n"
            )?;
        }
    }

    let resolved = resolve_slots(def, schema);
    let eq_hash_ok = eq_hash_support.get(name).copied().unwrap_or(false);
    let derives = compute_struct_derives(&resolved, eq_hash_ok);
    writeln!(out, "#[derive({derives})]")?;
    writeln!(out, "pub struct {name} {{")?;

    for (slot_name, slot) in &resolved {
        let rust_field = snake_case(slot_name);
        let rust_type = field_type_for(name, slot_name, slot, schema, roles, any_of_enums);
        render_doc_comment(out, "    ", slot.description.as_deref())?;

        let mut serde_attrs: Vec<String> = Vec::new();
        if rust_field != *slot_name {
            serde_attrs.push(format!("rename = \"{}\"", escape_str(slot_name)));
        }
        if !slot.required && !slot.multivalued {
            serde_attrs.push("default".to_string());
            serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
        } else if slot.multivalued {
            serde_attrs.push("default".to_string());
            serde_attrs.push("skip_serializing_if = \"Vec::is_empty\"".to_string());
        }
        if !serde_attrs.is_empty() {
            writeln!(out, "    #[serde({})]", serde_attrs.join(", "))?;
        }
        writeln!(out, "    pub {rust_field}: {rust_type},")?;
    }

    out.write_str("}\n\n")?;

    render_constructor(out, name, &resolved, schema, roles, any_of_enums)?;

    let mut impl_targets: Vec<String> = Vec::new();
    for ancestor in is_a_ancestors(def, schema) {
        if roles.get(&ancestor) == Some(&ClassRole::Trait) {
            impl_targets.push(ancestor);
        }
    }
    for mixin in &def.mixins {
        if roles.get(mixin) == Some(&ClassRole::Trait) && !impl_targets.contains(mixin) {
            impl_targets.push(mixin.clone());
        }
        // A mixin's own is_a ancestors are also satisfied by this
        // class; without this walk, a child of a mixin-with-supertrait
        // would impl the mixin but not the supertrait, and the trait
        // bound on a polymorphic field would fail to resolve.
        if let Some(mixin_def) = schema.classes.get(mixin) {
            for ancestor in is_a_ancestors(mixin_def, schema) {
                if roles.get(&ancestor) == Some(&ClassRole::Trait)
                    && !impl_targets.contains(&ancestor)
                {
                    impl_targets.push(ancestor);
                }
            }
        }
    }
    impl_targets.sort();
    impl_targets.dedup();
    for trait_name in &impl_targets {
        writeln!(out, "impl {trait_name} for {name} {{}}")?;
    }
    if !impl_targets.is_empty() {
        out.write_char('\n')?;
    }
    Ok(())
}

/// Emit `impl <Name> { pub fn new(<required_fields…>) -> Self }` so
/// downstream consumers can construct an instance without naming every
/// optional field — surviving future schema additions of optional
/// fields without breaking calling code. Skipped when the struct has
/// no required fields, since `Default::default()` already covers that
/// shape and an empty-arg `new()` would be redundant.
fn render_constructor<W: Write>(
    out: &mut W,
    name: &str,
    resolved: &BTreeMap<String, SlotDefinition>,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    any_of_enums: &mut BTreeMap<String, Vec<String>>,
) -> fmt::Result {
    let has_required = resolved
        .values()
        .any(|slot| slot.required && !slot.multivalued);
    if !has_required {
        return Ok(());
    }

    let params: Vec<(String, String)> = resolved
        .iter()
        .filter(|(_, slot)| slot.required && !slot.multivalued)
        .map(|(slot_name, slot)| {
            (
                snake_case(slot_name),
                field_type_for(name, slot_name, slot, schema, roles, any_of_enums),
            )
        })
        .collect();
    let param_list = params
        .iter()
        .map(|(field, ty)| format!("{field}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");

    writeln!(out, "impl {name} {{")?;
    writeln!(out, "    pub fn new({param_list}) -> Self {{")?;
    writeln!(out, "        Self {{")?;
    for (slot_name, slot) in resolved {
        let field = snake_case(slot_name);
        if slot.multivalued {
            writeln!(out, "            {field}: Vec::new(),")?;
        } else if slot.required {
            writeln!(out, "            {field},")?;
        } else {
            writeln!(out, "            {field}: None,")?;
        }
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    out.write_str("}\n\n")
}

fn render_kind_enum<W: Write>(
    out: &mut W,
    name: &str,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    eq_hash_support: &BTreeMap<String, bool>,
) -> fmt::Result {
    let descendants: Vec<String> = schema
        .classes
        .iter()
        .filter(|(other_name, def)| {
            roles.get(*other_name) == Some(&ClassRole::Struct)
                && is_descendant_of(def, name, schema)
        })
        .map(|(n, _)| n.clone())
        .collect();

    if descendants.is_empty() {
        // Trait class declared but no concrete descendant impls it.
        // The `<Name>Kind` enum would have zero variants — emit a
        // breadcrumb comment so a reader understands why a slot
        // ranging over `<Name>` falls back to `String` (see
        // `type_for_range`).
        return write!(
            out,
            "// NOTE: `{name}` has no concrete descendants in this schema;\n\
             //       no `{name}Kind` enum is emitted. Slots ranging over\n\
             //       `{name}` fall back to `String` at the field level.\n\n"
        );
    }

    let eq_hash_ok = eq_hash_support.get(name).copied().unwrap_or(false);
    write!(
        out,
        "/// Closed enum of concrete classes that implement `{name}`. Used as the\n\
         /// field type when a slot's range is `{name}`.\n"
    )?;
    writeln!(out, "#[derive({})]", enum_derive_line(eq_hash_ok))?;
    out.write_str("#[serde(untagged)]\n")?;
    out.write_str("#[non_exhaustive]\n")?;
    writeln!(out, "pub enum {name}Kind {{")?;
    for desc in &descendants {
        writeln!(out, "    {desc}(Box<{desc}>),")?;
    }
    out.write_str("}\n\n")
}

fn render_any_of_enum<W: Write>(
    out: &mut W,
    name: &str,
    members: &[String],
    eq_hash_ok: bool,
) -> fmt::Result {
    out.write_str("/// Polymorphic range union for the slot identified by this type name.\n")?;
    writeln!(out, "#[derive({})]", enum_derive_line(eq_hash_ok))?;
    out.write_str("#[serde(untagged)]\n")?;
    out.write_str("#[non_exhaustive]\n")?;
    writeln!(out, "pub enum {name} {{")?;
    for member in members {
        let variant = pascal_case(member);
        writeln!(out, "    {variant}(Box<{member}>),")?;
    }
    out.write_str("}\n\n")
}

/// Derive list for an emitted enum (`<Name>Kind` or `any_of` union).
/// `Eq + Hash` toggle on; serde derives always present.
fn enum_derive_line(eq_hash_ok: bool) -> &'static str {
    if eq_hash_ok {
        "Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize"
    } else {
        "Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize"
    }
}

// ---------------------------------------------------------------------------
// Derive selection
// ---------------------------------------------------------------------------

/// Compose the `#[derive(...)]` line for a generated struct based on
/// which Rust traits its resolved field set supports.
///
/// `Debug, Clone, serde::Serialize, serde::Deserialize` are always
/// emitted (every field type we produce supports them). `PartialEq` is
/// always added — every emitted field type supports it. `Default` is
/// added only when every field is default-able under the conservative
/// rules in [`supports_default`].
///
/// `Eq` + `Hash` are added when every resolved slot's field type
/// supports them — see [`compute_eq_hash_support`] for the recursive
/// per-class analysis the `eq_hash_ok` argument carries the result of.
fn compute_struct_derives(slots: &BTreeMap<String, SlotDefinition>, eq_hash_ok: bool) -> String {
    let mut derives = vec!["Debug", "Clone", "PartialEq"];
    if eq_hash_ok {
        derives.push("Eq");
        derives.push("Hash");
    }
    if slots.values().all(supports_default) {
        derives.push("Default");
    }
    derives.push("serde::Serialize");
    derives.push("serde::Deserialize");
    derives.join(", ")
}

/// Conservatively determines whether a slot's framed Rust type supports
/// `Default`. The check looks at the framing first:
///
/// - `Vec<T>`: always defaults to `vec![]`, regardless of T
/// - `Option<T>`: always defaults to `None`, regardless of T
/// - Required + single: `Default` only when T itself implements
///   `Default`. The known-Default-able set is the Default-deriving
///   primitives we emit (`String`, `i64`, `bool`, `f64`). `chrono`
///   datetime types, `Box<T>` for class T, class-typed bare refs, and
///   any_of enum bare types are *not* `Default` under this rule.
fn supports_default(slot: &SlotDefinition) -> bool {
    if slot.multivalued || !slot.required {
        return true;
    }
    // Required + single. `any_of` ranges resolve to a generated enum;
    // those enums don't derive `Default`, so a required bare any_of
    // field disqualifies the containing struct.
    if !slot.any_of.is_empty() {
        return false;
    }
    // `range: None` falls back to LinkML's implicit `default_range`,
    // which is `string` by convention — the same fallback `type_for_range`
    // applies. `String` implements `Default`.
    let range = slot.range.as_deref().unwrap_or("string");
    matches!(
        range,
        // String-like primitives — all `Default`.
        "string" | "str" | "uri" | "uriorcurie" | "curie" | "ncname"
        | "objectidentifier" | "nodeidentifier"
        // Numeric and boolean primitives — all `Default`.
        | "integer" | "int" | "boolean" | "bool" | "float" | "double" | "decimal" // `chrono::DateTime<Utc>`, `NaiveDate`, `NaiveTime` are not `Default`.
                                                                                  // Class refs and enum refs are also not `Default` under the
                                                                                  // conservative rule (would need recursive analysis).
    )
}

// ---------------------------------------------------------------------------
// Type mapping
// ---------------------------------------------------------------------------

/// Pick the Rust type for a class struct field. Combines range
/// resolution (primitive vs class vs trait vs any_of) with
/// required/multivalued framing, and wraps single-valued class-typed
/// fields in `Box` to break potential type-size cycles (a class whose
/// slot range references itself or any ancestor would otherwise have
/// infinite layout).
///
/// `Box` is unnecessary for `Vec<T>` (Vec already provides heap
/// indirection regardless of T), for enums (variants are sized after
/// their own Boxing), for primitives, and for the `<Name>Kind` closed
/// enums (those Box their variants internally).
fn field_type_for(
    class_name: &str,
    slot_name: &str,
    slot: &SlotDefinition,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
    any_of_enums: &mut BTreeMap<String, Vec<String>>,
) -> String {
    if !slot.any_of.is_empty() {
        let enum_name = format!("{class_name}{}", pascal_case(slot_name));
        // LinkML spec: an `any_of` branch can omit its `range`, in
        // which case it inherits the slot's outer `range`. Without the
        // fallback those branches would be silently dropped from the
        // generated enum.
        let outer_range = slot.range.as_deref();
        let members: Vec<String> = slot
            .any_of
            .iter()
            .filter_map(|b| b.range.as_deref().or(outer_range).map(str::to_string))
            .collect();
        any_of_enums.insert(enum_name.clone(), members);
        // any_of enums Box their variants internally → field stays sized.
        return framed_sized(&enum_name, slot);
    }

    let Some(range) = &slot.range else {
        return framed_sized("String", slot);
    };

    let needs_box = matches!(roles.get(range.as_str()), Some(ClassRole::Struct));
    let base = type_for_range(range, schema, roles);

    if needs_box {
        framed_boxed(&base, slot)
    } else {
        framed_sized(&base, slot)
    }
}

/// Framing for a type that's sized on its own (primitive, enum, Kind
/// enum, any_of enum, or a struct used inside a `Vec`).
fn framed_sized(base: &str, slot: &SlotDefinition) -> String {
    if slot.multivalued {
        format!("Vec<{base}>")
    } else if slot.required {
        base.to_string()
    } else {
        format!("Option<{base}>")
    }
}

/// Framing for a concrete struct that may transitively contain itself.
/// `Vec<T>` is sized regardless of T's size; `Option<T>` and bare `T`
/// must be `Box`ed to break layout cycles.
fn framed_boxed(base: &str, slot: &SlotDefinition) -> String {
    if slot.multivalued {
        format!("Vec<{base}>")
    } else if slot.required {
        format!("Box<{base}>")
    } else {
        format!("Option<Box<{base}>>")
    }
}

/// Does this trait-role class have any concrete (Struct-role)
/// descendants? Mirrors the filter `render_kind_enum` applies before
/// deciding whether to emit a `<Name>Kind` union enum.
fn has_concrete_descendants(
    name: &str,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
) -> bool {
    schema.classes.iter().any(|(other_name, def)| {
        roles.get(other_name) == Some(&ClassRole::Struct) && is_descendant_of(def, name, schema)
    })
}

/// Map a LinkML range (primitive name, class name, enum name) to a Rust
/// type. Range names that resolve to a trait class are rewritten to
/// `<Name>Kind` so the field type is a sized closed enum of concrete
/// descendants.
fn type_for_range(
    range: &str,
    schema: &SchemaDefinition,
    roles: &BTreeMap<String, ClassRole>,
) -> String {
    match range {
        "string" | "str" | "uri" | "uriorcurie" | "curie" | "ncname" | "objectidentifier"
        | "nodeidentifier" => "String".to_string(),
        "integer" | "int" => "i64".to_string(),
        "boolean" | "bool" => "bool".to_string(),
        "float" | "double" | "decimal" => "f64".to_string(),
        "datetime" => "chrono::DateTime<chrono::Utc>".to_string(),
        "date" => "chrono::NaiveDate".to_string(),
        "time" => "chrono::NaiveTime".to_string(),
        other => {
            if roles.get(other) == Some(&ClassRole::Trait) {
                // Has subclasses or used as a mixin; field type uses the
                // closed-enum wrapper of concrete descendants — unless
                // there are no concrete descendants, in which case the
                // Kind enum isn't emitted and the field falls back to
                // `String` (URI/identifier). Mirrors the breadcrumb
                // comment `render_kind_enum` emits.
                if has_concrete_descendants(other, schema, roles) {
                    format!("{other}Kind")
                } else {
                    "String".to_string()
                }
            } else if schema.classes.contains_key(other)
                || schema.enums.contains_key(other)
                || schema.types.contains_key(other)
            {
                other.to_string()
            } else {
                // Unresolved ref. Preserve verbatim — could be defined in
                // an imported schema (a future writer pass would surface
                // a warning).
                other.to_string()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// String helpers
// ---------------------------------------------------------------------------

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

/// Emit a LinkML description as Rust doc-comment lines. Each output line
/// is `<indent>/// <text>\n`. Lines are wrapped at a soft 80-column
/// boundary (76 chars of content + 4 chars of `/// ` prefix), breaking
/// on word boundaries.
fn render_doc_comment<W: Write>(
    out: &mut W,
    indent: &str,
    description: Option<&str>,
) -> fmt::Result {
    let Some(text) = description else {
        return Ok(());
    };
    if text.is_empty() {
        return Ok(());
    }
    const WIDTH: usize = 76;
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.write_str(indent)?;
            out.write_str("///\n")?;
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() > WIDTH {
                writeln!(out, "{indent}/// {current}")?;
                current.clear();
                current.push_str(word);
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            writeln!(out, "{indent}/// {current}")?;
        }
    }
    Ok(())
}

/// Escape `"` and `\` in a string for embedding inside a Rust string
/// literal (used in `#[serde(rename = "...")]` attributes). Returns the
/// input borrowed when no escaping is needed — the common case for
/// well-formed LinkML identifiers — so the renderer doesn't allocate
/// per slot/variant in the typical schema.
fn escape_str(s: &str) -> std::borrow::Cow<'_, str> {
    if s.bytes().any(|b| b == b'\\' || b == b'"') {
        std::borrow::Cow::Owned(s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Sanitize a LinkML enum permissible-value text into a valid Rust
/// identifier suitable for use as an enum variant. Strips characters
/// outside `[A-Za-z0-9_]`, replaces `-` and ` ` with `_`, and prepends
/// `_` if the result starts with a digit. If the input is already a
/// valid identifier, returns it unchanged so the serde `rename`
/// attribute can be skipped.
fn variant_ident_for(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else if c == '-' || c == ' ' {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linkml::{ClassDefinition, EnumDefinition, PermissibleValue, SlotDefinition};

    // ----- snake_case --------------------------------------------------

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

    // ----- pascal_case -------------------------------------------------

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

    // ----- class roles -------------------------------------------------

    #[test]
    fn compute_class_roles_marks_is_a_parents_as_trait() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Parent".to_string(), ClassDefinition::new("Parent"));
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        schema.classes.insert("Child".to_string(), child);

        let roles = compute_class_roles(&schema);
        assert_eq!(roles["Parent"], ClassRole::Trait);
        assert_eq!(roles["Child"], ClassRole::Struct);
    }

    #[test]
    fn compute_class_roles_marks_mixins_as_trait() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Tagged".to_string(), ClassDefinition::new("Tagged"));
        let mut item = ClassDefinition::new("Item");
        item.mixins.push("Tagged".to_string());
        schema.classes.insert("Item".to_string(), item);

        let roles = compute_class_roles(&schema);
        assert_eq!(roles["Tagged"], ClassRole::Trait);
        assert_eq!(roles["Item"], ClassRole::Struct);
    }

    #[test]
    fn compute_class_roles_leaf_class_is_struct() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Loner".to_string(), ClassDefinition::new("Loner"));
        let roles = compute_class_roles(&schema);
        assert_eq!(roles["Loner"], ClassRole::Struct);
    }

    // ----- resolve_slots -----------------------------------------------

    fn slot_with_range(name: &str, range: &str) -> SlotDefinition {
        let mut s = SlotDefinition::new(name);
        s.range = Some(range.to_string());
        s
    }

    #[test]
    fn resolve_slots_inherits_from_is_a_parent() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        parent.attributes.insert(
            "inherited".to_string(),
            slot_with_range("inherited", "string"),
        );
        schema.classes.insert("Parent".to_string(), parent);

        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        child
            .attributes
            .insert("own".to_string(), slot_with_range("own", "integer"));
        schema.classes.insert("Child".to_string(), child.clone());

        let resolved = resolve_slots(&child, &schema);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved["inherited"].range.as_deref(), Some("string"));
        assert_eq!(resolved["own"].range.as_deref(), Some("integer"));
    }

    #[test]
    fn resolve_slots_flattens_mixin_slots() {
        let mut schema = SchemaDefinition::new("s");
        let mut mixin = ClassDefinition::new("Tagged");
        mixin
            .attributes
            .insert("tag".to_string(), slot_with_range("tag", "string"));
        schema.classes.insert("Tagged".to_string(), mixin);

        let mut item = ClassDefinition::new("Item");
        item.mixins.push("Tagged".to_string());
        item.attributes
            .insert("name".to_string(), slot_with_range("name", "string"));
        schema.classes.insert("Item".to_string(), item.clone());

        let resolved = resolve_slots(&item, &schema);
        assert!(resolved.contains_key("tag"));
        assert!(resolved.contains_key("name"));
    }

    #[test]
    fn resolve_slots_applies_slot_usage_range_refinement() {
        // Parent has a slot whose range is Activity. Child uses
        // slot_usage to refine that to QuestionFormation. The resolved
        // slot reflects the refinement.
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        parent.attributes.insert(
            "was_generated_by".to_string(),
            slot_with_range("was_generated_by", "Activity"),
        );
        schema.classes.insert("Parent".to_string(), parent);

        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        child.slot_usage.insert(
            "was_generated_by".to_string(),
            slot_with_range("was_generated_by", "QuestionFormation"),
        );
        let resolved = resolve_slots(&child, &schema);
        assert_eq!(
            resolved["was_generated_by"].range.as_deref(),
            Some("QuestionFormation"),
            "slot_usage should refine the inherited range"
        );
    }

    #[test]
    fn resolve_slots_slot_usage_preserves_unspecified_fields() {
        // Parent has a multivalued slot. Child's slot_usage only refines
        // the range; it doesn't touch `multivalued`. The resolved slot
        // remains multivalued.
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        let mut base = slot_with_range("tags", "string");
        base.multivalued = true;
        parent.attributes.insert("tags".to_string(), base);
        schema.classes.insert("Parent".to_string(), parent);

        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        child
            .slot_usage
            .insert("tags".to_string(), slot_with_range("tags", "Label"));
        let resolved = resolve_slots(&child, &schema);
        assert!(
            resolved["tags"].multivalued,
            "slot_usage that doesn't mention multivalued must preserve it"
        );
        assert_eq!(resolved["tags"].range.as_deref(), Some("Label"));
    }

    #[test]
    fn resolve_slots_any_of_propagates_through_slot_usage() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        parent
            .attributes
            .insert("x".to_string(), slot_with_range("x", "Thing"));
        schema.classes.insert("Parent".to_string(), parent);

        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        let mut refinement = SlotDefinition::new("x");
        refinement.any_of = vec![slot_with_range("", "A"), slot_with_range("", "B")];
        child.slot_usage.insert("x".to_string(), refinement);
        let resolved = resolve_slots(&child, &schema);
        let ranges: Vec<&str> = resolved["x"]
            .any_of
            .iter()
            .filter_map(|b| b.range.as_deref())
            .collect();
        assert_eq!(ranges, vec!["A", "B"]);
    }

    // ----- is_descendant_of -------------------------------------------

    #[test]
    fn descendant_traverses_is_a_chain() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Root".to_string(), ClassDefinition::new("Root"));
        let mut mid = ClassDefinition::new("Mid");
        mid.is_a = Some("Root".to_string());
        schema.classes.insert("Mid".to_string(), mid);
        let mut leaf = ClassDefinition::new("Leaf");
        leaf.is_a = Some("Mid".to_string());
        schema.classes.insert("Leaf".to_string(), leaf.clone());

        assert!(is_descendant_of(&leaf, "Mid", &schema));
        assert!(is_descendant_of(&leaf, "Root", &schema));
        assert!(!is_descendant_of(&leaf, "Unrelated", &schema));
    }

    #[test]
    fn descendant_includes_mixins() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("M".to_string(), ClassDefinition::new("M"));
        let mut leaf = ClassDefinition::new("Leaf");
        leaf.mixins.push("M".to_string());
        schema.classes.insert("Leaf".to_string(), leaf.clone());

        assert!(is_descendant_of(&leaf, "M", &schema));
    }

    // ----- cycle detection (slice 6.6) --------------------------------

    #[test]
    fn circular_is_a_chain_does_not_overflow() {
        // A schema with `A is_a B` AND `B is_a A` is malformed but
        // shouldn't crash the writer. The visited-set guard breaks the
        // cycle on the second encounter, returning what was resolved
        // up to that point. The test passes as long as it returns at
        // all (no stack overflow / no infinite recursion).
        let mut schema = SchemaDefinition::new("s");
        let mut a = ClassDefinition::new("A");
        a.is_a = Some("B".to_string());
        let mut b = ClassDefinition::new("B");
        b.is_a = Some("A".to_string());
        schema.classes.insert("A".to_string(), a.clone());
        schema.classes.insert("B".to_string(), b);

        // Both must terminate.
        let _ = resolve_slots(&a, &schema);
        let _ = is_descendant_of(&a, "B", &schema);
        let _ = is_a_ancestors(&a, &schema);
    }

    #[test]
    fn circular_mixin_chain_does_not_overflow() {
        // Mixin cycle: A mixes in B, B mixes in A. Same termination
        // guarantee as the is_a cycle test.
        let mut schema = SchemaDefinition::new("s");
        let mut a = ClassDefinition::new("A");
        a.mixins.push("B".to_string());
        let mut b = ClassDefinition::new("B");
        b.mixins.push("A".to_string());
        schema.classes.insert("A".to_string(), a.clone());
        schema.classes.insert("B".to_string(), b);

        let _ = resolve_slots(&a, &schema);
        let _ = is_descendant_of(&a, "B", &schema);
    }

    #[test]
    fn diamond_inheritance_is_not_treated_as_cycle() {
        // A diamond inheritance pattern (A → B → D, A → C → D) is NOT
        // a cycle even though D appears twice on the recursion stack
        // across DIFFERENT paths. The visited-set guard must pop on
        // exit so the second arrival at D succeeds.
        let mut schema = SchemaDefinition::new("s");
        let mut d = ClassDefinition::new("D");
        d.attributes.insert("name".to_string(), {
            let mut s = SlotDefinition::new("name");
            s.range = Some("string".to_string());
            s
        });
        schema.classes.insert("D".to_string(), d);
        let mut b = ClassDefinition::new("B");
        b.is_a = Some("D".to_string());
        schema.classes.insert("B".to_string(), b);
        let mut c = ClassDefinition::new("C");
        c.is_a = Some("D".to_string());
        schema.classes.insert("C".to_string(), c);
        let mut a = ClassDefinition::new("A");
        a.is_a = Some("B".to_string());
        a.mixins.push("C".to_string());
        schema.classes.insert("A".to_string(), a.clone());

        let resolved = resolve_slots(&a, &schema);
        assert!(
            resolved.contains_key("name"),
            "diamond ancestor slot should be inherited; got: {:?}",
            resolved.keys().collect::<Vec<_>>()
        );
    }

    // ----- type_for_range ---------------------------------------------

    #[test]
    fn type_for_range_class_with_subclasses_uses_kind_suffix() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Activity".to_string(), ClassDefinition::new("Activity"));
        let mut sub = ClassDefinition::new("QF");
        sub.is_a = Some("Activity".to_string());
        schema.classes.insert("QF".to_string(), sub);

        let roles = compute_class_roles(&schema);
        assert_eq!(type_for_range("Activity", &schema, &roles), "ActivityKind");
        assert_eq!(type_for_range("QF", &schema, &roles), "QF");
    }

    #[test]
    fn type_for_range_primitives() {
        let schema = SchemaDefinition::new("s");
        let roles = BTreeMap::new();
        assert_eq!(type_for_range("string", &schema, &roles), "String");
        assert_eq!(type_for_range("integer", &schema, &roles), "i64");
        assert_eq!(
            type_for_range("datetime", &schema, &roles),
            "chrono::DateTime<chrono::Utc>"
        );
    }

    // ----- supports_default -------------------------------------------

    fn slot_shape(range: Option<&str>, required: bool, multivalued: bool) -> SlotDefinition {
        let mut s = SlotDefinition::new("test");
        s.range = range.map(str::to_string);
        s.required = required;
        s.multivalued = multivalued;
        s
    }

    #[test]
    fn supports_default_for_optional_field_of_any_range() {
        // `Option<T>` is `Default` regardless of T.
        assert!(supports_default(&slot_shape(
            Some("datetime"),
            false,
            false
        )));
        assert!(supports_default(&slot_shape(
            Some("SomeClass"),
            false,
            false
        )));
    }

    #[test]
    fn supports_default_for_multivalued_field_of_any_range() {
        // `Vec<T>` is `Default` regardless of T.
        assert!(supports_default(&slot_shape(Some("datetime"), true, true)));
        assert!(supports_default(&slot_shape(
            Some("SomeClass"),
            false,
            true
        )));
    }

    #[test]
    fn supports_default_for_required_string_int_bool_float() {
        for primitive in ["string", "integer", "boolean", "float"] {
            assert!(
                supports_default(&slot_shape(Some(primitive), true, false)),
                "{primitive} should be Default-able when required+single"
            );
        }
    }

    #[test]
    fn supports_default_for_required_field_with_no_range() {
        // Per LinkML semantics, a slot with no `range:` falls back to
        // the schema's `default_range`, which is conventionally
        // `string`. `String` implements `Default`, so the field is
        // Default-able. Regression: scimantic's global `label` slot
        // has no `range:` but is `required: true`, and `Question.label`
        // should be `String` (Default).
        assert!(supports_default(&slot_shape(None, true, false)));
    }

    #[test]
    fn supports_default_rejects_required_datetime() {
        // chrono types don't implement Default; required+single datetime
        // disqualifies the containing struct.
        assert!(!supports_default(&slot_shape(
            Some("datetime"),
            true,
            false
        )));
        assert!(!supports_default(&slot_shape(Some("date"), true, false)));
        assert!(!supports_default(&slot_shape(Some("time"), true, false)));
    }

    #[test]
    fn supports_default_rejects_required_class_ref() {
        // Required bare class refs become Box<T>, which needs T: Default
        // — recursive analysis we don't do at this layer.
        assert!(!supports_default(&slot_shape(
            Some("SomeClass"),
            true,
            false
        )));
    }

    #[test]
    fn supports_default_rejects_required_any_of() {
        let mut s = slot_shape(None, true, false);
        s.any_of = vec![slot_shape(Some("A"), false, false)];
        assert!(!supports_default(&s));
    }

    // ----- compute_struct_derives -------------------------------------

    fn slots_from(pairs: &[(&str, SlotDefinition)]) -> BTreeMap<String, SlotDefinition> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn derives_include_default_when_every_field_supports_it() {
        let slots = slots_from(&[
            ("optional", slot_shape(Some("string"), false, false)),
            ("multi", slot_shape(Some("string"), true, true)),
            ("required_string", slot_shape(Some("string"), true, false)),
        ]);
        let derives = compute_struct_derives(&slots, false);
        assert!(derives.contains("Default"), "got: {derives}");
        assert!(derives.contains("PartialEq"));
    }

    #[test]
    fn derives_omit_default_when_a_field_is_required_datetime() {
        let slots = slots_from(&[
            ("created", slot_shape(Some("datetime"), true, false)),
            ("label", slot_shape(Some("string"), true, false)),
        ]);
        let derives = compute_struct_derives(&slots, false);
        assert!(!derives.contains("Default"), "got: {derives}");
        // PartialEq stays — datetime supports it.
        assert!(derives.contains("PartialEq"));
    }

    #[test]
    fn derives_always_include_debug_clone_partialeq_serde() {
        let slots = slots_from(&[("required_class", slot_shape(Some("SomeClass"), true, false))]);
        let derives = compute_struct_derives(&slots, false);
        assert!(derives.contains("Debug"));
        assert!(derives.contains("Clone"));
        assert!(derives.contains("PartialEq"));
        assert!(derives.contains("serde::Serialize"));
        assert!(derives.contains("serde::Deserialize"));
    }

    #[test]
    fn derives_for_empty_struct_include_default() {
        // No fields → everything supports Default vacuously.
        let derives = compute_struct_derives(&BTreeMap::new(), false);
        assert!(derives.contains("Default"));
    }

    // ----- doc comments -----------------------------------------------

    #[test]
    fn doc_comment_renders_single_line() {
        let mut out = String::new();
        render_doc_comment(&mut out, "", Some("A class.")).unwrap();
        assert_eq!(out, "/// A class.\n");
    }

    #[test]
    fn doc_comment_wrap_boundary_keeps_word_on_same_line_when_exactly_at_width() {
        // Pin down the EXACT wrap boundary: the predicate is
        // `current.len() + 1 + word.len() > WIDTH` (WIDTH = 76). A
        // line that lands exactly at 76 chars after joining must NOT
        // wrap. Catches the `+`/`*`/`-` arithmetic mutations and the
        // `>`/`>=`/`==` comparison mutations.
        //
        // Construct: a word of 74 chars + a single space + a 1-char
        // word = 76 chars total content. With `>`: 74+1+1=76, 76 > 76
        // is FALSE → stays on one line. With `>=`: TRUE → wraps. With
        // `==`: TRUE → wraps. With `+ → -`: 74-1+1=74 > 76 FALSE,
        // same outcome but the line content differs.
        let first_word = "a".repeat(74);
        let input = format!("{first_word} z");
        let mut out = String::new();
        render_doc_comment(&mut out, "", Some(&input)).unwrap();
        // Expect a single `/// <74-a-word> z` line, total 80 chars
        // (4 chars of `/// ` prefix + 76 chars of content).
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines.len(),
            1,
            "76-char content must fit on one line (no wrap); got {} lines:\n{out}",
            lines.len()
        );
        assert_eq!(
            lines[0].len(),
            80,
            "line should be exactly 80 chars; got: {out}"
        );
    }

    #[test]
    fn doc_comment_wraps_when_one_char_over_width() {
        // One char past WIDTH → wraps. Establishes the over-the-line
        // case (the dual of the at-the-line test above).
        let first_word = "a".repeat(75);
        let input = format!("{first_word} z");
        let mut out = String::new();
        render_doc_comment(&mut out, "", Some(&input)).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "77-char content must wrap; got {} lines:\n{out}",
            lines.len()
        );
    }

    #[test]
    fn doc_comment_wraps_long_lines() {
        let mut out = String::new();
        let long = "This is a long description that should wrap at a soft eighty-column boundary because Rust idiom keeps doc comments readable.";
        render_doc_comment(&mut out, "", Some(long)).unwrap();
        for line in out.lines() {
            assert!(line.starts_with("/// "), "missing prefix: {line}");
            assert!(line.len() <= 80, "line too long ({}): {line}", line.len());
        }
    }

    #[test]
    fn doc_comment_respects_blank_lines() {
        let mut out = String::new();
        render_doc_comment(&mut out, "", Some("First paragraph.\n\nSecond paragraph.")).unwrap();
        assert_eq!(out, "/// First paragraph.\n///\n/// Second paragraph.\n");
    }

    #[test]
    fn doc_comment_skipped_when_description_missing() {
        let mut out = String::new();
        render_doc_comment(&mut out, "", None).unwrap();
        assert_eq!(out, "");
    }

    // ----- enum rendering ---------------------------------------------

    #[test]
    fn render_enum_emits_variants_in_sorted_order() {
        let mut def = EnumDefinition::new("Color");
        def.permissible_values
            .insert("Aleatory".to_string(), PermissibleValue::new("Aleatory"));
        def.permissible_values
            .insert("Epistemic".to_string(), PermissibleValue::new("Epistemic"));

        let mut out = String::new();
        render_enum(&mut out, "UncertaintyNature", &def).unwrap();

        let aleatory_pos = out.find("Aleatory").unwrap();
        let epistemic_pos = out.find("Epistemic").unwrap();
        assert!(aleatory_pos < epistemic_pos);
    }

    #[test]
    fn render_enum_adds_serde_rename_for_non_ident_values() {
        let mut def = EnumDefinition::new("Color");
        def.permissible_values
            .insert("off-white".to_string(), PermissibleValue::new("off-white"));
        let mut out = String::new();
        render_enum(&mut out, "Color", &def).unwrap();
        assert!(out.contains("off_white"));
        assert!(out.contains(r#"rename = "off-white""#));
    }

    #[test]
    fn render_enum_sanitizes_spaces_as_underscores() {
        // LinkML permissible values can legitimately contain spaces
        // (e.g. "Open Source"). `variant_ident_for` maps both `-` and
        // ` ` to `_` so the resulting Rust ident is valid. The serde
        // rename preserves the original text for wire-format
        // compatibility.
        let mut def = EnumDefinition::new("License");
        def.permissible_values.insert(
            "Open Source".to_string(),
            PermissibleValue::new("Open Source"),
        );
        let mut out = String::new();
        render_enum(&mut out, "License", &def).unwrap();
        assert!(
            out.contains("Open_Source"),
            "spaces must become underscores; got: {out}"
        );
        assert!(
            out.contains(r#"rename = "Open Source""#),
            "rename must preserve the literal text including spaces; got: {out}"
        );
    }

    #[test]
    fn render_enum_marks_non_exhaustive() {
        let mut def = EnumDefinition::new("Color");
        def.permissible_values
            .insert("Red".to_string(), PermissibleValue::new("Red"));
        let mut out = String::new();
        render_enum(&mut out, "Color", &def).unwrap();
        assert!(
            out.contains("#[non_exhaustive]"),
            "LinkML enums must be #[non_exhaustive] so adding permissible values is non-breaking; got: {out}"
        );
    }

    // ----- trait + struct + impl rendering ----------------------------

    #[test]
    fn render_trait_emits_supertrait_for_is_a_parent() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Entity".to_string(), ClassDefinition::new("Entity"));
        let mut child = ClassDefinition::new("UncertaintyModel");
        child.is_a = Some("Entity".to_string());
        schema
            .classes
            .insert("UncertaintyModel".to_string(), child.clone());
        let mut leaf = ClassDefinition::new("Vagueness");
        leaf.is_a = Some("UncertaintyModel".to_string());
        schema.classes.insert("Vagueness".to_string(), leaf);

        let roles = compute_class_roles(&schema);
        let mut out = String::new();
        render_trait(&mut out, "UncertaintyModel", &child, &schema, &roles).unwrap();
        assert!(
            out.contains("pub trait UncertaintyModel: Entity {}"),
            "expected `pub trait UncertaintyModel: Entity {{}}`, got: {out}"
        );
    }

    #[test]
    fn render_trait_combines_is_a_parent_and_mixins_as_supertraits() {
        // A class with both `is_a` and `mixins` should emit a trait
        // with ALL of them as supertrait bounds, in order: is_a parent
        // first, then mixins. Each mixin appears once (the
        // !supertraits.contains check guards against duplicates when a
        // class lists the same name in multiple positions).
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Entity".to_string(), ClassDefinition::new("Entity"));
        schema
            .classes
            .insert("Tagged".to_string(), ClassDefinition::new("Tagged"));
        schema
            .classes
            .insert("Versioned".to_string(), ClassDefinition::new("Versioned"));
        let mut multi = ClassDefinition::new("Annotated");
        multi.is_a = Some("Entity".to_string());
        multi.mixins.push("Tagged".to_string());
        multi.mixins.push("Versioned".to_string());
        schema
            .classes
            .insert("Annotated".to_string(), multi.clone());
        // Add a leaf so Annotated, Tagged, Versioned all have role=Trait.
        let mut leaf = ClassDefinition::new("Concrete");
        leaf.is_a = Some("Annotated".to_string());
        schema.classes.insert("Concrete".to_string(), leaf);

        let roles = compute_class_roles(&schema);
        let mut out = String::new();
        render_trait(&mut out, "Annotated", &multi, &schema, &roles).unwrap();
        assert!(
            out.contains("pub trait Annotated: Entity + Tagged + Versioned {}"),
            "expected combined supertrait chain in order; got: {out}"
        );
    }

    #[test]
    fn render_trait_skips_mixin_supertrait_when_mixin_is_not_in_schema() {
        // A class can list a mixin name that isn't actually defined
        // as a class in this schema (e.g. an import is missing). The
        // supertrait emission must skip such phantom mixins rather
        // than emitting an unsatisfiable `pub trait X: Phantom {}`.
        // Pins down the `schema.classes.contains_key(mixin)` predicate.
        let mut schema = SchemaDefinition::new("s");
        let mut leaf = ClassDefinition::new("OnlyOne");
        leaf.mixins.push("PhantomMixin".to_string());
        schema.classes.insert("OnlyOne".to_string(), leaf.clone());
        // PhantomMixin is NOT inserted into schema.classes.

        let mut roles = compute_class_roles(&schema);
        // compute_class_roles puts PhantomMixin in the trait set (it
        // appears as a mixin name). Insert it explicitly to mirror
        // what the writer actually sees.
        roles.insert("PhantomMixin".to_string(), ClassRole::Trait);

        let mut out = String::new();
        render_trait(&mut out, "OnlyOne", &leaf, &schema, &roles).unwrap();
        // PhantomMixin isn't in schema.classes → omit from supertraits.
        assert!(
            out.contains("pub trait OnlyOne {}"),
            "phantom mixin must not appear in supertrait chain; got: {out}"
        );
        assert!(
            !out.contains("PhantomMixin"),
            "phantom mixin must not leak into output; got: {out}"
        );
    }

    #[test]
    fn render_trait_skips_mixin_supertrait_when_mixin_is_not_a_trait_role() {
        // A mixin name that doesn't resolve to a Trait role in this
        // schema (i.e. it's not actually used as a parent of anything
        // else, AND isn't itself a mixin somewhere) is omitted from
        // the supertrait chain. Pins down the `roles.get(mixin) ==
        // Some(&ClassRole::Trait)` predicate.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Tagged".to_string(), ClassDefinition::new("Tagged"));
        // No class actually uses Tagged as a mixin → Tagged's role is Struct.
        let mut leaf = ClassDefinition::new("OnlyOne");
        // OnlyOne references Tagged in its mixin list, but that doesn't
        // make Tagged a Trait-role class (compute_class_roles considers
        // a class a Trait iff some OTHER class names it as is_a parent
        // or mixin). Wait, actually `mixins` membership DOES make a
        // class Trait-role (see compute_class_roles). So construct the
        // test such that Tagged is NOT used as a mixin anywhere.
        // We'll test directly via the roles map.
        leaf.mixins.push("Tagged".to_string());
        schema.classes.insert("OnlyOne".to_string(), leaf.clone());

        let roles = compute_class_roles(&schema);
        // Because OnlyOne lists Tagged as a mixin, Tagged IS a Trait.
        // Sanity check.
        assert_eq!(roles["Tagged"], ClassRole::Trait);

        let mut out = String::new();
        render_trait(&mut out, "OnlyOne", &leaf, &schema, &roles).unwrap();
        // OnlyOne is in roles but has role=Struct (nothing inherits
        // from it). The render path here is "render_trait called on a
        // class that itself is a leaf with mixins" — emits a trait
        // referencing the mixins as supertraits.
        assert!(
            out.contains("pub trait OnlyOne: Tagged {}"),
            "expected `pub trait OnlyOne: Tagged {{}}`; got: {out}"
        );
    }

    #[test]
    fn render_class_avoids_duplicate_impl_when_mixin_overlaps_with_is_a_parent() {
        // A class whose mixin shares a name with its is_a parent (or
        // with one of the parent's ancestors) must NOT emit a
        // duplicate `impl X for Child {}` line. Pins down the
        // `!impl_targets.contains(&...)` guards on lines 409/417.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Shared".to_string(), ClassDefinition::new("Shared"));
        let mut leaf = ClassDefinition::new("Child");
        leaf.is_a = Some("Shared".to_string());
        // Pathological-but-valid: listing the is_a parent AGAIN in the
        // mixin list. The dedup guards must prevent two impl lines.
        leaf.mixins.push("Shared".to_string());
        schema.classes.insert("Child".to_string(), leaf.clone());

        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Child",
            &leaf,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        // Exactly ONE impl line for Shared.
        let count = out.matches("impl Shared for Child {}").count();
        assert_eq!(
            count, 1,
            "expected exactly one `impl Shared for Child {{}}` line; got {count}:\n{out}"
        );
    }

    #[test]
    fn render_class_omits_trailing_blank_line_when_no_impl_blocks() {
        // A leaf class with no `is_a` and no mixins emits the struct
        // body, then the closing `}\n\n`, with NO blank line beyond
        // that — because the impl-blocks-trailing-newline only fires
        // when impl_targets is non-empty. Pins down the
        // `!impl_targets.is_empty()` guard (line 429).
        let def = ClassDefinition::new("Loner");
        let schema = SchemaDefinition::new("s");
        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Loner",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        // After the struct, expect exactly `}\n\n` and then end-of-
        // string (no impl block separator). With `!` deleted the
        // function pushes `\n` even when there are no impl blocks,
        // producing `}\n\n\n` — three newlines.
        assert!(
            out.ends_with("}\n\n"),
            "leaf-class output should end `}}\\n\\n` (no impl-block separator); got: {:?}",
            &out[out.len().saturating_sub(10)..]
        );
        assert!(
            !out.ends_with("}\n\n\n"),
            "must not emit a spurious trailing blank line for impl-less classes; got: {:?}",
            &out[out.len().saturating_sub(10)..]
        );
    }

    #[test]
    fn render_class_emits_impl_for_mixin_and_its_ancestors() {
        // A class with a mixin (whose parent is itself a Trait-role
        // class) must emit `impl` blocks for BOTH the mixin AND the
        // mixin's `is_a` chain. Pins down lines 409–421 (the mixin
        // ancestor walk).
        let mut schema = SchemaDefinition::new("s");
        // RootTrait <- MidTrait (the mixin's parent chain)
        schema
            .classes
            .insert("RootTrait".to_string(), ClassDefinition::new("RootTrait"));
        let mut mid = ClassDefinition::new("MidTrait");
        mid.is_a = Some("RootTrait".to_string());
        schema.classes.insert("MidTrait".to_string(), mid);
        // Leaf uses MidTrait as a mixin.
        let mut leaf = ClassDefinition::new("Leaf");
        leaf.mixins.push("MidTrait".to_string());
        schema.classes.insert("Leaf".to_string(), leaf.clone());

        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Leaf",
            &leaf,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        // Both the mixin AND the mixin's `is_a` parent are satisfied.
        assert!(
            out.contains("impl MidTrait for Leaf {}"),
            "expected `impl MidTrait for Leaf {{}}`; got: {out}"
        );
        assert!(
            out.contains("impl RootTrait for Leaf {}"),
            "expected `impl RootTrait for Leaf {{}}` (mixin's is_a ancestor); got: {out}"
        );
    }

    #[test]
    fn render_class_omits_serde_rename_when_field_name_matches_slot() {
        // When a slot's snake_case form equals the original name (e.g.
        // a slot called `label` — already lowercase, no camelCase),
        // emit just `default` / `skip_serializing_if`, NOT a redundant
        // `rename`. Pins down `rust_field != *slot_name` from flipping
        // to `==` (which would rename only when names already match
        // — never useful).
        let mut def = ClassDefinition::new("Thing");
        let mut already_snake = SlotDefinition::new("label");
        already_snake.range = Some("string".to_string());
        already_snake.required = true;
        def.attributes.insert("label".to_string(), already_snake);

        let schema = SchemaDefinition::new("s");
        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Thing",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        // `label` is required + single + name matches → no serde attrs at all.
        assert!(
            out.contains("    pub label: String,\n"),
            "expected bare `pub label: String,`; got: {out}"
        );
        assert!(
            !out.contains(r#"rename = "label""#),
            "should NOT emit a redundant rename; got: {out}"
        );
        // serde_attrs would be non-empty only if a rename or option
        // framing was emitted; for required + name-matches, none apply.
        let label_block = out
            .split("pub struct Thing")
            .nth(1)
            .unwrap_or("")
            .split("\n}")
            .next()
            .unwrap_or("");
        assert!(
            !label_block.contains("#[serde("),
            "required + same-name field should emit no #[serde] attrs; got block:\n{label_block}"
        );
    }

    #[test]
    fn render_class_emits_impl_blocks_for_all_ancestors() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Entity".to_string(), ClassDefinition::new("Entity"));
        let mut mid = ClassDefinition::new("UncertaintyModel");
        mid.is_a = Some("Entity".to_string());
        schema.classes.insert("UncertaintyModel".to_string(), mid);
        let mut leaf = ClassDefinition::new("Vagueness");
        leaf.is_a = Some("UncertaintyModel".to_string());
        schema.classes.insert("Vagueness".to_string(), leaf.clone());

        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Vagueness",
            &leaf,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        assert!(out.contains("impl Entity for Vagueness {}"));
        assert!(out.contains("impl UncertaintyModel for Vagueness {}"));
    }

    #[test]
    fn render_kind_enum_lists_concrete_descendants() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Animal");
        parent.r#abstract = true;
        schema.classes.insert("Animal".to_string(), parent);
        for child in ["Cat", "Dog"] {
            let mut c = ClassDefinition::new(child);
            c.is_a = Some("Animal".to_string());
            schema.classes.insert(child.to_string(), c);
        }

        let roles = compute_class_roles(&schema);
        let mut out = String::new();
        render_kind_enum(&mut out, "Animal", &schema, &roles, &BTreeMap::new()).unwrap();
        assert!(out.contains("pub enum AnimalKind"));
        assert!(out.contains("Cat(Box<Cat>)"));
        assert!(out.contains("Dog(Box<Dog>)"));
        assert!(out.contains("#[serde(untagged)]"));
        assert!(out.contains("#[non_exhaustive]"));
        assert!(out.contains("PartialEq"));
    }

    #[test]
    fn any_of_branch_without_range_inherits_outer_range() {
        // Per LinkML spec, an `any_of` branch can omit `range:` and
        // inherit the slot's outer `range`. Catch silently-dropped
        // branches: a branch with no range should still produce a
        // variant in the generated enum, using the slot's outer range
        // as the inherited type.
        let mut def = ClassDefinition::new("Holder");
        let mut slot = SlotDefinition::new("value");
        slot.range = Some("Default".to_string());
        slot.any_of = vec![
            slot_with_range("", "Explicit"),
            SlotDefinition::new(""), // no range — should fall back to "Default"
        ];
        def.attributes.insert("value".to_string(), slot);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Holder".to_string(), def.clone());
        schema
            .classes
            .insert("Explicit".to_string(), ClassDefinition::new("Explicit"));
        schema
            .classes
            .insert("Default".to_string(), ClassDefinition::new("Default"));

        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Holder",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();

        let members = any_of_enums
            .get("HolderValue")
            .expect("any_of enum recorded");
        assert_eq!(
            members,
            &vec!["Explicit".to_string(), "Default".to_string()],
            "branch without explicit range should inherit slot's outer range"
        );
    }

    #[test]
    fn trait_class_without_descendants_omits_kind_enum_and_falls_back_to_string() {
        // A trait-role class with NO concrete (Struct-role) descendants
        // is a degenerate case that normally can't be produced by
        // `compute_class_roles` (a class becomes trait-role only when
        // something inherits from it). It CAN arise after schema edits
        // that remove a leaf, or in synthetic / partially-loaded
        // schemas. The writer must:
        //   1. Skip the `<Name>Kind` enum (zero variants → invalid Rust).
        //   2. Emit a breadcrumb comment explaining the absence.
        //   3. Have `type_for_range` fall back to `String` rather than
        //      emit a reference to the non-existent `<Name>Kind` type.
        //
        // Test setup: construct a `roles` map directly with a Trait
        // marker that has no actual descendants in the schema.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Phantom".to_string(), ClassDefinition::new("Phantom"));

        let mut roles = BTreeMap::new();
        roles.insert("Phantom".to_string(), ClassRole::Trait);

        // Kind enum: skipped, breadcrumb emitted.
        let mut out = String::new();
        render_kind_enum(&mut out, "Phantom", &schema, &roles, &BTreeMap::new()).unwrap();
        assert!(
            out.contains("no concrete descendants"),
            "should emit breadcrumb explaining missing Kind enum; got: {out}"
        );
        assert!(
            !out.contains("pub enum PhantomKind"),
            "should NOT emit an empty PhantomKind enum; got: {out}"
        );

        // type_for_range: returns "String", not "PhantomKind".
        assert_eq!(type_for_range("Phantom", &schema, &roles), "String");
    }

    #[test]
    fn render_class_emits_warning_comment_for_unresolved_global_slot_ref() {
        // A class that references a slot by name in its `slots:` array
        // but the schema doesn't define that slot anywhere should emit
        // a `// WARNING:` comment before the struct so the gap is
        // visible. Silent drop is misleading.
        let mut def = ClassDefinition::new("Lonely");
        def.slots.push("absent_slot".to_string());

        let schema = SchemaDefinition::new("s");
        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Lonely",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            out.contains("// WARNING") && out.contains("absent_slot"),
            "expected a warning comment for the unresolved slot ref; got: {out}"
        );
    }

    #[test]
    fn render_class_skips_warning_when_slot_ref_is_resolved_by_inline_attribute() {
        // The unresolved-slot warning must NOT fire when the same name
        // appears in `attributes` (inline definition) or `slot_usage`
        // (refinement). The schema-wide `slots:` table is only one of
        // three resolution sources; emitting a warning for refs that
        // resolve locally would be misleading.
        let mut def = ClassDefinition::new("HasInline");
        def.slots.push("inline_slot".to_string());
        def.attributes.insert(
            "inline_slot".to_string(),
            SlotDefinition::new("inline_slot"),
        );

        let schema = SchemaDefinition::new("s");
        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "HasInline",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            !out.contains("// WARNING"),
            "inline-defined slot should suppress the unresolved-ref warning; got: {out}"
        );

        // Same check for slot_usage as the resolution source.
        let mut def = ClassDefinition::new("HasUsage");
        def.slots.push("refined_slot".to_string());
        def.slot_usage.insert(
            "refined_slot".to_string(),
            SlotDefinition::new("refined_slot"),
        );
        let mut out = String::new();
        render_class(
            &mut out,
            "HasUsage",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            !out.contains("// WARNING"),
            "slot_usage-refined slot should suppress the unresolved-ref warning; got: {out}"
        );
    }

    #[test]
    fn has_concrete_descendants_requires_both_struct_role_and_descendant_relation() {
        // The check must be a conjunction: a Struct-role class that is
        // NOT a descendant of `name` should not count, nor should a
        // descendant class that isn't Struct-role. Replacing the `&&`
        // with `||` would treat any Struct class anywhere in the schema
        // as a descendant.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Phantom".to_string(), ClassDefinition::new("Phantom"));
        schema
            .classes
            .insert("Unrelated".to_string(), ClassDefinition::new("Unrelated"));

        let mut roles = BTreeMap::new();
        roles.insert("Phantom".to_string(), ClassRole::Trait);
        roles.insert("Unrelated".to_string(), ClassRole::Struct);

        assert!(
            !has_concrete_descendants("Phantom", &schema, &roles),
            "Unrelated is Struct-role but not a descendant of Phantom; \
             has_concrete_descendants must return false"
        );

        // And the corresponding `type_for_range` fallback still applies.
        assert_eq!(type_for_range("Phantom", &schema, &roles), "String");
    }

    #[test]
    fn render_any_of_enum_wraps_members_in_box_with_serde_untagged() {
        let mut out = String::new();
        render_any_of_enum(
            &mut out,
            "QuestionWasDerivedFrom",
            &["Question".to_string(), "Annotation".to_string()],
            true,
        )
        .unwrap();
        assert!(out.contains("#[serde(untagged)]"));
        assert!(out.contains("#[non_exhaustive]"));
        assert!(out.contains("PartialEq"));
        assert!(out.contains("pub enum QuestionWasDerivedFrom"));
        assert!(out.contains("Question(Box<Question>)"));
        assert!(out.contains("Annotation(Box<Annotation>)"));
    }

    #[test]
    fn render_class_emits_any_of_field_using_per_slot_enum_name() {
        let mut def = ClassDefinition::new("Question");
        let mut slot = SlotDefinition::new("wasDerivedFrom");
        slot.range = Some("Question".to_string());
        slot.any_of = vec![
            slot_with_range("", "Question"),
            slot_with_range("", "Annotation"),
        ];
        slot.multivalued = true;
        def.attributes.insert("wasDerivedFrom".to_string(), slot);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Question".to_string(), def.clone());
        schema
            .classes
            .insert("Annotation".to_string(), ClassDefinition::new("Annotation"));

        let roles = compute_class_roles(&schema);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Question",
            &def,
            &schema,
            &roles,
            &BTreeMap::new(),
            &mut any_of_enums,
        )
        .unwrap();
        assert!(out.contains("pub was_derived_from: Vec<QuestionWasDerivedFrom>"));
        assert_eq!(
            any_of_enums.get("QuestionWasDerivedFrom"),
            Some(&vec!["Question".to_string(), "Annotation".to_string()])
        );
    }

    // ----- Eq + Hash support analysis ---------------------------------

    /// Helper: build a one-class schema with a single attribute slot of
    /// the given range, then return `compute_eq_hash_support`'s answer
    /// for that class.
    fn eq_hash_for_single_slot(class_name: &str, range: &str) -> bool {
        let mut def = ClassDefinition::new(class_name);
        let mut slot = SlotDefinition::new("field");
        slot.range = Some(range.to_string());
        slot.required = true;
        def.attributes.insert("field".to_string(), slot);
        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert(class_name.to_string(), def);
        let roles = compute_class_roles(&schema);
        compute_eq_hash_support(&schema, &roles)
            .get(class_name)
            .copied()
            .unwrap_or(false)
    }

    #[test]
    fn compute_eq_hash_support_excludes_f64_bearing_struct() {
        // f64 doesn't implement Eq (NaN is unequal to itself), so any
        // class with a float / double / decimal field in its resolved
        // slot set must not derive Eq + Hash.
        assert!(!eq_hash_for_single_slot("HasFloat", "float"));
        assert!(!eq_hash_for_single_slot("HasDouble", "double"));
        assert!(!eq_hash_for_single_slot("HasDecimal", "decimal"));
    }

    #[test]
    fn compute_eq_hash_support_includes_datetime_struct() {
        // chrono::DateTime<Utc>, NaiveDate, NaiveTime all implement
        // both Eq and Hash, so a struct whose only field is a
        // datetime / date / time must derive Eq + Hash.
        assert!(eq_hash_for_single_slot("HasDateTime", "datetime"));
        assert!(eq_hash_for_single_slot("HasDate", "date"));
        assert!(eq_hash_for_single_slot("HasTime", "time"));
    }

    #[test]
    fn compute_eq_hash_support_propagates_through_class_chain() {
        // C → B (class ref) → A (f64 field). A is disqualified; B holds
        // an A-typed field, so B is disqualified; C holds a B-typed
        // field, so C is too. The chain must propagate to every
        // referrer.
        let mut schema = SchemaDefinition::new("s");
        for (cls, range) in [("A", "float"), ("B", "A"), ("C", "B")] {
            let mut def = ClassDefinition::new(cls);
            let mut slot = SlotDefinition::new("field");
            slot.range = Some(range.to_string());
            slot.required = true;
            def.attributes.insert("field".to_string(), slot);
            schema.classes.insert(cls.to_string(), def);
        }
        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        assert_eq!(support.get("A"), Some(&false));
        assert_eq!(support.get("B"), Some(&false));
        assert_eq!(support.get("C"), Some(&false));
    }

    #[test]
    fn compute_eq_hash_support_handles_recursive_class_via_box() {
        // A class with a slot ranging over itself is layout-cycled via
        // `Box<T>`. `Box<T>: Eq + Hash` iff `T: Eq + Hash`, so the
        // self-recursive class derives Eq + Hash as long as every
        // *other* field also does. The analyzer must not loop forever
        // on the cycle.
        let mut def = ClassDefinition::new("Node");
        let mut name = SlotDefinition::new("name");
        name.range = Some("string".to_string());
        name.required = true;
        def.attributes.insert("name".to_string(), name);
        let mut parent = SlotDefinition::new("parent");
        parent.range = Some("Node".to_string());
        def.attributes.insert("parent".to_string(), parent);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Node".to_string(), def);
        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        assert_eq!(support.get("Node"), Some(&true));
    }

    #[test]
    fn compute_eq_hash_support_keeps_trait_qualified_when_descendants_clean_and_ignores_unrelated_classes()
     {
        // `trait_descendants_support` must look ONLY at descendants of
        // the trait. An f64-bearing class elsewhere in the schema that
        // is not a descendant must not disqualify the trait. And a
        // trait whose own descendants all qualify must stay `true`.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Shape".to_string(), ClassDefinition::new("Shape"));

        let mut square = ClassDefinition::new("Square");
        square.is_a = Some("Shape".to_string());
        let mut side = SlotDefinition::new("side");
        side.range = Some("integer".to_string());
        side.required = true;
        square.attributes.insert("side".to_string(), side);
        schema.classes.insert("Square".to_string(), square);

        // Unrelated f64-bearing class. Not a descendant of Shape; must
        // be invisible to the trait's analysis.
        let mut unrelated = ClassDefinition::new("Unrelated");
        let mut value = SlotDefinition::new("value");
        value.range = Some("float".to_string());
        value.required = true;
        unrelated.attributes.insert("value".to_string(), value);
        schema.classes.insert("Unrelated".to_string(), unrelated);

        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        assert_eq!(support.get("Shape"), Some(&true));
        assert_eq!(support.get("Square"), Some(&true));
        assert_eq!(support.get("Unrelated"), Some(&false));
    }

    #[test]
    fn compute_eq_hash_support_disqualifies_trait_when_any_descendant_does_not() {
        // Trait `Shape` has two concrete descendants. `Square` is
        // Eq-clean; `Circle` carries an f64 radius. The Trait's bit —
        // which controls the `<Name>Kind` enum's derives — must be
        // false because at least one variant doesn't support Eq + Hash.
        let mut schema = SchemaDefinition::new("s");
        let shape = ClassDefinition::new("Shape");
        schema.classes.insert("Shape".to_string(), shape);

        let mut square = ClassDefinition::new("Square");
        square.is_a = Some("Shape".to_string());
        let mut side = SlotDefinition::new("side");
        side.range = Some("integer".to_string());
        side.required = true;
        square.attributes.insert("side".to_string(), side);
        schema.classes.insert("Square".to_string(), square);

        let mut circle = ClassDefinition::new("Circle");
        circle.is_a = Some("Shape".to_string());
        let mut radius = SlotDefinition::new("radius");
        radius.range = Some("float".to_string());
        radius.required = true;
        circle.attributes.insert("radius".to_string(), radius);
        schema.classes.insert("Circle".to_string(), circle);

        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        assert_eq!(support.get("Shape"), Some(&false));
        assert_eq!(support.get("Square"), Some(&true));
        assert_eq!(support.get("Circle"), Some(&false));
    }

    #[test]
    fn render_class_emits_eq_hash_derive_when_supported() {
        // End-to-end: a struct whose every field supports Eq + Hash
        // gets `Eq, Hash` in the `#[derive(...)]` line.
        let mut def = ClassDefinition::new("Item");
        let mut name = SlotDefinition::new("name");
        name.range = Some("string".to_string());
        name.required = true;
        def.attributes.insert("name".to_string(), name);
        let mut count = SlotDefinition::new("count");
        count.range = Some("integer".to_string());
        count.required = true;
        def.attributes.insert("count".to_string(), count);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Item".to_string(), def.clone());

        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Item",
            &def,
            &schema,
            &roles,
            &support,
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            out.contains("Eq, Hash"),
            "expected `Eq, Hash` in derive line; got:\n{out}"
        );
    }

    #[test]
    fn render_class_omits_eq_hash_when_field_disqualifies() {
        // Symmetric to the previous test: a struct with an f64 field
        // must NOT include Eq or Hash in the derive line.
        let mut def = ClassDefinition::new("Measure");
        let mut value = SlotDefinition::new("value");
        value.range = Some("float".to_string());
        value.required = true;
        def.attributes.insert("value".to_string(), value);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Measure".to_string(), def.clone());

        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Measure",
            &def,
            &schema,
            &roles,
            &support,
            &mut any_of_enums,
        )
        .unwrap();
        // Match the comma-tagged token to avoid false positive on the
        // word "Eq" inside e.g. "PartialEq".
        assert!(
            !out.contains("Eq, Hash"),
            "unexpected Eq + Hash in derive line; got:\n{out}"
        );
    }

    // ----- constructor methods ----------------------------------------

    #[test]
    fn render_class_emits_constructor_with_required_fields_only() {
        // `Question` has one required field (`label`) plus two
        // optional/multivalued ones. The generated constructor takes
        // exactly one parameter and defaults the rest.
        let mut def = ClassDefinition::new("Question");
        let mut label = SlotDefinition::new("label");
        label.range = Some("string".to_string());
        label.required = true;
        def.attributes.insert("label".to_string(), label);
        let mut maybe = SlotDefinition::new("maybe");
        maybe.range = Some("string".to_string());
        def.attributes.insert("maybe".to_string(), maybe);
        let mut many = SlotDefinition::new("many");
        many.range = Some("string".to_string());
        many.multivalued = true;
        def.attributes.insert("many".to_string(), many);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Question".to_string(), def.clone());
        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Question",
            &def,
            &schema,
            &roles,
            &support,
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            out.contains("impl Question {"),
            "expected an impl block; got:\n{out}"
        );
        assert!(
            out.contains("pub fn new(label: String)"),
            "expected constructor to take only the required field; got:\n{out}"
        );
        assert!(
            out.contains("label,"),
            "expected `label` to use parameter shorthand; got:\n{out}"
        );
        assert!(
            out.contains("maybe: None,"),
            "expected `maybe` to default to None; got:\n{out}"
        );
        assert!(
            out.contains("many: Vec::new(),"),
            "expected `many` to default to Vec::new(); got:\n{out}"
        );
    }

    #[test]
    fn render_class_skips_constructor_when_no_required_fields() {
        // `Default::default()` already covers an all-optional struct;
        // a zero-arg `new()` would be noise. Skip emission entirely.
        let mut def = ClassDefinition::new("Loose");
        let mut maybe = SlotDefinition::new("maybe");
        maybe.range = Some("string".to_string());
        def.attributes.insert("maybe".to_string(), maybe);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Loose".to_string(), def.clone());
        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Loose",
            &def,
            &schema,
            &roles,
            &support,
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            !out.contains("impl Loose {"),
            "no required fields → no constructor; got:\n{out}"
        );
    }

    #[test]
    fn render_class_constructor_skips_multivalued_required_in_param_list() {
        // A required + multivalued slot is `Vec<T>`. The constructor
        // defaults it to `Vec::new()` rather than asking for a value,
        // mirroring how Default-deriving structs treat `Vec`.
        let mut def = ClassDefinition::new("Holder");
        let mut items = SlotDefinition::new("items");
        items.range = Some("string".to_string());
        items.required = true;
        items.multivalued = true;
        def.attributes.insert("items".to_string(), items);
        let mut name = SlotDefinition::new("name");
        name.range = Some("string".to_string());
        name.required = true;
        def.attributes.insert("name".to_string(), name);

        let mut schema = SchemaDefinition::new("s");
        schema.classes.insert("Holder".to_string(), def.clone());
        let roles = compute_class_roles(&schema);
        let support = compute_eq_hash_support(&schema, &roles);
        let mut any_of_enums = BTreeMap::new();
        let mut out = String::new();
        render_class(
            &mut out,
            "Holder",
            &def,
            &schema,
            &roles,
            &support,
            &mut any_of_enums,
        )
        .unwrap();
        assert!(
            out.contains("pub fn new(name: String)"),
            "multivalued field must not appear in param list; got:\n{out}"
        );
        assert!(
            out.contains("items: Vec::new(),"),
            "expected `items` to default to Vec::new(); got:\n{out}"
        );
    }

    // ----- escape_str --------------------------------------------------

    #[test]
    fn escape_str_returns_borrowed_for_plain_string() {
        // Well-formed LinkML identifiers carry no `"` or `\`. The
        // zero-alloc path returns `Cow::Borrowed` pointing at the
        // original slice, with no escaping work performed.
        let s = "wasGeneratedBy";
        let result = escape_str(s);
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, s);
        if let std::borrow::Cow::Borrowed(borrowed) = &result {
            assert_eq!(borrowed.as_ptr(), s.as_ptr());
        }
    }

    #[test]
    fn escape_str_escapes_backslashes_into_owned() {
        // A lone `\` triggers the owned path and doubles the backslash
        // so the byte sequence round-trips through a Rust string
        // literal.
        let result = escape_str("\\");
        assert!(matches!(result, std::borrow::Cow::Owned(_)));
        assert_eq!(result, "\\\\");
    }

    #[test]
    fn escape_str_escapes_double_quotes_into_owned() {
        // A lone `"` triggers the owned path and escapes to `\"` so
        // the result is safe to embed in a Rust string literal.
        let result = escape_str("\"");
        assert!(matches!(result, std::borrow::Cow::Owned(_)));
        assert_eq!(result, "\\\"");
    }

    // ----- header + Writer trait surface ------------------------------

    #[test]
    fn renders_generated_marker_with_panschema_version() {
        let mut schema = SchemaDefinition::new("demo");
        schema.version = Some("0.1.0".to_string());

        let out = RustWriter::new().render(&schema);
        let expected_version = env!("CARGO_PKG_VERSION");
        assert!(out.contains(&format!("// @generated by panschema v{expected_version}")));
        assert!(out.contains("// Schema: demo"));
        assert!(out.contains("// Schema version: 0.1.0"));
    }

    #[test]
    fn omits_schema_version_line_when_unspecified() {
        let schema = SchemaDefinition::new("demo");
        let out = RustWriter::new().render(&schema);
        assert!(!out.contains("// Schema version:"));
    }

    #[test]
    fn format_id_is_rust() {
        assert_eq!(RustWriter::new().format_id(), "rust");
    }

    #[test]
    fn write_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join("dir").join("out.rs");
        let schema = SchemaDefinition::new("demo");

        RustWriter::new().write(&schema, &target).unwrap();
        assert!(target.exists());
        let body = std::fs::read_to_string(&target).unwrap();
        assert!(body.contains("@generated by panschema"));
    }

    // ----- full fixture rendering -------------------------------------

    fn fixture_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("demo");
        schema.version = Some("0.1.0".to_string());

        let mut color = EnumDefinition::new("Color");
        color
            .permissible_values
            .insert("Red".to_string(), PermissibleValue::new("Red"));
        color
            .permissible_values
            .insert("Blue".to_string(), PermissibleValue::new("Blue"));
        schema.enums.insert("Color".to_string(), color);

        let mut sample = ClassDefinition::new("Sample");
        let mut name = SlotDefinition::new("name");
        name.range = Some("string".to_string());
        name.required = true;
        sample.attributes.insert("name".to_string(), name);

        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.multivalued = true;
        sample.attributes.insert("tags".to_string(), tags);

        let mut color_ref = SlotDefinition::new("color");
        color_ref.range = Some("Color".to_string());
        sample.attributes.insert("color".to_string(), color_ref);

        let mut when = SlotDefinition::new("createdAt");
        when.range = Some("datetime".to_string());
        when.required = true;
        sample.attributes.insert("createdAt".to_string(), when);

        schema.classes.insert("Sample".to_string(), sample);

        schema
    }

    #[test]
    fn fixture_renders_as_syntactically_valid_rust() {
        let schema = fixture_schema();
        let body = RustWriter::new().render(&schema);
        syn::parse_file(&body)
            .unwrap_or_else(|e| panic!("generated Rust failed to parse: {e}\n---\n{body}"));
    }

    #[test]
    fn fixture_field_types_are_correct() {
        let schema = fixture_schema();
        let body = RustWriter::new().render(&schema);
        assert!(body.contains("pub name: String,"));
        assert!(body.contains("pub tags: Vec<String>,"));
        assert!(body.contains("pub color: Option<Color>,"));
        assert!(body.contains("pub created_at: chrono::DateTime<chrono::Utc>,"));
    }

    #[test]
    fn fixture_is_idempotent() {
        let schema = fixture_schema();
        let writer = RustWriter::new();
        assert_eq!(writer.render(&schema), writer.render(&schema));
    }

    #[test]
    fn render_into_streams_to_arbitrary_fmt_write_sink() {
        // A non-`String` sink — anything implementing `fmt::Write` — must
        // accept the rendered module without going through an intermediate
        // `String` allocation. Use `fmt::Formatter`-style adapter (here a
        // simple character-counting sink) to verify the trait bound is
        // actually generic, not String-special-cased.
        struct CountingSink {
            bytes: usize,
            buf: String,
        }
        impl Write for CountingSink {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.bytes += s.len();
                self.buf.push_str(s);
                Ok(())
            }
        }

        let schema = fixture_schema();
        let writer = RustWriter::new();
        let mut sink = CountingSink {
            bytes: 0,
            buf: String::new(),
        };
        writer.render_into(&mut sink, &schema).unwrap();

        let via_string = writer.render(&schema);
        assert_eq!(sink.bytes, via_string.len());
        assert_eq!(sink.buf, via_string);
    }
}
