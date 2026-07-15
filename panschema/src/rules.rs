//! Shared human-readable projection of a class `rule`'s conditions.
//!
//! One place builds the "when … then …" sentence from a [`ClassRule`]'s
//! pre/postconditions, so every writer that surfaces rules (the HTML card,
//! the graph hover payload) describes them identically — covering
//! `equals_string` / `equals_number`, `value_presence`, `required`, `range`,
//! `pattern`, value bounds, cardinality, and `any_of` alternatives.

use crate::linkml::{ClassRule, RuleConditions, SlotCondition, ValuePresence};

/// The slots a rule names, split by side: `trigger` slots appear in its
/// preconditions (what makes the rule fire), `governed` slots in its
/// postconditions (what the rule then constrains). Both are deduplicated and
/// sorted. Used to place graph glyphs on governed slots and to highlight a
/// rule's participants on hover.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuleParticipants {
    pub trigger: Vec<String>,
    pub governed: Vec<String>,
}

/// Collect the trigger and governed slot names a rule references, walking
/// both the direct `slot_conditions` and every `any_of` branch on each side.
pub fn rule_participants(rule: &ClassRule) -> RuleParticipants {
    RuleParticipants {
        trigger: rule
            .preconditions
            .as_ref()
            .map(condition_slots)
            .unwrap_or_default(),
        governed: rule
            .postconditions
            .as_ref()
            .map(condition_slots)
            .unwrap_or_default(),
    }
}

/// Every slot named in a condition set — its own `slot_conditions` keys plus
/// those of every `any_of` branch, recursively — deduplicated and sorted.
fn condition_slots(conditions: &RuleConditions) -> Vec<String> {
    let mut names: Vec<String> = conditions.slot_conditions.keys().cloned().collect();
    for alt in &conditions.any_of {
        names.extend(condition_slots(alt));
    }
    names.sort();
    names.dedup();
    names
}

/// Render a rule's pre/postconditions as one markdown "when … then …"
/// sentence, e.g. "when `status` = `actual`, then `region` is required", or
/// "when (`verdict` = `approved`) or (`verdict` = `rejected`), then
/// `approved_by` is present". `None` when the rule carries no renderable
/// condition on either side (a title/description-only entry).
pub fn rule_summary(rule: &ClassRule) -> Option<String> {
    let when = rule
        .preconditions
        .as_ref()
        .map(describe_conditions)
        .filter(|s| !s.is_empty());
    let then = rule
        .postconditions
        .as_ref()
        .map(describe_conditions)
        .filter(|s| !s.is_empty());

    match (when, then) {
        (Some(w), Some(t)) => Some(format!("when {}, then {}", w.join(", "), t.join(", "))),
        (Some(w), None) => Some(format!("when {}", w.join(", "))),
        (None, Some(t)) => Some(format!("then {}", t.join(", "))),
        (None, None) => None,
    }
}

/// Describe a whole condition set as markdown clauses: its `slot_conditions`
/// plus any `any_of` alternatives. Each `any_of` branch is parenthesized and
/// the branches are joined with "or", so a precondition that fires when
/// `verdict` is `approved` or `rejected` reads
/// "(`verdict` = `approved`) or (`verdict` = `rejected`)". A branch that
/// renders nothing is dropped rather than shown as an empty "()".
fn describe_conditions(conditions: &RuleConditions) -> Vec<String> {
    let mut clauses = describe_slot_conditions(&conditions.slot_conditions);
    let alts: Vec<String> = conditions
        .any_of
        .iter()
        .map(|alt| describe_conditions(alt).join(" and "))
        .filter(|s| !s.is_empty())
        .map(|s| format!("({s})"))
        .collect();
    if !alts.is_empty() {
        clauses.push(alts.join(" or "));
    }
    clauses
}

/// Render each slot's condition as a markdown clause, e.g. "`status` =
/// `actual`" or "`region` is required". Skips a slot whose condition sets
/// none of the fields panschema renders.
fn describe_slot_conditions(
    slot_conditions: &std::collections::BTreeMap<String, SlotCondition>,
) -> Vec<String> {
    slot_conditions
        .iter()
        .filter_map(|(slot, cond)| describe_slot_condition(slot, cond))
        .collect()
}

