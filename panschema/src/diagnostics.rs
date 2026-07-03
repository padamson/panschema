//! Diagnostics for LinkML constructs panschema parses but does not model.
//!
//! `serde` silently ignores unknown YAML keys, so a producer can write a
//! real constraint (`rules`, `unique_keys`, a boolean class expression)
//! and ship a schema where it is quietly dropped. [`ClassDefinition`]
//! captures such keys in its `unmodeled` catch-all; this module warns on
//! them.
//!
//! The guard warns by **default**: the ignore-list starts empty, so every
//! unmodeled key is reported until a specific one is identified as safe to
//! silence. That direction is deliberate — an allowlist could only catch
//! drops we already anticipated, leaving the exact blind spot the guard
//! exists to close.
//!
//! [`ClassDefinition`]: crate::linkml::ClassDefinition

use crate::linkml::SchemaDefinition;

/// Class-level LinkML keys panschema parses but deliberately does NOT
/// warn about — a **denylist that starts empty**.
///
/// Every unmodeled key warns until a specific key is identified as one
/// whose non-rendering is correct-by-definition (LinkML's equivalent of a
/// code comment) and added here *with its reason*. Starting empty is the
/// honest default: panschema surfaces every construct it doesn't handle,
/// and we silence individual keys only on evidence, never speculatively.
/// Never add a semantic/constraint construct here — model it, or let it
/// warn. See `docs/linkml-coverage.md`.
const IGNORED_CLASS_KEYS: &[&str] = &[];

/// One unmodeled key found on a class: the key and the class carrying it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmodeledConstruct {
    /// The class the construct was written on.
    pub class: String,
    /// The LinkML key that is parsed but not modeled (and not ignored).
    pub construct: String,
}

impl UnmodeledConstruct {
    /// A user-facing warning line.
    pub fn message(&self) -> String {
        format!(
            "`{}` on class `{}` is parsed but not modeled; it will not render or emit",
            self.construct, self.class
        )
    }
}

/// Report every class key that panschema parsed but did not model,
/// except the known-harmless ones, in a deterministic order (by class
/// name, then by key).
pub fn unmodeled_class_constructs(schema: &SchemaDefinition) -> Vec<UnmodeledConstruct> {
    scan(schema, IGNORED_CLASS_KEYS)
}

/// Whether `generate` should fail rather than merely warn: true only when
/// strict mode is on and at least one unmodeled construct was found.
pub fn should_fail_strict(findings: &[UnmodeledConstruct], strict: bool) -> bool {
    strict && !findings.is_empty()
}

/// The detection mechanism, parameterized by the ignore-list so tests can
/// exercise it with fabricated keys decoupled from the real list. Warns
/// by default: an unmodeled key is reported unless it is in `ignored`.
fn scan(schema: &SchemaDefinition, ignored: &[&str]) -> Vec<UnmodeledConstruct> {
    let mut found = Vec::new();
    // `classes` and each `unmodeled` map are BTreeMaps, so iteration is
    // name-sorted → a stable report.
    for (class_name, class) in &schema.classes {
        for key in class.unmodeled.keys() {
            if ignored.contains(&key.as_str()) {
                continue;
            }
            found.push(UnmodeledConstruct {
                class: class_name.clone(),
                construct: key.clone(),
            });
        }
    }
    found
}

/// Classes whose `rules` won't appear in RDF/OWL output.
///
/// This is a second, narrower class of silent drop than
/// [`unmodeled_class_constructs`]: `rules` is IR-modeled (feature 17 slice
/// 1), so it never reaches the `unmodeled` catch-all and that guard stays
/// silent about it — but no writer projects `rules` to RDF yet (deferred,
/// feature 17 slice 4), so a schema with `rules` renders them in HTML and
/// drops them from every RDF format with no signal. Call this when
/// generating a non-HTML format so that gap stays loud too, until slice 4
/// closes it.
pub fn classes_with_rules_unsupported_in_rdf(schema: &SchemaDefinition) -> Vec<String> {
    schema
        .classes
        .iter()
        .filter(|(_, class)| !class.rules.is_empty())
        .map(|(name, _)| name.clone())
        .collect()
}

