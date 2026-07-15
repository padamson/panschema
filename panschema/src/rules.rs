//! Shared human-readable projection of a class `rule`'s conditions.
//!
//! One place builds the "when … then …" sentence from a [`ClassRule`]'s
//! pre/postconditions, so every writer that surfaces rules (the HTML card,
//! the graph hover payload) describes them identically — covering
//! `equals_string` / `equals_number`, `value_presence`, `required`, `range`,
//! `pattern`, value bounds, cardinality, and `any_of` alternatives.

use crate::linkml::{ClassRule, RuleConditions, SlotCondition, ValuePresence};

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