fn describe_slot_condition(slot: &str, cond: &SlotCondition) -> Option<String> {
    // `any_of` on the slot's value: describe each alternative for the same
    // slot and join with "or", e.g. "`verdict` = `approved` or `verdict` =
    // `rejected`". Alternatives that render nothing are dropped.
    if !cond.any_of.is_empty() {
        let alts: Vec<String> = cond
            .any_of
            .iter()
            .filter_map(|alt| describe_slot_condition(slot, alt))
            .collect();
        if !alts.is_empty() {
            return Some(alts.join(" or "));
        }
    }

    let mut clauses = Vec::new();
    if let Some(v) = &cond.equals_string {
        clauses.push(format!("= `{v}`"));
    }
    if let Some(v) = cond.equals_number {
        clauses.push(format!("= {v}"));
    }
    if let Some(vp) = cond.value_presence {
        clauses.push(
            match vp {
                ValuePresence::Present => "is present",
                ValuePresence::Absent => "is absent",
            }
            .to_string(),
        );
    }
    if cond.required {
        clauses.push("is required".to_string());
    }
    if let Some(r) = &cond.range {
        clauses.push(format!("is a `{r}`"));
    }
    if let Some(p) = &cond.pattern {
        clauses.push(format!("matches `{p}`"));
    }
    if let Some(min) = cond.minimum_value {
        clauses.push(format!(">= {min}"));
    }
    if let Some(max) = cond.maximum_value {
        clauses.push(format!("<= {max}"));
    }
    if let Some(min) = cond.minimum_cardinality {
        clauses.push(format!("has at least {min} value(s)"));
    }
    if let Some(max) = cond.maximum_cardinality {
        clauses.push(format!("has at most {max} value(s)"));
    }
    if clauses.is_empty() {
        return None;
    }
    Some(format!("`{slot}` {}", clauses.join(" and ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conds(slots: &[(&str, SlotCondition)], any_of: Vec<RuleConditions>) -> RuleConditions {
        RuleConditions {
            slot_conditions: slots
                .iter()
                .map(|(n, c)| (n.to_string(), c.clone()))
                .collect(),
            any_of,
        }
    }

    #[test]
    fn rule_participants_split_trigger_and_governed_including_any_of() {
        // ImageApproval shape: an `any_of` precondition over `verdict`, and
        // `value_presence` postconditions on `approved_by` / `approved_at`.
        // Trigger comes from the any_of branches; governed from the
        // postcondition slots — each deduplicated and sorted.
        let eq = |v: &str| SlotCondition {
            equals_string: Some(v.to_string()),
            ..Default::default()
        };
        let present = SlotCondition {
            value_presence: Some(ValuePresence::Present),
            ..Default::default()
        };
        let rule = ClassRule {
            title: None,
            description: None,
            preconditions: Some(conds(
                &[],
                vec![
                    conds(&[("verdict", eq("approved"))], Vec::new()),
                    conds(&[("verdict", eq("rejected"))], Vec::new()),
                ],
            )),
            postconditions: Some(conds(
                &[("approved_by", present.clone()), ("approved_at", present)],
                Vec::new(),
            )),
        };

        let p = rule_participants(&rule);
        assert_eq!(p.trigger, vec!["verdict"], "trigger from any_of branches");
        assert_eq!(
            p.governed,
            vec!["approved_at", "approved_by"],
            "governed slots, sorted + deduped"
        );
    }

    #[test]
    fn slot_level_any_of_renders_as_alternatives_in_the_trigger() {
        // The real cuisineiq `ImageApproval` shape: the `verdict` slot
        // condition is an `any_of` over equals_string values. It must render
        // as a "when" trigger, not vanish (which left "then … is present"
        // with no trigger).
        let branch = |v: &str| SlotCondition {
            equals_string: Some(v.to_string()),
            ..Default::default()
        };
        let verdict = SlotCondition {
            any_of: vec![branch("approved"), branch("rejected")],
            ..Default::default()
        };
        let present = SlotCondition {
            value_presence: Some(ValuePresence::Present),
            ..Default::default()
        };
        let rule = ClassRule {
            title: None,
            description: None,
            preconditions: Some(conds(&[("verdict", verdict)], Vec::new())),
            postconditions: Some(conds(&[("approved_by", present)], Vec::new())),
        };

        // Lock the exact human-readable formatting, not just fragments.
        let s = rule_summary(&rule).expect("a slot-level any_of rule must render a summary");
        assert_eq!(
            s,
            "when `verdict` = `approved` or `verdict` = `rejected`, \
             then `approved_by` is present"
        );
    }

    #[test]
    fn rule_participants_are_empty_for_a_conditionless_rule() {
        let rule = ClassRule {
            title: Some("t".into()),
            description: None,
            preconditions: None,
            postconditions: None,
        };
        assert_eq!(rule_participants(&rule), RuleParticipants::default());
    }
}
