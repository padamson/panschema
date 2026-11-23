//! What every reported finding says about itself: a kind a program can
//! match on, the place in the data it is about, and the sentence a person
//! reads.

/// Something a check found, in a form a program can act on.
///
/// A caller that treats findings differently matches [`Finding::kind`],
/// which names the case and stays the same across releases, where the
/// wording of [`Finding::message`] is free to change.
pub trait Finding {
    /// The finding's kind, one of [`kind::ALL`]: `<family>/<case>`.
    fn kind(&self) -> &'static str;

    /// What in the data the finding is about: a record's id, an entry's
    /// place within a record (`q-07.claims[0]`), or the IRI several
    /// datasets share. `None` for a finding about a dataset as a whole.
    fn place(&self) -> Option<&str>;

    /// The sentence a person reads.
    fn message(&self) -> String;
}

/// Implement [`Finding`] for a type that has one kind, names its place in
/// one `String` field, and already renders its sentence as `message()`.
macro_rules! finding {
    ($type:ty, $kind:expr, $place:ident) => {
        impl $crate::findings::Finding for $type {
            fn kind(&self) -> &'static str {
                $kind
            }
            fn place(&self) -> Option<&str> {
                Some(&self.$place)
            }
            fn message(&self) -> String {
                <$type>::message(self)
            }
        }
    };
}
pub(crate) use finding;

