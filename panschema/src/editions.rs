//! Book editions: one published book per schema release line.
//!
//! An edition is a minor line during 0.x and a major line after 1.0, built
//! from the newest tag in the line, so it spans exactly the releases a
//! consumer's pin moves within. The edge ref builds one more, unreleased,
//! edition. Editions read the release list `panschema publish` reads
//! (`[publishing]`), so the book and the schema docs agree on which
//! edition every version belongs to.
//!
//! Reference: [`docs/features/52-book-editions.md`](../../docs/features/52-book-editions.md)

use std::collections::BTreeMap;

use semver::Version;

use crate::publish::{PublishError, PublishingConfig};

/// One edition of the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edition {
    /// The directory the edition lands in under `book/`: `v0.1`, `v1`, or
    /// the edge label.
    pub label: String,
    /// The ref the edition builds from: the newest tag in its line, or the
    /// edge ref.
    pub source: String,
    /// The released versions the edition spans, oldest first. Empty for
    /// the unreleased edition.
    pub versions: Vec<String>,
}

impl Edition {
    /// Whether the edition is built from a release tag rather than the
    /// edge ref.
    pub fn is_released(&self) -> bool {
        !self.versions.is_empty()
    }
}

/// Every edition a release list produces, and which one is current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditionPlan {
    /// Released editions newest first, then the unreleased one.
    pub editions: Vec<Edition>,
    /// The label of the edition the site root serves.
    pub current: String,
}

/// Group `[publishing]`'s release list into editions.
///
/// Fails when a version does not parse as semver (its line cannot be
/// known), when the tags are spelled two ways or one is listed twice
/// (edition names would be inconsistent or ambiguous), or when the edge
/// label is also an edition's label.
pub fn plan(publishing: &PublishingConfig) -> Result<EditionPlan, PublishError> {
    // Edition labels reuse the tags' prefix, so every tag must share one.
    let mut spelling: Option<(&str, &str)> = None;
    // Keyed by line, so iteration runs oldest line first; each line's
    // versions are sorted by semver below.
    let mut lines: BTreeMap<Line, Vec<(Version, &str)>> = BTreeMap::new();
    for tag in &publishing.versions {
        let prefix = if tag.starts_with(['v', 'V']) {
            &tag[..1]
        } else {
            ""
        };
        let version = Version::parse(&tag[prefix.len()..]).map_err(|_| {
            PublishError::EditionVersionNotSemver {
                version: tag.clone(),
            }
        })?;
        match spelling {
            None => spelling = Some((prefix, tag)),
            Some((first_prefix, first)) if first_prefix != prefix => {
                return Err(PublishError::MixedTagSpelling {
                    first: first.to_string(),
                    second: tag.clone(),
                });
            }
            Some(_) => {}
        }
        let line = lines.entry(Line::of(&version)).or_default();
        if line.iter().any(|(seen, _)| *seen == version) {
            return Err(PublishError::DuplicateVersion {
                version: tag.clone(),
            });
        }
        line.push((version, tag));
    }
    let prefix = spelling.map_or("", |(prefix, _)| prefix);

    let mut editions: Vec<Edition> = lines
        .into_iter()
        .rev()
        .map(|(line, mut tags)| {
            tags.sort_by(|a, b| a.0.cmp(&b.0));
            // A pin never resolves to a pre-release, so a line builds from
            // its newest release; a line of only pre-releases has none.
            let (_, source) = tags
                .iter()
                .rev()
                .find(|(version, _)| version.pre.is_empty())
                .or(tags.last())
                .expect("a line holds at least one tag");
            Edition {
                label: format!("{prefix}{}", line.label()),
                source: source.to_string(),
                versions: tags.iter().map(|(_, tag)| tag.to_string()).collect(),
            }
        })
        .collect();

    if let Some(edge) = &publishing.edge {
        if editions.iter().any(|e| &e.label == edge) {
            return Err(PublishError::EdgeShadowsEdition { edge: edge.clone() });
        }
        editions.push(Edition {
            label: edge.clone(),
            source: edge.clone(),
            versions: Vec::new(),
        });
    }

    let current = editions
        .iter()
        .find(|e| e.versions.contains(&publishing.current) || e.source == publishing.current)
        .map(|e| e.label.clone())
        .ok_or_else(|| PublishError::InvalidCurrent {
            current: publishing.current.clone(),
            versions: publishing.versions.clone(),
            edge: publishing.edge.clone(),
        })?;

    Ok(EditionPlan { editions, current })
}