/// A `unique_keys` slot that doesn't resolve to any slot on its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedKeySlot {
    /// The class carrying the `unique_keys` entry.
    pub class: String,
    /// The `unique_keys` entry (map key) naming the constraint.
    pub key: String,
    /// The referenced slot name that isn't in the class's effective set.
    pub slot: String,
}

impl UnresolvedKeySlot {
    /// A user-facing warning line.
    pub fn message(&self) -> String {
        format!(
            "unique key `{}` on class `{}` references slot `{}`, which the class does not have",
            self.key, self.class, self.slot
        )
    }
}

/// Report every `unique_keys` slot that names a slot the class doesn't
/// actually have, checked against its *effective* slot set (inherited +
/// mixin + inline + `slot_usage`), in deterministic order.
///
/// A structural check with no home yet: feature 07's `validate` surface
/// isn't built, so this routes through the same `generate`-time
/// `eprintln!` warning path as the other diagnostics until it lands.
pub fn unresolved_unique_key_slots(schema: &SchemaDefinition) -> Vec<UnresolvedKeySlot> {
    let mut found = Vec::new();
    for (class_name, class) in &schema.classes {
        if class.unique_keys.is_empty() {
            continue;
        }
        let effective = crate::linkml_resolve::resolve_effective_slots(class, schema);
        for (key_name, key) in &class.unique_keys {
            for slot in &key.unique_key_slots {
                if !effective.contains_key(slot) {
                    found.push(UnresolvedKeySlot {
                        class: class_name.clone(),
                        key: key_name.clone(),
                        slot: slot.clone(),
                    });
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(yaml: &str) -> SchemaDefinition {
        serde_yaml::from_str(yaml).expect("parse schema")
    }

    // Fabricated key — never a real LinkML key — so these mechanism
    // tests stay valid regardless of which real keys are modeled or added
    // to the ignore-list over time.
    const UNKNOWN_KEY: &str = "panschema_test_unmodeled_key";

    #[test]
    fn warns_on_any_unmodeled_key_by_default() {
        // The guard's whole point: an unmodeled key we never enumerated is
        // reported anyway (empty ignore-list ⇒ warn).
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        assert_eq!(
            scan(&schema, &[]),
            vec![UnmodeledConstruct {
                class: "C".to_string(),
                construct: UNKNOWN_KEY.to_string(),
            }]
        );
    }

    #[test]
    fn silences_a_key_on_the_ignore_list() {
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        assert!(scan(&schema, &[UNKNOWN_KEY]).is_empty());
    }

    #[test]
    fn public_fn_reports_unmodeled_keys_through_the_real_ignore_list() {
        // Pins the public entry point (real, empty ignore-list) to
        // actually scan and report — not return nothing.
        let schema = parse(&format!("name: s\nclasses:\n  C:\n    {UNKNOWN_KEY}: []\n"));
        let found = unmodeled_class_constructs(&schema);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].construct, UNKNOWN_KEY);
        assert_eq!(found[0].class, "C");
    }

    #[test]
    fn message_names_the_construct_and_class() {
        let msg = UnmodeledConstruct {
            class: "Deployment".to_string(),
            construct: "rules".to_string(),
        }
        .message();
        assert!(
            msg.contains("rules") && msg.contains("Deployment"),
            "message must name the construct and class; got: {msg}"
        );
    }

    #[test]
    fn strict_fails_only_when_strict_and_findings_present() {
        let some = vec![UnmodeledConstruct {
            class: "C".to_string(),
            construct: "rules".to_string(),
        }];
        let none: Vec<UnmodeledConstruct> = Vec::new();
        assert!(should_fail_strict(&some, true), "strict + findings ⇒ fail");
        assert!(!should_fail_strict(&some, false), "not strict ⇒ never fail");
        assert!(
            !should_fail_strict(&none, true),
            "strict + no findings ⇒ ok"
        );
        assert!(!should_fail_strict(&none, false));
    }

    #[test]
    fn classes_with_rules_unsupported_in_rdf_lists_classes_with_rules() {
        let schema = parse(
            "name: s\nclasses:\n  Deployment:\n    rules:\n      - description: d\n  Bare:\n    description: no rules here\n",
        );
        assert_eq!(
            classes_with_rules_unsupported_in_rdf(&schema),
            vec!["Deployment".to_string()]
        );
    }

    #[test]
    fn classes_with_rules_unsupported_in_rdf_empty_when_no_rules() {
        let schema = parse("name: s\nclasses:\n  Bare:\n    description: x\n");
        assert!(classes_with_rules_unsupported_in_rdf(&schema).is_empty());
    }

    // The resolver keys cycle-detection on `ClassDefinition.name`, which the
    // YAML reader backfills from the map key before any diagnostic runs;
    // these tests build classes with names already set to match that
    // precondition (the raw `parse` helper skips backfill).
    use crate::linkml::{ClassDefinition, SlotDefinition, UniqueKey};

    fn class_with_attr(name: &str, attr: &str) -> ClassDefinition {
        let mut c = ClassDefinition::new(name);
        c.attributes
            .insert(attr.to_string(), SlotDefinition::new(attr));
        c
    }

    #[test]
    fn unresolved_unique_key_slots_flags_a_slot_the_class_lacks() {
        // `offered_by` is a real attribute; `ghost` is not — only the
        // latter is flagged, and it names the class, key, and slot.
        let mut schema = SchemaDefinition::new("s");
        let mut offering = class_with_attr("Offering", "offered_by");
        offering.unique_keys.insert(
            "k".to_string(),
            UniqueKey {
                unique_key_slots: vec!["offered_by".to_string(), "ghost".to_string()],
                description: None,
            },
        );
        schema.classes.insert("Offering".to_string(), offering);
        assert_eq!(
            unresolved_unique_key_slots(&schema),
            vec![UnresolvedKeySlot {
                class: "Offering".to_string(),
                key: "k".to_string(),
                slot: "ghost".to_string(),
            }]
        );
    }

    #[test]
    fn unresolved_unique_key_slots_resolves_inherited_slots() {
        // A key slot defined on an `is_a` parent is in the effective set,
        // so it does not warn.
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Base".to_string(), class_with_attr("Base", "name"));
        let mut sub = ClassDefinition::new("Sub");
        sub.is_a = Some("Base".to_string());
        sub.unique_keys.insert(
            "k".to_string(),
            UniqueKey {
                unique_key_slots: vec!["name".to_string()],
                description: None,
            },
        );
        schema.classes.insert("Sub".to_string(), sub);
        assert!(
            unresolved_unique_key_slots(&schema).is_empty(),
            "an inherited slot must resolve"
        );
    }

    #[test]
    fn unresolved_unique_key_slots_message_names_class_key_slot() {
        let msg = UnresolvedKeySlot {
            class: "Offering".to_string(),
            key: "k".to_string(),
            slot: "ghost".to_string(),
        }
        .message();
        assert!(
            msg.contains("Offering") && msg.contains("`k`") && msg.contains("ghost"),
            "message must name class, key, and slot; got: {msg}"
        );
    }

    #[test]
    fn silent_on_modeled_keys() {
        // Modeled keys map to named fields and never reach the `unmodeled`
        // catch-all, so they never warn — independent of the (currently
        // empty) ignore-list.
        let schema = parse(
            "name: s\nclasses:\n  C:\n    description: d\n    abstract: true\n    mixins: [M]\n",
        );
        assert!(
            unmodeled_class_constructs(&schema).is_empty(),
            "modeled keys must not warn"
        );
    }
}