/// Define each kind as a constant and list them all in `ALL`, so a kind
/// cannot exist without being listed.
macro_rules! kinds {
    ($($(#[$doc:meta])* $name:ident = $text:literal;)*) => {
        $($(#[$doc])* pub const $name: &str = $text;)*

        /// Every kind a finding can report.
        pub const ALL: &[&str] = &[$($name),*];
    };
}

/// The kinds a finding can report. A kind is `<family>/<case>`; a family
/// of schema-declared claims is named as
/// [`SLOT_SEMANTICS_FAMILIES`](crate::diagnostics::SLOT_SEMANTICS_FAMILIES)
/// names it. Adding a kind is not a breaking change; changing one is.
pub mod kind {
    /// The family a kind belongs to: the part before its `/`.
    pub fn family(kind: &str) -> &str {
        kind.split_once('/').map_or(kind, |(family, _)| family)
    }

    kinds! {
        /// A required slot has no value on the record.
        REQUIRED_SLOT_ABSENT = "conformance/required-slot-absent";
        /// A single-valued slot holds more than one value.
        SINGLE_VALUED_SLOT_REPEATED = "conformance/single-valued-slot-repeated";
        /// A multivalued slot holds fewer values than its declared minimum.
        FEWER_VALUES_THAN_MINIMUM = "conformance/fewer-values-than-minimum";
        /// A multivalued slot holds more values than its declared maximum.
        MORE_VALUES_THAN_MAXIMUM = "conformance/more-values-than-maximum";
        /// A type designator names a class other than the record's own.
        DESIGNATOR_NAMES_ANOTHER_CLASS = "conformance/designator-names-another-class";
        /// A type designator holds a value that is not a string.
        DESIGNATOR_NOT_A_STRING = "conformance/designator-not-a-string";
        /// The schema's pattern for the slot is not a valid regular expression.
        INVALID_PATTERN = "conformance/invalid-pattern";
        /// An object at a union of class ranges names none of them.
        OBJECT_NAMES_NO_CLASS_RANGE = "conformance/object-names-no-class-range";
        /// A value whose shape the slot's range cannot take.
        VALUE_SHAPE_OUTSIDE_RANGE = "conformance/value-shape-outside-range";
        /// A reference whose target's class is no member of the union range.
        REFERENCE_OUTSIDE_UNION = "conformance/reference-outside-union";
        /// A scalar of a kind the range's primitives do not include.
        SCALAR_KIND_OUTSIDE_RANGE = "conformance/scalar-kind-outside-range";
        /// A value the slot's range enum does not permit.
        NOT_A_PERMISSIBLE_VALUE = "conformance/not-a-permissible-value";
        /// A string the slot's pattern does not match.
        PATTERN_MISMATCH = "conformance/pattern-mismatch";
        /// A number below the slot's declared minimum.
        BELOW_MINIMUM = "conformance/below-minimum";
        /// A number above the slot's declared maximum.
        ABOVE_MAXIMUM = "conformance/above-maximum";
        /// A non-numeric value at a slot that declares a numeric bound.
        NOT_NUMERIC_UNDER_BOUND = "conformance/not-numeric-under-bound";
        /// A value of a specializing slot that its parent slot does not hold.
        OUTSIDE_SPECIALIZED_SLOT = "conformance/outside-specialized-slot";
        /// A rule whose precondition holds and whose postcondition fails.
        RULE_POSTCONDITION_FAILED = "conformance/rule-postcondition-failed";
        /// A rule none of whose postcondition alternatives the record satisfies.
        RULE_ALTERNATIVES_FAILED = "conformance/rule-alternatives-failed";
        /// A typed reference to an id no record in the dataset defines.
        DANGLING_REFERENCE = "conformance/dangling-reference";
        /// A field the record's class never declared.
        UNDECLARED_FIELD = "conformance/undeclared-field";
        /// The data matched none of the schema's `tree_root` classes, or several.
        NO_ROOT_MATCHED = "conformance/no-root-matched";
        /// A record whose bare values could not expand against their base.
        EXPANSION_GAP = "conformance/expansion-gap";
        /// An identifier used by more than one record.
        DUPLICATE_IDENTIFIER = "conformance/duplicate-identifier";
        /// A collection entry that loaded nothing.
        UNUSABLE_COLLECTION_ENTRY = "conformance/unusable-collection-entry";
        /// Records authored inline at a slot the schema declares as references.
        UNDECLARED_INLINING = "conformance/undeclared-inlining";
        /// The dataset container's id is also a record's id.
        ROOT_ID_COLLIDES_WITH_RECORD = "conformance/root-id-collides-with-record";
        /// The data is not a mapping, so it cannot be a `tree_root` container.
        DATA_NOT_A_MAPPING = "conformance/data-not-a-mapping";

        /// A reference into a sibling's namespace that no sibling record mints.
        REFERENCE_UNRESOLVED = "reference/unresolved";
        /// A reference landing in no namespace a sibling owns.
        REFERENCE_UNCOVERED = "reference/uncovered";

        /// A stated absence a sibling record contradicts.
        ABSENCE_CLAIM_CONTRADICTED = "absence-claim/contradicted";
        /// A stated absence that could not be evaluated.
        ABSENCE_CLAIM_UNCHECKABLE = "absence-claim/uncheckable";
        /// Stated absences no sibling graph was loaded to check.
        ABSENCE_CLAIM_UNCHECKED = "absence-claim/unchecked";

        /// A version pin that disagrees with the version its sibling resolved to.
        VERSION_PIN_MISMATCHED = "version-pin/mismatched";
        /// A version pin that could not be evaluated.
        VERSION_PIN_UNCHECKABLE = "version-pin/uncheckable";
        /// Version pins no sibling graph was loaded to check.
        VERSION_PIN_UNCHECKED = "version-pin/unchecked";

        /// A value claim whose record carries other values at the slot.
        VALUE_CLAIM_DIFFERS = "value-claim/differs";
        /// A value claim whose record sets no value at the slot.
        VALUE_CLAIM_UNSET = "value-claim/unset";
        /// A value claim naming a slot the record's class does not carry.
        VALUE_CLAIM_SLOT_NOT_CARRIED = "value-claim/slot-not-carried";
        /// A value claim about a record outside the `among` values.
        VALUE_CLAIM_NOT_AMONG = "value-claim/not-among";
        /// A value claim that could not be evaluated.
        VALUE_CLAIM_UNCHECKABLE = "value-claim/uncheckable";
        /// Value claims no sibling graph was loaded to check.
        VALUE_CLAIM_UNCHECKED = "value-claim/unchecked";

        /// One IRI minted by more than one dataset.
        DATASET_IRI_COLLISION = "dataset/iri-collision";
        /// One record that several datasets define and scope apart.
        DATASET_UNINTENDED_SPLIT = "dataset/unintended-split";

        /// A warning the schema's load raised. Every one shares this kind
        /// until each says its own.
        SCHEMA_LOAD_WARNING = "schema/load-warning";

        /// A schema declares claim slots, but its `[check.<name>]` names no
        /// `resolve_against` siblings to check them against.
        CHECK_NO_RESOLVE_AGAINST = "check/no-resolve-against";
        /// A `resolve_against` sibling declares no datasets, so no reference
        /// into its namespace can resolve.
        CHECK_SIBLING_DECLARES_NO_DATASETS = "check/sibling-declares-no-datasets";
    }
}

#[cfg(test)]
mod tests {
    use super::kind;
    use crate::diagnostics::SLOT_SEMANTICS_FAMILIES;

    fn is_kebab(text: &str) -> bool {
        !text.is_empty()
            && text.split('-').all(|word| {
                !word.is_empty()
                    && word
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
    }

    #[test]
    fn every_kind_is_one_family_and_one_case_in_kebab_case_and_none_repeats() {
        let mut seen = std::collections::BTreeSet::new();
        for kind in kind::ALL {
            let parts: Vec<&str> = kind.split('/').collect();
            assert!(
                parts.len() == 2 && parts.iter().all(|part| is_kebab(part)),
                "`{kind}` is `<family>/<case>`, each kebab-case"
            );
            assert!(seen.insert(kind), "`{kind}` is listed once");
            assert_eq!(kind::family(kind), parts[0], "`{kind}`'s family");
        }
    }

    #[test]
    fn a_claim_familys_kinds_carry_the_name_the_family_table_gives_it() {
        let checked: Vec<_> = SLOT_SEMANTICS_FAMILIES
            .iter()
            .filter_map(|family| Some((family, family.sibling_check.as_ref()?)))
            .collect();
        assert!(
            !checked.is_empty(),
            "some family is checked against siblings"
        );
        for (family, check) in checked {
            assert_eq!(
                check.unchecked_kind,
                format!("{}/unchecked", family.noun),
                "the `{}` family's unchecked kind is named after it",
                family.annotation
            );
            assert!(
                kind::ALL.contains(&check.unchecked_kind),
                "`{}` is a listed kind",
                check.unchecked_kind
            );
        }

        let nouns: Vec<&str> = SLOT_SEMANTICS_FAMILIES
            .iter()
            .map(|family| family.noun)
            .collect();
        for kind in kind::ALL {
            let family = kind.split('/').next().expect("a family");
            assert!(
                ["conformance", "reference", "dataset", "schema", "check"].contains(&family)
                    || nouns.contains(&family),
                "`{kind}` belongs to a check's family or to a claim family the table names"
            );
        }
    }
}