/// A release line: what a Cargo-style caret pin moves within. Each 0.0.z
/// release is its own line, then the minor line during 0.x, then the
/// major line after 1.0. Variant order is release order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Line {
    Patch(u64),
    Minor(u64),
    Major(u64),
}

impl Line {
    fn of(version: &Version) -> Self {
        match (version.major, version.minor) {
            (0, 0) => Line::Patch(version.patch),
            (0, minor) => Line::Minor(minor),
            (major, _) => Line::Major(major),
        }
    }

    fn label(self) -> String {
        match self {
            Line::Patch(patch) => format!("0.0.{patch}"),
            Line::Minor(minor) => format!("0.{minor}"),
            Line::Major(major) => major.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publishing(body: &str) -> PublishingConfig {
        toml::from_str(body).expect("test [publishing] body should parse")
    }

    fn labels_and_sources(plan: &EditionPlan) -> Vec<(&str, &str)> {
        plan.editions
            .iter()
            .map(|e| (e.label.as_str(), e.source.as_str()))
            .collect()
    }

    #[test]
    fn a_minor_line_is_one_edition_built_from_its_newest_patch() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.2.0"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.2", "v0.2.0"), ("v0.1", "v0.1.1")]
        );
        let v01 = plan.editions.iter().find(|e| e.label == "v0.1").unwrap();
        assert_eq!(v01.versions, vec!["v0.1.0", "v0.1.1"]);
    }

    #[test]
    fn after_one_point_oh_an_edition_is_a_major_line() {
        let plan = plan(&publishing(
            r#"versions = ["v1.0.0", "v1.2.3", "v2.0.0"]
               current = "v2.0.0""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v2", "v2.0.0"), ("v1", "v1.2.3")]
        );
    }

    /// Ordering by version, not by position, is the whole difference
    /// between building `v0.1` from its newest patch and from whichever
    /// tag the author happened to write last.
    #[test]
    fn the_newest_patch_is_chosen_by_semver_not_list_order() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.10", "v0.1.9", "v0.1.2"]
               current = "v0.1.10""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.1", "v0.1.10")]);
        assert_eq!(
            plan.editions[0].versions,
            vec!["v0.1.2", "v0.1.9", "v0.1.10"]
        );
    }

    #[test]
    fn a_release_beats_its_own_pre_release() {
        let plan = plan(&publishing(
            r#"versions = ["v0.2.0", "v0.2.0-rc.1"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.2", "v0.2.0")]);
    }

    /// A consumer's pin never resolves to a pre-release, so the line's
    /// book describes its newest release, not a candidate after it.
    #[test]
    fn a_line_builds_from_its_newest_release_not_a_later_pre_release() {
        let plan = plan(&publishing(
            r#"versions = ["v0.2.0", "v0.2.1-rc.1"]
               current = "v0.2.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.2", "v0.2.0")]);
    }

    #[test]
    fn a_line_of_only_pre_releases_builds_from_the_newest_one() {
        let plan = plan(&publishing(
            r#"versions = ["v0.3.0-rc.1", "v0.3.0-rc.2"]
               current = "v0.3.0-rc.2""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("v0.3", "v0.3.0-rc.2")]);
    }

    /// Cargo's caret treats every 0.0.z as its own incompatible line, so
    /// a pin on `0.0.1` never moves to `0.0.2`.
    #[test]
    fn each_zero_zero_release_is_its_own_edition() {
        let plan = plan(&publishing(
            r#"versions = ["v0.0.1", "v0.0.2"]
               current = "v0.0.2""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.0.2", "v0.0.2"), ("v0.0.1", "v0.0.1")]
        );
    }

    #[test]
    fn an_upper_case_v_prefix_is_a_prefix_too() {
        let plan = plan(&publishing(
            r#"versions = ["V1.0.0"]
               current = "V1.0.0""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("V1", "V1.0.0")]);
    }

    /// Edition directories are named from the tags, so two spellings would
    /// publish `book/v0.1/` beside `book/0.2/`.
    #[test]
    fn tags_spelled_two_ways_are_refused() {
        let err = plan(&publishing(
            r#"versions = ["0.1.0", "v0.1.1", "0.2.0"]
               current = "0.2.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::MixedTagSpelling { first, second } => {
                assert_eq!((first.as_str(), second.as_str()), ("0.1.0", "v0.1.1"))
            }
            other => panic!("expected MixedTagSpelling, got {other:?}"),
        }
    }

    #[test]
    fn a_version_listed_twice_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.1.0"]
               current = "v0.1.1""#,
        ))
        .unwrap_err();

        match err {
            PublishError::DuplicateVersion { version } => assert_eq!(version, "v0.1.0"),
            other => panic!("expected DuplicateVersion, got {other:?}"),
        }
    }

    #[test]
    fn tags_without_a_v_prefix_keep_that_spelling() {
        let plan = plan(&publishing(
            r#"versions = ["0.1.0", "0.1.1"]
               current = "0.1.1""#,
        ))
        .unwrap();

        assert_eq!(labels_and_sources(&plan), vec![("0.1", "0.1.1")]);
    }

    #[test]
    fn the_edge_is_an_unreleased_edition_after_the_released_ones() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "v0.1.1""#,
        ))
        .unwrap();

        assert_eq!(
            labels_and_sources(&plan),
            vec![("v0.1", "v0.1.1"), ("main", "main")]
        );
        let main = &plan.editions[1];
        assert!(
            !main.is_released(),
            "the edge edition must not count as released; got {main:?}"
        );
        assert!(
            plan.editions[0].is_released(),
            "a tagged edition is released; got {:?}",
            plan.editions[0]
        );
    }

    #[test]
    fn the_current_edition_is_the_line_holding_current() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.0", "v0.1.1", "v0.2.0"]
               edge = "main"
               current = "v0.1.0""#,
        ))
        .unwrap();

        assert_eq!(plan.current, "v0.1");
    }

    #[test]
    fn current_naming_the_edge_makes_the_unreleased_edition_current() {
        let plan = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "main""#,
        ))
        .unwrap();

        assert_eq!(plan.current, "main");
    }

    /// `plan` is public, so it can be handed a `[publishing]` that never
    /// went through the publish spec's own validation.
    #[test]
    fn a_current_in_no_edition_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "main"
               current = "v0.9.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::InvalidCurrent { current, .. } => assert_eq!(current, "v0.9.0"),
            other => panic!("expected InvalidCurrent, got {other:?}"),
        }
    }

    #[test]
    fn a_version_that_is_not_semver_is_refused_by_name() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.0", "release-2"]
               current = "v0.1.0""#,
        ))
        .unwrap_err();

        match err {
            PublishError::EditionVersionNotSemver { version } => assert_eq!(version, "release-2"),
            other => panic!("expected EditionVersionNotSemver, got {other:?}"),
        }
    }

    #[test]
    fn an_edge_label_that_is_also_an_edition_label_is_refused() {
        let err = plan(&publishing(
            r#"versions = ["v0.1.1"]
               edge = "v0.1"
               current = "v0.1.1""#,
        ))
        .unwrap_err();

        match err {
            PublishError::EdgeShadowsEdition { edge } => assert_eq!(edge, "v0.1"),
            other => panic!("expected EdgeShadowsEdition, got {other:?}"),
        }
    }
}
