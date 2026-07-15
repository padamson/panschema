//! HTML Writer
//!
//! Writes LinkML SchemaDefinition to HTML documentation.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use askama::Template;

use crate::graph_writer::GraphWriter;
use crate::io::{IoError, IoResult, Writer};
use crate::linkml::{Example, SchemaDefinition};

/// Entity reference for sidebar navigation and cross-references.
#[derive(Debug, Clone)]
pub struct EntityRef {
    pub id: String,
    pub label: String,
}

impl EntityRef {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Namespace prefix/IRI mapping.
#[derive(Debug, Clone)]
pub struct Namespace {
    pub prefix: String,
    pub iri: String,
}

/// Full class data for rendering class cards.
#[derive(Debug, Clone)]
pub struct ClassData {
    pub id: String,
    pub label: String,
    pub iri: String,
    /// Expanded link target paired with `iri`; `None` falls back to
    /// plain text in the template.
    pub iri_href: Option<String>,
    pub description: Option<String>,
    pub superclass: Option<EntityRef>,
    pub subclasses: Vec<EntityRef>,
    pub mixins: Vec<EntityRef>,
    pub slots: Vec<SlotInClass>,
    pub mappings: Vec<Mapping>,
    /// External `rdfs:subClassOf` grounding — typically upstream
    /// ontology classes the schema declares this class as a
    /// subclass of. Distinct from `superclass`, which models the
    /// intra-schema `is_a` parent.
    pub external_superclasses: Vec<ExternalLink>,
    /// `true` for LinkML classes with `abstract: true`. Surfaced as
    /// a small badge in the card heading so readers can tell
    /// foundation classes from instantiable ones at a glance.
    pub is_abstract: bool,
    /// Deprecation note when the class is marked `deprecated:`. Drives a
    /// "Deprecated" badge in the heading plus the note text on the card;
    /// `None` renders nothing.
    pub deprecated: Option<String>,
    /// Alternative names from `aliases:`. Rendered as a comma-joined
    /// "Aliases" row; empty renders nothing.
    pub aliases: Vec<String>,
    /// Related-resource references from `see_also:`, CURIE-expanded into
    /// links. Rendered as a "See also" row; empty renders nothing.
    pub see_also: Vec<ExternalLink>,
    /// Worked examples from `examples:`. Rendered as an "Examples"
    /// section listing each value with its optional description; empty
    /// renders nothing.
    pub examples: Vec<Example>,
    /// Conditional constraints from `rules:`. Rendered as a "Rules"
    /// section; empty renders nothing.
    pub rules: Vec<RuleInClass>,
    /// Uniqueness constraints from `unique_keys:`. Rendered as a "Unique
    /// keys" row; empty renders nothing.
    pub unique_keys: Vec<UniqueKeyInClass>,
}

/// A `rules` entry as rendered on a class card.
#[derive(Debug, Clone)]
pub struct RuleInClass {
    pub title: Option<String>,
    /// Markdown-rendered, like [`ClassData::description`].
    pub description: Option<String>,
    /// Markdown-rendered "when … then …" sentence built from the rule's
    /// pre/postconditions (e.g. "when `status` = `actual`, then `region`
    /// is required"). `None` when the rule has neither — a
    /// title/description-only entry.
    pub summary: Option<String>,
}

/// A `unique_keys` entry as rendered on a class card.
#[derive(Debug, Clone)]
pub struct UniqueKeyInClass {
    /// The key's name (the `unique_keys` map key).
    pub name: String,
    /// The slot tuple whose combined values must be unique.
    pub slots: Vec<String>,
    /// Markdown-rendered description, when the key declares one.
    pub description: Option<String>,
}

/// One pre-order entry in the Classes hierarchy view. The template
/// renders the flattened sequence as semantically nested `<ul>`/`<li>`
/// markup: `has_children` opens a child list after the card, and
/// `closes` says how many ancestor levels this entry is the last
/// descendant of (each closed with a `</ul></li>` pair).
#[derive(Debug, Clone)]
pub struct ClassTreeEntry {
    /// Index into the alphabetical `class_data` list — the card to
    /// render at this position. Doubling as the class's alphabetical
    /// rank, it is also the CSS `order` value the flat view sorts
    /// cards by after dissolving the tree with `display: contents`.
    pub index: usize,
    pub depth: usize,
    pub has_children: bool,
    pub closes: usize,
}

impl ClassTreeEntry {
    /// Closing tags for the ancestor levels this leaf terminates.
    /// Empty for entries with children — their `<ul>` is closed by
    /// their own last descendant.
    pub fn close_tags(&self) -> String {
        "</ul></li>".repeat(self.closes)
    }
}

/// Arrange the alphabetical class list into a pre-order `is_a`
/// forest: roots are classes with no resolvable parent, children
/// nest under their parent in alphabetical order. Fail-open on
/// pathological shapes — an `is_a` cycle leaves its members
/// unreachable from any root, so a sweep pass renders them as
/// roots rather than dropping them.
fn build_class_tree(class_data: &[ClassData]) -> Vec<ClassTreeEntry> {
    let index_by_id: HashMap<&str, usize> = class_data
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.as_str(), i))
        .collect();

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); class_data.len()];
    let mut is_child = vec![false; class_data.len()];
    for (i, class) in class_data.iter().enumerate() {
        if let Some(parent) = &class.superclass
            && let Some(&p) = index_by_id.get(parent.id.as_str())
            && p != i
        {
            children[p].push(i);
            is_child[i] = true;
        }
    }

    let mut entries = Vec::new();
    let mut visited = vec![false; class_data.len()];
    let walk_root = |root: usize, entries: &mut Vec<ClassTreeEntry>, visited: &mut Vec<bool>| {
        let mut stack = vec![(root, 0usize)];
        while let Some((idx, depth)) = stack.pop() {
            if visited[idx] {
                continue;
            }
            visited[idx] = true;
            let kids: Vec<usize> = children[idx]
                .iter()
                .copied()
                .filter(|&k| !visited[k])
                .collect();
            entries.push(ClassTreeEntry {
                index: idx,
                depth,
                has_children: !kids.is_empty(),
                closes: 0,
            });
            for &kid in kids.iter().rev() {
                stack.push((kid, depth + 1));
            }
        }
    };
    for (root, &child) in is_child.iter().enumerate() {
        if !child {
            walk_root(root, &mut entries, &mut visited);
        }
    }
    // Cycle members are nobody's root and nobody reached them; render
    // them as roots so no class silently disappears from the docs.
    let unreached: Vec<usize> = (0..class_data.len()).filter(|&i| !visited[i]).collect();
    for idx in unreached {
        walk_root(idx, &mut entries, &mut visited);
    }

    // A leaf closes every ancestor level it is the last descendant
    // of: the difference between its depth and the next entry's.
    let depths: Vec<usize> = entries.iter().map(|e| e.depth).collect();
    for (i, entry) in entries.iter_mut().enumerate() {
        if entry.has_children {
            continue;
        }
        let next_depth = depths.get(i + 1).copied().unwrap_or(0);
        entry.closes = entry.depth.saturating_sub(next_depth);
    }
    entries
}

/// A slot as it appears on a specific class, with framing resolved for
/// rendering.
#[derive(Debug, Clone)]
pub struct SlotInClass {
    pub name: String,
    pub range: Option<RangeRef>,
    pub required: bool,
    pub multivalued: bool,
    /// Members of an `any_of` union; empty for single-range slots.
    pub any_of: Vec<RangeRef>,
    /// `true` when this class suppresses the slot via
    /// `maximum_cardinality: 0` — it declares the slot but permits no
    /// value. The card shows "has no value" instead of a range.
    pub suppressed: bool,
    pub description: Option<String>,
    /// `true` when this class's `slot_usage` overrides an inherited slot.
    pub refined_here: bool,
    /// Display label for where an inherited slot came from
    /// (e.g. `"mixin Named"`); `None` for the class's own slots.
    pub origin: Option<String>,
    /// Plain-text description shown as a hover tooltip on inherited
    /// slots. Inherited entries render compactly — the inline
    /// description belongs to the defining class's card — so
    /// `description` and `description_tooltip` are mutually
    /// exclusive.
    pub description_tooltip: Option<String>,
}

/// Range reference for property cards - either a class link or a datatype name.
#[derive(Debug, Clone)]
pub struct RangeRef {
    pub class_ref: Option<EntityRef>,
    pub datatype: String,
}

/// A single permissible value rendered on an enum card.
#[derive(Debug, Clone)]
pub struct PermissibleValueData {
    pub text: String,
    pub description: Option<String>,
    /// The value's `meaning` — a concept IRI grounding it in an
    /// upstream vocabulary — as a hyperlink with its cached label, or
    /// `None` when the value declares no meaning.
    pub meaning: Option<ExternalLink>,
}

/// Enumeration data for rendering an enum card.
#[derive(Debug, Clone)]
pub struct EnumData {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub permissible_values: Vec<PermissibleValueData>,
    /// Deprecation note; see [`ClassData::deprecated`].
    pub deprecated: Option<String>,
    /// Alternative names; see [`ClassData::aliases`].
    pub aliases: Vec<String>,
    /// Related-resource links; see [`ClassData::see_also`].
    pub see_also: Vec<ExternalLink>,
    /// Worked examples; see [`ClassData::examples`].
    pub examples: Vec<Example>,
}

/// Type data for rendering a type card.
#[derive(Debug, Clone)]
pub struct TypeData {
    pub id: String,
    pub label: String,
    /// The type's `uri` as a hyperlink, when declared.
    pub uri: Option<ExternalLink>,
    pub description: Option<String>,
    /// The parent type (`typeof`) this derives from — a link to its
    /// own `#type-` card when that parent is declared in the schema,
    /// else plain text.
    pub base_type: Option<EntityRef>,
    pub pattern: Option<String>,
    /// Deprecation note; see [`ClassData::deprecated`].
    pub deprecated: Option<String>,
    /// Alternative names; see [`ClassData::aliases`].
    pub aliases: Vec<String>,
    /// Related-resource links; see [`ClassData::see_also`].
    pub see_also: Vec<ExternalLink>,
    /// Worked examples; see [`ClassData::examples`].
    pub examples: Vec<Example>,
}

/// A cross-ontology mapping rendered on class / property cards.
/// `kind` is one of "exact" / "close" / "related" / "narrow" /
/// "broad" — hence the `&'static str`. `href` is `None` for values
/// whose prefix isn't declared, signalling fallback rendering.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub kind: &'static str,
    pub display: String,
    pub href: Option<String>,
    /// Upstream `rdfs:label` for the expanded IRI, when cached.
    pub label: Option<String>,
    /// Every upstream definitional annotation for the expanded IRI
    /// (definition / description / comment / example), when cached.
    pub definitions: Vec<String>,
}

impl Mapping {
    /// Tooltip text: CURIE = IRI identity line, plus each upstream
    /// definitional annotation on its own paragraph when cached.
    /// Browsers render literal newlines in `title` attributes.
    pub fn tooltip(&self) -> String {
        tooltip_text(&self.display, self.href.as_deref(), &self.definitions)
    }
}

/// External hyperlink with an optional expansion. `display` is the
/// CURIE or IRI the author wrote; `href` is the expanded link
/// target, or `None` when the prefix isn't declared.
#[derive(Debug, Clone)]
pub struct ExternalLink {
    pub display: String,
    pub href: Option<String>,
    /// Upstream `rdfs:label` for the expanded IRI, when cached.
    pub label: Option<String>,
    /// Every upstream definitional annotation for the expanded IRI
    /// (definition / description / comment / example), when cached.
    pub definitions: Vec<String>,
}

impl ExternalLink {
    /// See [`Mapping::tooltip`].
    pub fn tooltip(&self) -> String {
        tooltip_text(&self.display, self.href.as_deref(), &self.definitions)
    }
}

/// Tooltip: the `CURIE = IRI` identity line, then each upstream
/// definitional annotation as its own paragraph (a term may carry a
/// definition, a description, a comment, and an example — all are
/// shown for maximum grounding context).
fn tooltip_text(display: &str, href: Option<&str>, definitions: &[String]) -> String {
    let identity = match href {
        Some(href) => format!("{display} = {href}"),
        None => display.to_string(),
    };
    if definitions.is_empty() {
        identity
    } else {
        format!("{identity}\n\n{}", definitions.join("\n\n"))
    }
}

/// Full property data for rendering property cards.
#[derive(Debug, Clone)]
pub struct SlotData {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub iri_href: Option<String>,
    pub slot_type: String,
    pub description: Option<String>,
    /// Every class this slot is a domain of (a slot can belong to
    /// several classes). Rendered as the Domain row.
    pub domains: Vec<EntityRef>,
    pub range: Option<RangeRef>,
    /// Members of an `any_of` union range; empty for single-range slots.
    /// Rendered as the Range row when `range` itself is absent (the
    /// common `any_of` case), so a polymorphic range isn't dropped.
    pub any_of: Vec<RangeRef>,
    /// Validation `pattern` (regex), if any — rendered truncated with the
    /// full value on a tooltip.
    pub pattern: Option<String>,
    pub characteristics: Vec<String>,
    pub mappings: Vec<Mapping>,
    /// Deprecation note when the slot is marked `deprecated:`. The
    /// "Deprecated" badge rides the `characteristics` list; this carries
    /// the note text rendered alongside it. `None` renders nothing.
    pub deprecated: Option<String>,
    /// Alternative names; see [`ClassData::aliases`].
    pub aliases: Vec<String>,
    /// Related-resource links; see [`ClassData::see_also`].
    pub see_also: Vec<ExternalLink>,
    /// Worked examples; see [`ClassData::examples`].
    pub examples: Vec<Example>,
    /// The slot's `ifabsent` default, rendered readably for the Default
    /// row (`planned`, `8080`, `"svc"`, `true`). `None` renders no row.
    pub default: Option<String>,
}

/// A resolved property value for rendering individual cards.
#[derive(Debug, Clone)]
pub struct PropertyValueData {
    pub property_label: String,
    pub property_ref: Option<EntityRef>,
    pub value: String,
}

/// Full individual data for rendering individual cards.
#[derive(Debug, Clone)]
pub struct IndividualData {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub description: Option<String>,
    pub types: Vec<EntityRef>,
    pub property_values: Vec<PropertyValueData>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    title: &'a str,
    iri: &'a str,
    version: Option<&'a str>,
    comment: Option<&'a str>,
    active_section: &'a str,
    classes: &'a [EntityRef],
    class_data: &'a [ClassData],
    class_tree: &'a [ClassTreeEntry],
    slots: &'a [EntityRef],
    slot_data: &'a [SlotData],
    enums: &'a [EntityRef],
    enum_data: &'a [EnumData],
    types: &'a [EntityRef],
    type_data: &'a [TypeData],
    individuals: &'a [EntityRef],
    individual_data: &'a [IndividualData],
    namespaces: &'a [Namespace],
    /// Empty slice for class cards that don't have slots yet
    /// Graph data JSON for visualization (None = no graph)
    graph_json: Option<&'a str>,
    /// Number of nodes in the graph (for sidebar badge)
    graph_node_count: usize,
    /// Number of edges in the graph (for sidebar badge)
    graph_edge_count: usize,
    /// Graph viz aspect ratio components, rendered into the
    /// `.graph-container` CSS rule.
    graph_aspect_w: u32,
    graph_aspect_h: u32,
    /// Layout-algorithm identifier rendered into the
    /// `--graph-layout` CSS custom property. The JS picker reads this
    /// to set its initial selection.
    graph_default_layout: &'a str,
    /// Multi-version cohort context. When `Some`, the header gains a
    /// version dropdown and the body may show a stale/edge banner.
    /// Always `None` for the `panschema generate` path.
    version_context: Option<&'a VersionContext>,
    /// URL the header brand link targets. `"./"` for single-version
    /// output (page sits at the deploy root). `panschema publish`
    /// supplies this explicitly from the manifest's
    /// `[publishing].site_root_url` (default `"../current/"`).
    site_root_href: &'a str,
}

/// Per-page context describing the multi-version cohort this page is
/// part of. Drives the version-dropdown control in the header and the
/// "you're viewing X; current is Y" / "edge build" banners. Absent
/// (`None`) when the schema is rendered as a single-version output by
/// `panschema generate`; present (`Some(_)`) when rendered by
/// `panschema publish` for a versioned site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionContext {
    /// Ordered list of versions to show in the dropdown. Conventionally
    /// edge first (if present), then released versions newest to oldest.
    pub all_versions: Vec<String>,
    /// The version this specific page is rendered for.
    pub viewing: String,
    /// The version `current/` aliases. Used to decide whether to show
    /// the "you're viewing X; current is Y" banner.
    pub current: String,
    /// Edge ref name (e.g. `"main"`), if the cohort includes an edge
    /// build. Pages rendered for this ref get the "edge build from HEAD"
    /// banner.
    pub edge: Option<String>,
    /// URL template with a literal `{version}` placeholder. The dropdown
    /// JS substitutes each option's value to form cross-version links.
    pub url_pattern: String,
}

impl VersionContext {
    /// Substitute `{version}` in `url_pattern` to produce a navigation
    /// URL for the given version.
    pub fn url_for(&self, version: &str) -> String {
        self.url_pattern.replace("{version}", version)
    }

    /// `true` if `version` is the cohort's `edge` ref. Templates use
    /// this to badge the edge entry in the dropdown.
    pub fn is_edge(&self, version: &str) -> bool {
        self.edge.as_deref() == Some(version)
    }

    /// `true` when the page being rendered is the cohort's `current`
    /// version (so the stale-banner can be suppressed).
    pub fn viewing_is_current(&self) -> bool {
        self.viewing == self.current
    }

    /// `true` when the page being rendered is the cohort's edge build
    /// (so the edge-banner can be shown).
    pub fn viewing_is_edge(&self) -> bool {
        self.edge.as_deref() == Some(self.viewing.as_str())
    }
}

/// Writer for HTML documentation output
pub struct HtmlWriter {
    /// Whether to include graph visualization (default: true)
    pub include_graph: bool,
    /// Schema graph viz aspect ratio as `(width, height)`. Default 16:8
    /// — fits a typical laptop screen alongside browser chrome and an
    /// OS task bar. Consumers can override per-schema via the manifest's
    /// `html_graph_aspect = "W:H"` field.
    pub graph_aspect: (u32, u32),
    /// Layout-algorithm identifier (e.g. `"sgd"` / `"force-directed"`)
    /// for the initial value of the graph-viz layout picker. Consumers
    /// pin one per-schema via the manifest's `html_default_layout`
    /// field. Defaults to `"auto"` — the not-pinned sentinel: the viz
    /// picks a default from the graph's inheritance density at render
    /// time (Hierarchical for `is_a`-heavy schemas, SGD otherwise). The
    /// JS picker falls back to force-directed in 3D mode since SGD and
    /// the static layouts are 2D-only.
    pub graph_default_layout: String,
    /// Optional multi-version cohort context. Set by `panschema publish`;
    /// `None` for the single-version `panschema generate` path. When
    /// present, the rendered page gains a version dropdown in the header
    /// and a banner when `viewing` differs from `current` or matches `edge`.
    pub version_context: Option<VersionContext>,
    /// Override for the header brand link target. `None` means use the
    /// per-flow default: `"./"` for single-version output (page sits at
    /// the deploy root) — `panschema publish` always sets this explicitly
    /// from the manifest's `site_root_url`.
    pub site_root_href: Option<String>,
    /// Upstream label cache. `None` renders external references as
    /// CURIEs (the historical behavior); the CLI generate path wires
    /// a populated store so they render as upstream labels.
    pub label_store: Option<crate::labels::LabelStore>,
}

/// Parse a `"W:H"` aspect-ratio string. Both components must be positive
/// integers and at most 9999 (a sanity cap; nothing useful needs more
/// digits and bigger values would suggest a typo such as a wall-clock
/// time slipped into the field).
pub fn parse_graph_aspect(s: &str) -> Result<(u32, u32), String> {
    let (w_str, h_str) = s
        .split_once(':')
        .ok_or_else(|| format!("aspect ratio `{s}` must be `W:H` (e.g. `16:9`)"))?;
    let w: u32 = w_str
        .trim()
        .parse()
        .map_err(|_| format!("aspect ratio width `{w_str}` is not a non-negative integer"))?;
    let h: u32 = h_str
        .trim()
        .parse()
        .map_err(|_| format!("aspect ratio height `{h_str}` is not a non-negative integer"))?;
    if w == 0 || h == 0 {
        return Err(format!("aspect ratio `{s}` must have non-zero components"));
    }
    if w > 9999 || h > 9999 {
        return Err(format!("aspect ratio `{s}` components must be <= 9999"));
    }
    Ok((w, h))
}

/// Embedded WASM visualization files (from panschema-viz build)
mod wasm_files {
    /// JavaScript bindings for WASM visualization
    pub const VIZ_JS: &str = include_str!("../../panschema-viz/pkg/panschema_viz.js");

    /// Compiled WASM binary
    pub const VIZ_WASM: &[u8] = include_bytes!("../../panschema-viz/pkg/panschema_viz_bg.wasm");
}

impl HtmlWriter {
    /// Create a new HTML writer with default options (graph enabled,
    /// 16:8 graph aspect ratio, SGD default layout — see
    /// [`Self::graph_default_layout`] for the choice).
    pub fn new() -> Self {
        Self {
            include_graph: true,
            graph_aspect: (16, 8),
            graph_default_layout: "auto".to_string(),
            version_context: None,
            site_root_href: None,
            label_store: None,
        }
    }

    /// Create a new HTML writer with custom options
    pub fn with_options(include_graph: bool) -> Self {
        Self {
            include_graph,
            graph_aspect: (16, 8),
            graph_default_layout: "auto".to_string(),
            version_context: None,
            site_root_href: None,
            label_store: None,
        }
    }

    /// Attach a populated upstream-label cache so external CURIEs
    /// render as human-readable labels.
    #[must_use]
    pub fn with_label_store(mut self, store: crate::labels::LabelStore) -> Self {
        self.label_store = Some(store);
        self
    }

    /// Attach a multi-version cohort context. Used by `panschema publish`
    /// to inject the dropdown + banner UX into each per-version page.
    #[must_use]
    pub fn with_version_context(mut self, ctx: VersionContext) -> Self {
        self.version_context = Some(ctx);
        self
    }

    /// Override the header brand-link target. Consumed by
    /// `panschema publish` to forward the manifest's `site_root_url`
    /// into each per-version page.
    #[must_use]
    pub fn with_site_root_href(mut self, href: impl Into<String>) -> Self {
        self.site_root_href = Some(href.into());
        self
    }

    /// Override the schema graph viz aspect ratio. The writer accepts
    /// any pair of positive `u32`s; pre-validate strings via
    /// [`parse_graph_aspect`].
    #[must_use]
    pub fn with_graph_aspect(mut self, w: u32, h: u32) -> Self {
        self.graph_aspect = (w, h);
        self
    }

    /// Override the default layout algorithm for the graph picker.
    /// Pre-validate via [`crate::manifest::validate_layout_name`] —
    /// this method does not re-check, on the assumption the value
    /// already passed manifest parsing.
    #[must_use]
    pub fn with_default_layout(mut self, name: impl Into<String>) -> Self {
        self.graph_default_layout = name.into();
        self
    }

    /// Test convenience: build template data without a label store
    /// (external references render as CURIEs).
    #[cfg(test)]
    fn build_template_data(schema: &SchemaDefinition) -> TemplateData {
        Self::build_template_data_with_labels(schema, None)
    }

    /// Build template data, rendering upstream labels for external
    /// references when a populated [`crate::labels::LabelStore`] is
    /// supplied.
    fn build_template_data_with_labels(
        schema: &SchemaDefinition,
        labels: Option<&crate::labels::LabelStore>,
    ) -> TemplateData {
        let iri = schema.id.clone().unwrap_or_else(|| schema.name.clone());
        let title = schema.title.clone().unwrap_or_else(|| schema.name.clone());

        let namespaces = build_namespaces(schema, &iri);

        // Build class data
        let mut class_refs = Vec::new();
        let mut class_data_list = Vec::new();

        // Sort classes by name for consistent ordering
        let mut sorted_classes: Vec<_> = schema.classes.iter().collect();
        sorted_classes.sort_by(|a, b| {
            let label_a = a.1.annotations.get("panschema:label").unwrap_or(a.0);
            let label_b = b.1.annotations.get("panschema:label").unwrap_or(b.0);
            label_a.cmp(label_b)
        });

        for (class_id, class_def) in &sorted_classes {
            let label = class_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| (*class_id).clone());

            class_refs.push(EntityRef {
                id: (*class_id).clone(),
                label: label.clone(),
            });

            // Find superclass
            let superclass = class_def.is_a.as_ref().and_then(|parent_id| {
                schema.classes.get(parent_id).map(|parent| {
                    let parent_label = parent
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| parent_id.clone());
                    EntityRef {
                        id: parent_id.clone(),
                        label: parent_label,
                    }
                })
            });

            // Find subclasses
            let subclasses: Vec<EntityRef> = schema
                .classes
                .iter()
                .filter(|(_, c)| c.is_a.as_ref() == Some(class_id))
                .map(|(sub_id, sub_def)| {
                    let sub_label = sub_def
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| sub_id.clone());
                    EntityRef {
                        id: sub_id.clone(),
                        label: sub_label,
                    }
                })
                .collect();

            // Unresolved mixins (from un-loaded imports or typos) are
            // skipped: a broken `#class-X` anchor is worse than omission.
            let mixins: Vec<EntityRef> = class_def
                .mixins
                .iter()
                .filter_map(|mixin_id| {
                    schema.classes.get(mixin_id).map(|mixin_def| {
                        let mixin_label = mixin_def
                            .annotations
                            .get("panschema:label")
                            .cloned()
                            .unwrap_or_else(|| mixin_id.clone());
                        EntityRef {
                            id: mixin_id.clone(),
                            label: mixin_label,
                        }
                    })
                })
                .collect();

            let resolved =
                crate::linkml_resolve::resolve_effective_slots_with_provenance(class_def, schema);
            let slots: Vec<SlotInClass> = resolved
                .iter()
                .map(|(slot_name, rs)| {
                    let slot_def = &rs.definition;
                    let cardinality = crate::linkml_resolve::effective_cardinality(slot_def);
                    let origin = rs.provenance.origin_label(class_id);
                    // Inline description only where the slot is
                    // defined or refined; inherited entries carry it
                    // as a tooltip to keep subclass cards compact.
                    let (description, description_tooltip) = if origin.is_some() {
                        (None, slot_def.description.clone())
                    } else {
                        (
                            slot_def
                                .description
                                .as_deref()
                                .map(|d| render_description(d, schema)),
                            None,
                        )
                    };
                    // Render the induced per-class range (slot_usage
                    // applied), not the raw inherited definition: a
                    // single induced range fills `range`, several fill
                    // `any_of`, and a suppressed slot shows neither.
                    let induced = &rs.induced;
                    let (range, any_of) = if induced.ranges.len() == 1 {
                        (Some(range_ref_for(&induced.ranges[0], schema)), Vec::new())
                    } else {
                        (
                            None,
                            induced
                                .ranges
                                .iter()
                                .map(|r| range_ref_for(r, schema))
                                .collect(),
                        )
                    };
                    SlotInClass {
                        name: slot_name.clone(),
                        range,
                        required: cardinality.required,
                        multivalued: cardinality.multivalued,
                        any_of,
                        suppressed: induced.suppressed,
                        description,
                        refined_here: class_def.slot_usage.contains_key(slot_name),
                        origin,
                        description_tooltip,
                    }
                })
                .collect();

            let mappings = build_mappings(
                &class_def.exact_mappings,
                &class_def.close_mappings,
                &class_def.related_mappings,
                &class_def.narrow_mappings,
                &class_def.broad_mappings,
                schema,
                labels,
            );

            // class_uri wins when present; otherwise treat the
            // class name as a bare CURIE so the schema's
            // default_prefix resolves it (the LinkML convention).
            let iri_href = class_def
                .class_uri
                .as_deref()
                .and_then(|c| crate::linkml_resolve::expand_curie(schema, c))
                .or_else(|| crate::linkml_resolve::expand_curie(schema, class_id));

            let external_superclasses: Vec<ExternalLink> = class_def
                .subclass_of
                .as_deref()
                .map(|raw| {
                    let href = crate::linkml_resolve::expand_curie(schema, raw);
                    let (label, definitions) = lookup_term(labels, href.as_deref());
                    ExternalLink {
                        display: raw.to_string(),
                        href,
                        label,
                        definitions,
                    }
                })
                .into_iter()
                .collect();

            class_data_list.push(ClassData {
                id: (*class_id).clone(),
                label,
                iri: class_def
                    .class_uri
                    .clone()
                    .unwrap_or_else(|| (*class_id).clone()),
                iri_href,
                description: class_def
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
                superclass,
                subclasses,
                mixins,
                slots,
                mappings,
                external_superclasses,
                is_abstract: class_def.r#abstract,
                deprecated: class_def.deprecated.clone(),
                aliases: class_def.aliases.clone(),
                see_also: build_see_also(&class_def.see_also, schema, labels),
                examples: class_def.examples.clone(),
                rules: build_rules(&class_def.rules, schema),
                unique_keys: build_unique_keys(&class_def.unique_keys, schema),
            });
        }

        // Build property (slot) data
        let mut slot_refs = Vec::new();
        let mut slot_data_list = Vec::new();

        // Sort slots by label for consistent ordering
        let mut sorted_slots: Vec<_> = schema.slots.iter().collect();
        sorted_slots.sort_by(|a, b| {
            let label_a = a.1.annotations.get("panschema:label").unwrap_or(a.0);
            let label_b = b.1.annotations.get("panschema:label").unwrap_or(b.0);
            label_a.cmp(label_b)
        });

        for (slot_id, slot_def) in &sorted_slots {
            let label = slot_def
                .annotations
                .get("panschema:label")
                .cloned()
                .unwrap_or_else(|| (*slot_id).clone());

            slot_refs.push(EntityRef {
                id: (*slot_id).clone(),
                label: label.clone(),
            });

            // Every relation renders under the single LinkML term. The
            // object-vs-datatype distinction lives in the card's Range row
            // (a class link vs a datatype name), so the badge stays "Slot".
            let slot_type = "Slot".to_string();

            // Resolve every effective domain class to an EntityRef — the
            // slot's own `domain:` or all classes that list it in
            // `slots:` — so the card names every owning class, matching
            // the graph hover.
            let domains: Vec<EntityRef> =
                crate::linkml_resolve::resolve_slot_domains(schema, slot_id, slot_def)
                    .into_iter()
                    .filter_map(|domain_id| {
                        schema.classes.get(&domain_id).map(|c| {
                            let domain_label = c
                                .annotations
                                .get("panschema:label")
                                .cloned()
                                .unwrap_or_else(|| domain_id.clone());
                            EntityRef {
                                id: domain_id.clone(),
                                label: domain_label,
                            }
                        })
                    })
                    .collect();

            // Resolve range
            let range = slot_def.range.as_ref().map(|range_id| {
                let class_ref = schema.classes.get(range_id).map(|c| {
                    let range_label = c
                        .annotations
                        .get("panschema:label")
                        .cloned()
                        .unwrap_or_else(|| range_id.clone());
                    EntityRef {
                        id: range_id.clone(),
                        label: range_label,
                    }
                });

                RangeRef {
                    class_ref,
                    datatype: range_id.clone(),
                }
            });

            // Members of an `any_of` union range, resolved to refs (each
            // member's own range, or the slot's range as a fallback).
            let any_of: Vec<RangeRef> = slot_def
                .any_of
                .iter()
                .filter_map(|branch| {
                    branch
                        .range
                        .as_deref()
                        .or(slot_def.range.as_deref())
                        .map(|r| range_ref_for(r, schema))
                })
                .collect();

            // Build characteristics. Surface effective cardinality
            // (required / multivalued / explicit bounds), identifier, and
            // inverse — the same slot facts the graph hover shows.
            let cardinality = crate::linkml_resolve::effective_cardinality(slot_def);
            let mut characteristics = Vec::new();
            if cardinality.required {
                characteristics.push("Required".to_string());
            }
            if cardinality.multivalued {
                characteristics.push("Multivalued".to_string());
            }
            if slot_def.identifier {
                characteristics.push("Identifier".to_string());
            }
            // OWL relationship characteristics, surfaced as badges.
            for (set, label) in [
                (slot_def.symmetric, "Symmetric"),
                (slot_def.asymmetric, "Asymmetric"),
                (slot_def.reflexive, "Reflexive"),
                (slot_def.irreflexive, "Irreflexive"),
                (slot_def.transitive, "Transitive"),
            ] {
                if set {
                    characteristics.push(label.to_string());
                }
            }
            if slot_def.deprecated.is_some() {
                characteristics.push("Deprecated".to_string());
            }
            // Numeric value bounds, shown with ≥ / ≤ so they read distinctly
            // from the `min..max` *cardinality* badge below. `f64` Display
            // already drops a trailing `.0` (1.0 → "1", 0.5 → "0.5").
            if let Some(min) = slot_def.minimum_value {
                characteristics.push(format!("≥ {min}"));
            }
            if let Some(max) = slot_def.maximum_value {
                characteristics.push(format!("≤ {max}"));
            }
            if cardinality.min.is_some() || cardinality.max.is_some() {
                let lo = cardinality
                    .min
                    .map_or_else(|| "0".to_string(), |m| m.to_string());
                let hi = cardinality
                    .max
                    .map_or_else(|| "*".to_string(), |x| x.to_string());
                characteristics.push(format!("{lo}..{hi}"));
            }
            if let Some(inverse_id) = &slot_def.inverse {
                let inverse_label = schema
                    .slots
                    .get(inverse_id)
                    .and_then(|inv| inv.annotations.get("panschema:label"))
                    .cloned()
                    .unwrap_or_else(|| inverse_id.clone());
                characteristics.push(format!("Inverse of: {}", inverse_label));
            }

            let mappings = build_mappings(
                &slot_def.exact_mappings,
                &slot_def.close_mappings,
                &slot_def.related_mappings,
                &slot_def.narrow_mappings,
                &slot_def.broad_mappings,
                schema,
                labels,
            );

            let iri_href = slot_def
                .slot_uri
                .as_deref()
                .and_then(|s| crate::linkml_resolve::expand_curie(schema, s))
                .or_else(|| crate::linkml_resolve::expand_curie(schema, slot_id));

            slot_data_list.push(SlotData {
                id: (*slot_id).clone(),
                label,
                iri: slot_def
                    .slot_uri
                    .clone()
                    .unwrap_or_else(|| (*slot_id).clone()),
                iri_href,
                slot_type,
                description: slot_def
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
                domains,
                range,
                any_of,
                pattern: slot_def.pattern.clone(),
                characteristics,
                mappings,
                deprecated: slot_def.deprecated.clone(),
                aliases: slot_def.aliases.clone(),
                see_also: build_see_also(&slot_def.see_also, schema, labels),
                examples: slot_def.examples.clone(),
                default: slot_def.ifabsent.as_deref().map(format_ifabsent_default),
            });
        }

        // Build individual data from annotations
        let mut individual_refs = Vec::new();
        let mut individual_data_list = Vec::new();

        if let Some(individuals_str) = schema.annotations.get("panschema:individuals") {
            for ind_id in individuals_str.split(',') {
                let ind_id = ind_id.trim();
                if ind_id.is_empty() {
                    continue;
                }

                // Get type IRIs from annotation
                let type_key = format!("panschema:individual:{}", ind_id);
                let type_iris: Vec<String> = schema
                    .annotations
                    .get(&type_key)
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                    .unwrap_or_default();

                // Resolve types to EntityRefs
                let types: Vec<EntityRef> = type_iris
                    .iter()
                    .filter_map(|type_iri| {
                        // Extract class ID from IRI
                        let type_id = type_iri
                            .rfind('#')
                            .map(|pos| &type_iri[pos + 1..])
                            .unwrap_or(type_iri);

                        schema.classes.get(type_id).map(|c| {
                            let type_label = c
                                .annotations
                                .get("panschema:label")
                                .cloned()
                                .unwrap_or_else(|| type_id.to_string());
                            EntityRef {
                                id: type_id.to_string(),
                                label: type_label,
                            }
                        })
                    })
                    .collect();

                // Get property values from annotations
                let mut property_values = Vec::new();
                let prefix = format!("panschema:individual:{}:", ind_id);
                for (key, value) in &schema.annotations {
                    if key.starts_with(&prefix) {
                        let prop_id = &key[prefix.len()..];

                        let prop_ref = schema.slots.get(prop_id).map(|slot| {
                            let prop_label = slot
                                .annotations
                                .get("panschema:label")
                                .cloned()
                                .unwrap_or_else(|| prop_id.to_string());
                            EntityRef {
                                id: prop_id.to_string(),
                                label: prop_label,
                            }
                        });

                        let property_label = prop_ref
                            .as_ref()
                            .map(|r| r.label.clone())
                            .unwrap_or_else(|| prop_id.to_string());

                        property_values.push(PropertyValueData {
                            property_label,
                            property_ref: prop_ref,
                            value: value.clone(),
                        });
                    }
                }

                // Sort property values by property_id
                property_values.sort_by(|a, b| a.property_label.cmp(&b.property_label));

                // For now, use id as label (could be stored in annotation)
                let label = ind_id.chars().next().map_or(ind_id.to_string(), |c| {
                    c.to_uppercase().to_string() + &ind_id[1..]
                });

                individual_refs.push(EntityRef {
                    id: ind_id.to_string(),
                    label: label.clone(),
                });

                individual_data_list.push(IndividualData {
                    id: ind_id.to_string(),
                    label,
                    iri: format!("{}#{}", iri, ind_id),
                    description: None,
                    types,
                    property_values,
                });
            }
        }

        // Build enumeration data, sorted by name for stable output.
        let mut enum_refs = Vec::new();
        let mut enum_data_list = Vec::new();
        let mut sorted_enums: Vec<_> = schema.enums.iter().collect();
        sorted_enums.sort_by(|a, b| a.0.cmp(b.0));
        for (enum_id, enum_def) in sorted_enums {
            enum_refs.push(EntityRef {
                id: enum_id.clone(),
                label: enum_id.clone(),
            });
            let permissible_values = enum_def
                .permissible_values
                .iter()
                .map(|(text, pv)| PermissibleValueData {
                    text: text.clone(),
                    description: pv.description.clone(),
                    meaning: pv.meaning.as_deref().map(|raw| {
                        let href = crate::linkml_resolve::expand_curie(schema, raw);
                        let (label, definitions) = lookup_term(labels, href.as_deref());
                        ExternalLink {
                            display: raw.to_string(),
                            href,
                            label,
                            definitions,
                        }
                    }),
                })
                .collect();
            enum_data_list.push(EnumData {
                id: enum_id.clone(),
                label: enum_id.clone(),
                description: enum_def
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
                permissible_values,
                deprecated: enum_def.deprecated.clone(),
                aliases: enum_def.aliases.clone(),
                see_also: build_see_also(&enum_def.see_also, schema, labels),
                examples: enum_def.examples.clone(),
            });
        }

        // Build type data, sorted by name for stable output.
        let mut type_refs = Vec::new();
        let mut type_data_list = Vec::new();
        let mut sorted_types: Vec<_> = schema.types.iter().collect();
        sorted_types.sort_by(|a, b| a.0.cmp(b.0));
        for (type_id, type_def) in sorted_types {
            type_refs.push(EntityRef {
                id: type_id.clone(),
                label: type_id.clone(),
            });
            let uri = type_def.uri.as_deref().map(|raw| {
                let href = crate::linkml_resolve::expand_curie(schema, raw);
                let (label, definitions) = lookup_term(labels, href.as_deref());
                ExternalLink {
                    display: raw.to_string(),
                    href,
                    label,
                    definitions,
                }
            });
            // A parent type links to its own card when declared here.
            let base_type = type_def.typeof_.as_deref().map(|parent| EntityRef {
                id: parent.to_string(),
                label: parent.to_string(),
            });
            type_data_list.push(TypeData {
                id: type_id.clone(),
                label: type_id.clone(),
                uri,
                description: type_def
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
                base_type,
                pattern: type_def.pattern.clone(),
                deprecated: type_def.deprecated.clone(),
                aliases: type_def.aliases.clone(),
                see_also: build_see_also(&type_def.see_also, schema, labels),
                examples: type_def.examples.clone(),
            });
        }

        TemplateData {
            title,
            iri,
            version: schema.version.clone(),
            comment: schema
                .description
                .as_deref()
                .map(|d| render_description(d, schema)),
            namespaces,
            class_refs,
            class_tree: build_class_tree(&class_data_list),
            class_data: class_data_list,
            slot_refs,
            slot_data: slot_data_list,
            enum_refs,
            enum_data: enum_data_list,
            type_refs,
            type_data: type_data_list,
            individual_refs,
            individual_data: individual_data_list,
        }
    }
}

impl Default for HtmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Container for all template data
struct TemplateData {
    title: String,
    iri: String,
    version: Option<String>,
    comment: Option<String>,
    namespaces: Vec<Namespace>,
    class_refs: Vec<EntityRef>,
    class_data: Vec<ClassData>,
    class_tree: Vec<ClassTreeEntry>,
    slot_refs: Vec<EntityRef>,
    slot_data: Vec<SlotData>,
    enum_refs: Vec<EntityRef>,
    enum_data: Vec<EnumData>,
    type_refs: Vec<EntityRef>,
    type_data: Vec<TypeData>,
    individual_refs: Vec<EntityRef>,
    individual_data: Vec<IndividualData>,
}

impl Writer for HtmlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        // Create output directory if it doesn't exist
        fs::create_dir_all(output).map_err(IoError::Io)?;

        let data = Self::build_template_data_with_labels(schema, self.label_store.as_ref());

        // Generate graph JSON for visualization (only if enabled)
        let (graph_json_string, graph_node_count, graph_edge_count) = if self.include_graph {
            let graph_data = GraphWriter::new().schema_to_graph(schema);
            let node_count = graph_data.nodes.len();
            let edge_count = graph_data.edges.len();
            // The JSON is embedded in an inline <script>; serde_json does
            // not escape `<`, so a `</script>` inside any schema string
            // would close the element mid-JSON and execute what follows.
            // Escaping `<` as its `<` form keeps the JSON byte-for-byte
            // equivalent (JSON.parse decodes it back), so panschema-viz reads
            // the identical wire shape — only the on-page bytes change.
            let json = serde_json::to_string(&graph_data)
                .map_err(|e| IoError::Write(e.to_string()))?
                .replace('<', "\\u003c");
            (Some(json), node_count, edge_count)
        } else {
            (None, 0, 0)
        };

        let template = IndexTemplate {
            title: &data.title,
            iri: &data.iri,
            version: data.version.as_deref(),
            comment: data.comment.as_deref(),
            active_section: "metadata",
            classes: &data.class_refs,
            class_data: &data.class_data,
            class_tree: &data.class_tree,
            slots: &data.slot_refs,
            slot_data: &data.slot_data,
            enums: &data.enum_refs,
            enum_data: &data.enum_data,
            types: &data.type_refs,
            type_data: &data.type_data,
            individuals: &data.individual_refs,
            individual_data: &data.individual_data,
            namespaces: &data.namespaces,
            graph_json: graph_json_string.as_deref(),
            graph_node_count,
            graph_edge_count,
            graph_aspect_w: self.graph_aspect.0,
            graph_aspect_h: self.graph_aspect.1,
            graph_default_layout: &self.graph_default_layout,
            version_context: self.version_context.as_ref(),
            // `panschema generate` writes the page at the output root, so
            // `./` always resolves to the deploy root. `panschema publish`
            // sets this explicitly from the manifest's `site_root_url`.
            site_root_href: self.site_root_href.as_deref().unwrap_or("./"),
        };

        let html = template
            .render()
            .map_err(|e| IoError::Write(e.to_string()))?;

        let output_path = output.join("index.html");
        fs::write(&output_path, html).map_err(IoError::Io)?;

        // Copy WASM visualization files if graph is enabled
        if self.include_graph {
            fs::write(output.join("panschema_viz.js"), wasm_files::VIZ_JS).map_err(IoError::Io)?;
            fs::write(output.join("panschema_viz_bg.wasm"), wasm_files::VIZ_WASM)
                .map_err(IoError::Io)?;
        }

        Ok(())
    }

    fn format_id(&self) -> &str {
        "html"
    }
}

/// Render a LinkML `description:` value to HTML. Runs CommonMark
/// markdown over the input then expands `[[Name]]` cross-reference
/// markers against `schema` into anchor links.
///
/// Markdown handles inline links (`[text](url)`), emphasis
/// (`**bold**`, `*italic*`), code spans, and block constructs
/// (paragraphs, lists, fenced code). Raw HTML embedded in
/// descriptions is escaped — `<a href="…">…</a>` typed by the
/// author renders as literal angle-bracket text, not a real anchor.
/// Authors who need a clickable link use markdown syntax instead.
///
/// `[[Name]]` markers pass through markdown as plain text (no
/// markdown construct starts with `[[`), so post-processing the
/// rendered HTML to substitute them is safe — they only appear in
/// text nodes, never inside tag attributes.
fn render_description(text: &str, schema: &SchemaDefinition) -> String {
    use pulldown_cmark::{Event, Parser, html};

    // Route raw HTML through text escaping so author-embedded
    // `<a href="…">` cannot inject markup into the output. The
    // pulldown-cmark HTML renderer escapes `< > &` in `Event::Text`
    // automatically.
    let events = Parser::new(text).map(|ev| match ev {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        other => other,
    });
    let mut rendered = String::with_capacity(text.len());
    html::push_html(&mut rendered, events);
    substitute_xref_markers(&rendered, schema)
}

/// Walk the markdown-rendered HTML, replacing `[[Name]]` markers
/// (which markdown passes through as text — see [`render_description`])
/// with anchor links. Plain text outside markers is left as-is; it has
/// already been HTML-escaped by the markdown renderer.
fn substitute_xref_markers(html: &str, schema: &SchemaDefinition) -> String {
    let mut out = String::with_capacity(html.len());
    let mut remainder = html;
    while let Some((before, after_open)) = remainder.split_once("[[") {
        out.push_str(before);
        if let Some((name, after_close)) = after_open.split_once("]]")
            && is_xref_ident(name)
        {
            out.push_str(&render_xref(name, schema));
            remainder = after_close;
            continue;
        }
        out.push_str("[[");
        remainder = after_open;
    }
    out.push_str(remainder);
    out
}

fn is_xref_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Defaults are appended only for prefix names the schema didn't
/// declare, so generated docs that reference `xsd:string` etc. always
/// have a namespace entry even when the source schema is sparse.
fn build_namespaces(schema: &SchemaDefinition, schema_iri: &str) -> Vec<Namespace> {
    let mut out = Vec::with_capacity(schema.prefixes.len() + 5);
    out.push(Namespace {
        prefix: String::new(),
        iri: schema_iri.to_string(),
    });
    for (prefix, base) in &schema.prefixes {
        out.push(Namespace {
            prefix: prefix.clone(),
            iri: base.clone(),
        });
    }
    let defaults: &[(&str, &str)] = &[
        ("owl", "http://www.w3.org/2002/07/owl#"),
        ("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"),
        ("rdfs", "http://www.w3.org/2000/01/rdf-schema#"),
        ("xsd", "http://www.w3.org/2001/XMLSchema#"),
    ];
    for (prefix, iri) in defaults {
        if !schema.prefixes.contains_key(*prefix) {
            out.push(Namespace {
                prefix: (*prefix).to_string(),
                iri: (*iri).to_string(),
            });
        }
    }
    out
}

/// Map a LinkML range name onto a `RangeRef` — a class anchor link when
/// the range names a class declared in this schema, otherwise the bare
/// name as a datatype (covers LinkML primitives like `string`, `integer`,
/// `datetime`, plus enum / type names, plus unresolved CURIE-style refs
/// from imported schemas).
fn range_ref_for(range: &str, schema: &SchemaDefinition) -> RangeRef {
    if let Some(class_def) = schema.classes.get(range) {
        let label = class_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| range.to_string());
        RangeRef {
            class_ref: Some(EntityRef {
                id: range.to_string(),
                label,
            }),
            datatype: String::new(),
        }
    } else {
        RangeRef {
            class_ref: None,
            datatype: range.to_string(),
        }
    }
}

/// Build the rendered mapping list. The emission order (exact →
/// narrow → broad → related → close) follows SKOS strictness so the
/// reader's eye lands on tight matches first.
#[allow(clippy::too_many_arguments)]
fn build_mappings(
    exact: &[String],
    close: &[String],
    related: &[String],
    narrow: &[String],
    broad: &[String],
    schema: &SchemaDefinition,
    labels: Option<&crate::labels::LabelStore>,
) -> Vec<Mapping> {
    let mut out: Vec<Mapping> = Vec::new();
    for (kind, values) in [
        ("exact", exact),
        ("narrow", narrow),
        ("broad", broad),
        ("related", related),
        ("close", close),
    ] {
        for value in values {
            let href = crate::linkml_resolve::expand_curie(schema, value);
            let (label, definitions) = lookup_term(labels, href.as_deref());
            out.push(Mapping {
                kind,
                display: value.clone(),
                href,
                label,
                definitions,
            });
        }
    }
    out
}

/// Build the rendered `see_also` link list. Each URIorCURIE entry is
/// CURIE-expanded the same way mappings are, so a declared prefix
/// becomes a hyperlink and an undeclared one falls back to plain text.
fn build_see_also(
    see_also: &[String],
    schema: &SchemaDefinition,
    labels: Option<&crate::labels::LabelStore>,
) -> Vec<ExternalLink> {
    see_also
        .iter()
        .map(|raw| {
            let href = crate::linkml_resolve::expand_curie(schema, raw);
            let (label, definitions) = lookup_term(labels, href.as_deref());
            ExternalLink {
                display: raw.clone(),
                href,
                label,
                definitions,
            }
        })
        .collect()
}

/// Build the rendered `rules` list. Title/description pass through the
/// same markdown pipeline as [`ClassData::description`]; `summary` is
/// built from the pre/postconditions and rendered the same way, so
/// slot/value names referenced in either come out as `<code>`.
fn build_rules(rules: &[crate::linkml::ClassRule], schema: &SchemaDefinition) -> Vec<RuleInClass> {
    rules
        .iter()
        .map(|rule| RuleInClass {
            title: rule.title.clone(),
            description: rule
                .description
                .as_deref()
                .map(|d| render_description(d, schema)),
            summary: rule_summary_markdown(rule).map(|s| render_description(&s, schema)),
        })
        .collect()
}

/// Build the rendered `unique_keys` list, in stable name-sorted order
/// (the source is a `BTreeMap`). Descriptions pass through the same
/// markdown pipeline as [`ClassData::description`].
fn build_unique_keys(
    unique_keys: &std::collections::BTreeMap<String, crate::linkml::UniqueKey>,
    schema: &SchemaDefinition,
) -> Vec<UniqueKeyInClass> {
    unique_keys
        .iter()
        .map(|(name, key)| UniqueKeyInClass {
            name: name.clone(),
            slots: key.unique_key_slots.clone(),
            description: key
                .description
                .as_deref()
                .map(|d| render_description(d, schema)),
        })
        .collect()
}

/// Render a `ClassRule`'s pre/postconditions as one markdown "when …
/// then …" sentence. `None` when the rule carries neither (a
/// title/description-only entry).
fn rule_summary_markdown(rule: &crate::linkml::ClassRule) -> Option<String> {
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
fn describe_conditions(conditions: &crate::linkml::RuleConditions) -> Vec<String> {
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
    slot_conditions: &std::collections::BTreeMap<String, crate::linkml::SlotCondition>,
) -> Vec<String> {
    slot_conditions
        .iter()
        .filter_map(|(slot, cond)| describe_slot_condition(slot, cond))
        .collect()
}

fn describe_slot_condition(slot: &str, cond: &crate::linkml::SlotCondition) -> Option<String> {
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
                crate::linkml::ValuePresence::Present => "is present",
                crate::linkml::ValuePresence::Absent => "is absent",
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

/// Render a slot's `ifabsent` value readably for the Default row, peeling
/// the typed-form wrapper down to the value a reader cares about:
/// `ItemStatus(planned)` → `planned`, `int(8080)` → `8080`,
/// `float(1.0)` → `1.0`, `string(svc)` → `"svc"` (quoted, so a string
/// default is unambiguous), and a bare boolean (`true`/`True`) → `true`.
/// Any other form is shown verbatim.
fn format_ifabsent_default(raw: &str) -> String {
    let trimmed = raw.trim();
    match trimmed {
        "true" | "True" => return "true".to_string(),
        "false" | "False" => return "false".to_string(),
        _ => {}
    }
    if let Some((form, arg)) = trimmed.strip_suffix(')').and_then(|s| s.split_once('(')) {
        let arg = arg.trim();
        return if form.trim() == "string" {
            format!("\"{arg}\"")
        } else {
            // Enum / int / float / double all read best as the bare value.
            arg.to_string()
        };
    }
    trimmed.to_string()
}

/// `(label, definitions)` for an expanded IRI, when the store has it.
fn lookup_term(
    labels: Option<&crate::labels::LabelStore>,
    iri: Option<&str>,
) -> (Option<String>, Vec<String>) {
    match labels.zip(iri).and_then(|(store, iri)| store.lookup(iri)) {
        Some(info) => (info.label.clone(), info.definitions.clone()),
        None => (None, Vec::new()),
    }
}

fn render_xref(name: &str, schema: &SchemaDefinition) -> String {
    if schema.classes.contains_key(name) {
        format!(r##"<a href="#class-{name}" class="entity-ref class-ref">{name}</a>"##)
    } else if schema.enums.contains_key(name) {
        format!(r##"<a href="#enum-{name}" class="entity-ref enum-ref">{name}</a>"##)
    } else if schema.slots.contains_key(name) {
        format!(r##"<a href="#slot-{name}" class="entity-ref slot-ref">{name}</a>"##)
    } else if schema.types.contains_key(name) {
        format!(r##"<a href="#type-{name}" class="entity-ref type-ref">{name}</a>"##)
    } else {
        format!(
            "[[{name}]]<!-- WARNING: [[{name}]] does not resolve to a class, \
             enum, slot, or type in this schema -->"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Reader;
    use crate::owl_reader::OwlReader;
    use std::path::PathBuf;

    #[test]
    fn format_ifabsent_default_normalizes_booleans_and_quotes_strings() {
        // Capitalized LinkML booleans normalize to lowercase (a bare
        // `True`/`False` would otherwise pass through verbatim); a
        // `string(...)` default is quoted so it reads unambiguously; enum
        // and numeric forms show the bare value.
        assert_eq!(format_ifabsent_default("true"), "true");
        assert_eq!(format_ifabsent_default("True"), "true");
        assert_eq!(format_ifabsent_default("false"), "false");
        assert_eq!(format_ifabsent_default("False"), "false");
        assert_eq!(format_ifabsent_default("string(svc)"), "\"svc\"");
        assert_eq!(format_ifabsent_default("int(8080)"), "8080");
        assert_eq!(format_ifabsent_default("ItemStatus(planned)"), "planned");
    }

    fn cohort_context(viewing: &str, current: &str, edge: Option<&str>) -> VersionContext {
        VersionContext {
            all_versions: vec!["main".into(), "v0.2.0".into(), "v0.1.0".into()],
            viewing: viewing.into(),
            current: current.into(),
            edge: edge.map(String::from),
            url_pattern: "/schema/{version}/".into(),
        }
    }

    #[test]
    fn html_writer_default_layout_is_auto() {
        // `auto` is the not-pinned sentinel: the viz picks a default
        // from the graph's inheritance density at render time
        // (Hierarchical for `is_a`-heavy schemas, SGD otherwise). The
        // manifest's `html_default_layout` field still overrides. This
        // pins the in-tree fallback so a regression that hard-codes a
        // concrete default (defeating the auto-detect) fails loudly.
        assert_eq!(HtmlWriter::new().graph_default_layout, "auto");
        assert_eq!(HtmlWriter::with_options(true).graph_default_layout, "auto");
        assert_eq!(HtmlWriter::with_options(false).graph_default_layout, "auto");
    }

    #[test]
    fn version_context_is_edge_matches_only_edge_ref() {
        let vc = cohort_context("v0.1.0", "v0.2.0", Some("main"));
        assert!(vc.is_edge("main"));
        assert!(!vc.is_edge("v0.1.0"));
        assert!(!vc.is_edge("v0.2.0"));
        assert!(!vc.is_edge("not-a-ref"));

        // When `edge` is None, nothing is the edge — every probe returns false.
        let vc_no_edge = cohort_context("v0.1.0", "v0.2.0", None);
        assert!(!vc_no_edge.is_edge("main"));
        assert!(!vc_no_edge.is_edge("v0.1.0"));
    }

    #[test]
    fn version_context_viewing_predicates_distinguish_current_and_edge() {
        let viewing_current = cohort_context("v0.2.0", "v0.2.0", Some("main"));
        assert!(viewing_current.viewing_is_current());
        assert!(!viewing_current.viewing_is_edge());

        let viewing_edge = cohort_context("main", "v0.2.0", Some("main"));
        assert!(!viewing_edge.viewing_is_current());
        assert!(viewing_edge.viewing_is_edge());

        let viewing_stale = cohort_context("v0.1.0", "v0.2.0", Some("main"));
        assert!(!viewing_stale.viewing_is_current());
        assert!(!viewing_stale.viewing_is_edge());
    }

    #[test]
    fn version_context_url_for_substitutes_version_placeholder() {
        let vc = cohort_context("v0.1.0", "v0.2.0", None);
        assert_eq!(vc.url_for("v0.2.0"), "/schema/v0.2.0/");
        assert_eq!(vc.url_for("main"), "/schema/main/");
        // A pattern without the placeholder is returned unchanged.
        let vc_no_placeholder = VersionContext {
            url_pattern: "/static-url".into(),
            ..vc
        };
        assert_eq!(vc_no_placeholder.url_for("v0.2.0"), "/static-url");
    }

    fn reference_ontology_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("reference.ttl")
    }

    #[test]
    fn html_writer_format_id() {
        let writer = HtmlWriter::new();
        assert_eq!(writer.format_id(), "html");
    }

    #[test]
    fn html_writer_emits_single_version_brand_link_as_dot_slash() {
        // `panschema generate` writes index.html at the output root,
        // so the brand link must be `./` — equivalent to the deploy
        // root from that path. The versioned `panschema publish`
        // path is exercised separately in publish::tests.
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();
        assert!(
            html.contains(r#"<a href="./" class="site-title""#),
            "single-version output must use `./` brand link"
        );
        assert!(
            !html.contains(r#"<a href="/" class="site-title""#),
            "absolute brand link must not appear"
        );
    }

    #[test]
    fn html_writer_renders_schema_description_markdown_as_live_html() {
        // The schema-level description is mounted into the metadata
        // card. Like the entity cards, the writer hands the template
        // already-rendered HTML from `render_description`, so the
        // template must mount it via `|safe` — otherwise Askama
        // double-escapes the writer's output and the user sees the
        // literal `<p>…<a href="…">…</a>` markup as visible text
        // instead of a live link.
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("http://example.org/s".to_string());
        schema.description = Some(
            "see the [book](https://example.org/book) for context — Noy & McGuinness".to_string(),
        );
        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();

        assert!(
            html.contains(r#"<a href="https://example.org/book">book</a>"#),
            "schema description markdown link must render as a live anchor; got: {html}"
        );
        // Double-escape signature: any of the writer-produced markup
        // appearing as escaped text means Askama escaped it a second
        // time. `&lt;a ` would mean the anchor's own `<a` got escaped;
        // `&amp;amp;` / `&#38;amp;` would mean the writer's `&amp;` got
        // re-escaped.
        assert!(
            !html.contains("&lt;a "),
            "rendered anchor must not be re-escaped; got: {html}"
        );
        assert!(
            !html.contains("&amp;amp;") && !html.contains("&#38;amp;"),
            "ampersand must not be double-escaped; got: {html}"
        );
    }

    #[test]
    fn html_writer_builds_template_data_from_schema() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        assert_eq!(data.title, "panschema Reference Ontology");
        assert!(data.iri.contains("panschema/reference"));
        assert_eq!(data.version, Some("0.2.0".to_string()));
    }

    #[test]
    fn html_writer_builds_class_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 6 classes
        assert_eq!(data.class_refs.len(), 6);
        assert_eq!(data.class_data.len(), 6);

        // Find Dog class
        let dog = data.class_data.iter().find(|c| c.id == "Dog").unwrap();
        assert_eq!(dog.label, "Dog");
        assert!(dog.superclass.is_some());
        assert_eq!(dog.superclass.as_ref().unwrap().id, "Mammal");
    }

    #[test]
    fn class_tree_nests_reference_hierarchy_preorder() {
        // Animal → {Mammal → {Cat, Dog}, Pet}, plus Person as a
        // disconnected root rendered flat alongside the tree. `closes`
        // counts the ancestor levels a leaf is the last descendant of, so
        // the template can emit matching `</ul></li>` pairs.
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let data = HtmlWriter::build_template_data(&schema);

        let got: Vec<(&str, usize, bool, usize)> = data
            .class_tree
            .iter()
            .map(|e| {
                (
                    data.class_data[e.index].id.as_str(),
                    e.depth,
                    e.has_children,
                    e.closes,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("Animal", 0, true, 0),
                ("Mammal", 1, true, 0),
                ("Cat", 2, false, 0),
                ("Dog", 2, false, 1),
                ("Pet", 1, false, 1),
                ("Person", 0, false, 0),
            ]
        );
    }

    #[test]
    fn class_tree_flat_order_is_alphabetical_rank() {
        // The flat view sorts cards by CSS `order`; each entry's rank
        // must match the class's position in the alphabetical
        // `class_data` list.
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let data = HtmlWriter::build_template_data(&schema);

        let mut indices: Vec<usize> = data.class_tree.iter().map(|e| e.index).collect();
        indices.sort_unstable();
        let expected: Vec<usize> = (0..data.class_data.len()).collect();
        assert_eq!(
            indices, expected,
            "every alphabetical rank appears exactly once as a flat-order index"
        );
    }

    #[test]
    fn class_tree_mixin_consumer_appears_once_under_is_a_parent() {
        // Mixins don't create tree edges — a class with both an
        // `is_a` parent and a mixin nests under the parent only and
        // appears exactly once.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Base".to_string(), ClassDefinition::new("Base"));
        schema
            .classes
            .insert("Auditable".to_string(), ClassDefinition::new("Auditable"));
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Base".to_string());
        child.mixins = vec!["Auditable".to_string()];
        schema.classes.insert("Child".to_string(), child);

        let data = HtmlWriter::build_template_data(&schema);

        let child_entries: Vec<_> = data
            .class_tree
            .iter()
            .filter(|e| data.class_data[e.index].id == "Child")
            .collect();
        assert_eq!(child_entries.len(), 1, "Child must appear exactly once");
        assert_eq!(child_entries[0].depth, 1, "Child nests under Base only");
        assert_eq!(data.class_tree.len(), 3, "every class appears in the tree");
    }

    #[test]
    fn class_tree_unresolved_is_a_parent_renders_as_root() {
        // An `is_a` pointing at a class missing from the schema (e.g.
        // an un-loaded import) must not drop the class from the tree.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut orphan = ClassDefinition::new("Orphan");
        orphan.is_a = Some("Ghost".to_string());
        schema.classes.insert("Orphan".to_string(), orphan);

        let data = HtmlWriter::build_template_data(&schema);

        assert_eq!(data.class_tree.len(), 1);
        assert_eq!(data.class_data[data.class_tree[0].index].id, "Orphan");
        assert_eq!(data.class_tree[0].depth, 0);
    }

    #[test]
    fn class_tree_is_a_cycle_fails_open_to_roots() {
        // A pathological `is_a` cycle must not infinite-loop or drop
        // classes: cycle members still render, each exactly once.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut a = ClassDefinition::new("Alpha");
        a.is_a = Some("Beta".to_string());
        let mut b = ClassDefinition::new("Beta");
        b.is_a = Some("Alpha".to_string());
        schema.classes.insert("Alpha".to_string(), a);
        schema.classes.insert("Beta".to_string(), b);

        let data = HtmlWriter::build_template_data(&schema);

        let mut ids: Vec<&str> = data
            .class_tree
            .iter()
            .map(|e| data.class_data[e.index].id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["Alpha", "Beta"], "cycle members each appear once");
    }

    #[test]
    fn class_tree_close_tags_emit_matching_pairs() {
        // The template closes a leaf's open ancestors via this string;
        // a leaf with children contributes nothing (its `<ul>` is
        // closed by its last descendant).
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let data = HtmlWriter::build_template_data(&schema);

        // Dog is the last child of Mammal but not of Animal (Pet follows
        // Mammal under Animal), so Dog closes only Mammal's level.
        let dog = data
            .class_tree
            .iter()
            .find(|e| data.class_data[e.index].id == "Dog")
            .unwrap();
        assert_eq!(dog.close_tags(), "</ul></li>");
        // Pet, the last child of Animal, closes Animal's level.
        let pet = data
            .class_tree
            .iter()
            .find(|e| data.class_data[e.index].id == "Pet")
            .unwrap();
        assert_eq!(pet.close_tags(), "</ul></li>");
        let animal = data
            .class_tree
            .iter()
            .find(|e| data.class_data[e.index].id == "Animal")
            .unwrap();
        assert_eq!(animal.close_tags(), "");
    }

    #[test]
    fn class_card_slots_carry_origin_for_inherited_entries() {
        // The card tags inherited slots with where they came from;
        // the class's own slots carry no tag.
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut named = ClassDefinition::new("Named");
        named
            .attributes
            .insert("name".into(), SlotDefinition::new("name"));
        schema.classes.insert("Named".into(), named);
        let mut person = ClassDefinition::new("Person");
        person.mixins = vec!["Named".into()];
        person
            .attributes
            .insert("email".into(), SlotDefinition::new("email"));
        schema.classes.insert("Person".into(), person);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Person").unwrap();
        let name = card.slots.iter().find(|s| s.name == "name").unwrap();
        assert_eq!(name.origin.as_deref(), Some("mixin Named"));
        let email = card.slots.iter().find(|s| s.name == "email").unwrap();
        assert_eq!(email.origin, None);
    }

    #[test]
    fn inherited_slot_description_moves_to_tooltip() {
        // The defining class's card owns the inline description;
        // inheriting cards render the slot compactly with the
        // description as a hover tooltip — otherwise every subclass
        // repeats the parent's prose.
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        let mut field = SlotDefinition::new("field");
        field.description = Some("What this field asserts.".into());
        parent.attributes.insert("field".into(), field);
        schema.classes.insert("Parent".into(), parent);
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".into());
        schema.classes.insert("Child".into(), child);

        let data = HtmlWriter::build_template_data(&schema);
        let on_parent = data.class_data.iter().find(|c| c.id == "Parent").unwrap();
        let parent_slot = on_parent.slots.iter().find(|s| s.name == "field").unwrap();
        assert!(parent_slot.description.is_some(), "definer renders inline");
        assert_eq!(parent_slot.description_tooltip, None);

        let on_child = data.class_data.iter().find(|c| c.id == "Child").unwrap();
        let child_slot = on_child.slots.iter().find(|s| s.name == "field").unwrap();
        assert_eq!(child_slot.description, None, "inheritor renders compactly");
        assert_eq!(
            child_slot.description_tooltip.as_deref(),
            Some("What this field asserts.")
        );
    }

    #[test]
    fn class_card_slot_framing_uses_effective_cardinality() {
        // Explicit cardinality bounds decide the rendered
        // required/multivalued framing, not the raw flags: a slot
        // bounded 1..1 renders as required and single-valued even
        // with both flags unset.
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut thing = ClassDefinition::new("Thing");
        let mut ident = SlotDefinition::new("ident");
        ident.minimum_cardinality = Some(1);
        ident.maximum_cardinality = Some(1);
        thing.attributes.insert("ident".into(), ident);
        schema.classes.insert("Thing".into(), thing);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Thing").unwrap();
        let slot = card.slots.iter().find(|s| s.name == "ident").unwrap();
        assert!(slot.required, "min >= 1 renders as required");
        assert!(!slot.multivalued, "max == 1 renders as single-valued");
    }

    #[test]
    fn class_card_renders_induced_per_class_slot_range() {
        // A subclass narrowing an inherited `any_of` union via
        // `slot_usage` shows its induced range on the card, not the
        // wide inherited union: a scalar narrows to a single range, a
        // smaller union replaces, and `maximum_cardinality: 0` reads
        // as "has no value".
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};

        let union = |members: &[&str]| {
            let mut s = SlotDefinition::new("u");
            s.any_of = members
                .iter()
                .map(|m| {
                    let mut b = SlotDefinition::new("u");
                    b.range = Some((*m).to_string());
                    b
                })
                .collect();
            s
        };

        let mut schema = SchemaDefinition::new("acts");
        for artifact in ["Question", "Result", "Dataset", "Annotation"] {
            schema
                .classes
                .insert(artifact.into(), ClassDefinition::new(artifact));
        }
        let mut has_input = union(&["Question", "Result", "Dataset", "Annotation"]);
        has_input.name = "hasInput".into();
        schema.slots.insert("hasInput".into(), has_input);
        let mut has_output = union(&["Result", "Dataset"]);
        has_output.name = "hasOutput".into();
        schema.slots.insert("hasOutput".into(), has_output);

        let mut act = ClassDefinition::new("Act");
        act.slots = vec!["hasInput".into(), "hasOutput".into()];
        schema.classes.insert("Act".into(), act);

        // Analysis: scalar narrows hasInput to a single Dataset range.
        let mut analysis = ClassDefinition::new("Analysis");
        analysis.is_a = Some("Act".into());
        let mut in_narrow = SlotDefinition::new("hasInput");
        in_narrow.range = Some("Dataset".into());
        analysis.slot_usage.insert("hasInput".into(), in_narrow);
        schema.classes.insert("Analysis".into(), analysis);

        // EvidenceExtraction: a smaller (2-member) union replaces the
        // inherited 4-member union on hasInput.
        let mut extraction = ClassDefinition::new("EvidenceExtraction");
        extraction.is_a = Some("Act".into());
        extraction
            .slot_usage
            .insert("hasInput".into(), union(&["Annotation", "Result"]));
        schema
            .classes
            .insert("EvidenceExtraction".into(), extraction);

        // EvidenceAssessment: suppresses hasOutput.
        let mut assessment = ClassDefinition::new("EvidenceAssessment");
        assessment.is_a = Some("Act".into());
        let mut no_output = SlotDefinition::new("hasOutput");
        no_output.maximum_cardinality = Some(0);
        assessment.slot_usage.insert("hasOutput".into(), no_output);
        schema
            .classes
            .insert("EvidenceAssessment".into(), assessment);

        let data = HtmlWriter::build_template_data(&schema);
        let card = |name: &str| data.class_data.iter().find(|c| c.id == name).unwrap();
        let slot = |c: &ClassData, n: &str| c.slots.iter().find(|s| s.name == n).unwrap().clone();

        // Scalar narrowing: single induced range, no lingering union.
        let analysis_in = slot(card("Analysis"), "hasInput");
        assert!(
            analysis_in.any_of.is_empty(),
            "lingering union must not survive"
        );
        assert_eq!(
            analysis_in
                .range
                .as_ref()
                .and_then(|r| r.class_ref.as_ref())
                .map(|c| c.id.as_str()),
            Some("Dataset")
        );

        // Union narrowing: the smaller union replaces the inherited one.
        let extraction_in = slot(card("EvidenceExtraction"), "hasInput");
        let in_members: Vec<&str> = extraction_in
            .any_of
            .iter()
            .filter_map(|r| r.class_ref.as_ref().map(|c| c.id.as_str()))
            .collect();
        assert_eq!(in_members, vec!["Annotation", "Result"]);

        // Suppression: no range, the suppressed flag is set.
        let suppressed = slot(card("EvidenceAssessment"), "hasOutput");
        assert!(suppressed.suppressed);
        assert!(suppressed.range.is_none() && suppressed.any_of.is_empty());

        // The base class still shows the full union.
        let act_in = slot(card("Act"), "hasInput");
        assert_eq!(
            act_in.any_of.len(),
            4,
            "unrefined class keeps the full union"
        );
    }

    #[test]
    fn html_writer_renders_enum_and_type_sections() {
        // Enums and types each get their own HTML section, card, and
        // sidebar entry — parity with every node kind the graph draws.
        use crate::linkml::{EnumDefinition, PermissibleValue, SchemaDefinition, TypeDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("xsd".into(), "http://www.w3.org/2001/XMLSchema#".into());

        let mut status = EnumDefinition::new("Status");
        status.description = Some("Lifecycle status.".into());
        let mut open = PermissibleValue::new("open");
        open.description = Some("Open for changes.".into());
        open.meaning = Some("xsd:string".into());
        status.permissible_values.insert("open".into(), open);
        status
            .permissible_values
            .insert("closed".into(), PermissibleValue::new("closed"));
        schema.enums.insert("Status".into(), status);

        let mut phone = TypeDefinition::new("PhoneNumber");
        phone.description = Some("An E.164 phone number.".into());
        phone.typeof_ = Some("string".into());
        phone.uri = Some("xsd:string".into());
        phone.pattern = Some(r"^\+[1-9]\d{1,14}$".into());
        schema.types.insert("PhoneNumber".into(), phone);

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_enum_type_sections_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write failed");
        let html =
            fs::read_to_string(temp_dir.join("index.html")).expect("failed to read index.html");
        let _ = fs::remove_dir_all(&temp_dir);

        // Enumerations section + card + permissible values.
        assert!(html.contains(r#"id="enums""#), "enums section present");
        assert!(html.contains(r#"id="enum-Status""#), "enum card present");
        assert!(html.contains("Permissible values"));
        assert!(html.contains(">open<") && html.contains(">closed<"));
        assert!(
            html.contains("http://www.w3.org/2001/XMLSchema#string"),
            "the value's expanded meaning IRI is hyperlinked"
        );

        // Types section + card + constraints.
        assert!(html.contains(r#"id="types""#), "types section present");
        assert!(
            html.contains(r#"id="type-PhoneNumber""#),
            "type card present"
        );
        assert!(html.contains(r"^\+[1-9]\d{1,14}$"), "type pattern rendered");

        // Sidebar nav entries.
        assert!(html.contains(r##"href="#enums""##) && html.contains("Enumerations"));
        assert!(html.contains(r##"href="#types""##));
    }

    #[test]
    fn html_writer_class_data_resolves_mixin_entity_refs() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};

        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Auditable".to_string(), ClassDefinition::new("Auditable"));
        schema.classes.insert(
            "Publishable".to_string(),
            ClassDefinition::new("Publishable"),
        );
        let mut doc = ClassDefinition::new("Document");
        doc.mixins = vec!["Auditable".to_string(), "Publishable".to_string()];
        schema.classes.insert("Document".to_string(), doc);

        let data = HtmlWriter::build_template_data(&schema);
        let document = data.class_data.iter().find(|c| c.id == "Document").unwrap();
        let mixin_ids: Vec<&str> = document.mixins.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(mixin_ids, vec!["Auditable", "Publishable"]);
    }

    #[test]
    fn html_writer_class_data_skips_unresolved_mixin_refs() {
        // Anchor links to a missing class card would be broken; skip
        // is the conservative choice over emitting a dead link.
        use crate::linkml::{ClassDefinition, SchemaDefinition};

        let mut schema = SchemaDefinition::new("s");
        let mut doc = ClassDefinition::new("Document");
        doc.mixins = vec!["Phantom".to_string()];
        schema.classes.insert("Document".to_string(), doc);

        let data = HtmlWriter::build_template_data(&schema);
        let document = data.class_data.iter().find(|c| c.id == "Document").unwrap();
        assert!(
            document.mixins.is_empty(),
            "expected unresolved mixin to be skipped; got: {:?}",
            document.mixins
        );
    }

    #[test]
    fn render_description_links_known_class_reference() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let html = render_description("see [[Question]] for context", &schema);
        assert!(
            html.contains(
                r##"<a href="#class-Question" class="entity-ref class-ref">Question</a>"##
            ),
            "expected class anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_links_known_enum_reference() {
        use crate::linkml::{EnumDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .enums
            .insert("ActStatus".to_string(), EnumDefinition::new("ActStatus"));
        let html = render_description("captured by the [[ActStatus]] enum", &schema);
        assert!(
            html.contains(
                r##"<a href="#enum-ActStatus" class="entity-ref enum-ref">ActStatus</a>"##
            ),
            "expected enum anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_links_known_slot_reference() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("status".to_string(), SlotDefinition::new("status"));
        let html = render_description("the [[status]] slot", &schema);
        assert!(
            html.contains(r##"<a href="#slot-status" class="entity-ref slot-ref">status</a>"##),
            "expected slot anchor; got: {html}"
        );
    }

    #[test]
    fn render_description_emits_warning_comment_for_unresolved_reference() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("nothing here: [[Phantom]]", &schema);
        assert!(
            html.contains("[[Phantom]]"),
            "expected literal text; got: {html}"
        );
        assert!(
            html.contains("<!-- WARNING:"),
            "expected warning comment; got: {html}"
        );
    }

    #[test]
    fn render_description_html_escapes_surrounding_plain_text() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("if a < b & c > d", &schema);
        // `< > &` in body content must be escaped — the rendered HTML
        // is mounted via `|safe` in entity descriptions, so the writer
        // can't lean on Askama for escaping. `"` and `'` are body-safe
        // in HTML5 and pass through, matching CommonMark output.
        assert!(html.contains("&lt;"), "got: {html}");
        assert!(html.contains("&amp;"), "got: {html}");
        assert!(html.contains("&gt;"), "got: {html}");
    }

    #[test]
    fn render_description_passes_body_safe_quotes_through() {
        // Descriptions land in element body content (mounted into
        // `<div class="entity-description">…</div>`), where `"` and
        // `'` are HTML5-safe and need no escape. CommonMark output
        // matches; we keep authors' quote characters readable in the
        // rendered source instead of `&quot;`/`&#39;`-encoding them.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description(r#"says "hi" and 'bye'"#, &schema);
        assert!(html.contains(r#"says "hi" and 'bye'"#), "got: {html}");
    }

    #[test]
    fn render_description_rejects_invalid_xref_idents() {
        // `[[Name]]` requires a LinkML-style ident: alphabetic or `_`
        // first char, alphanumeric or `_` continuation. Anything else
        // is treated as literal `[[...]]` text, not a cross-reference.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        assert!(render_description("[[]]", &schema).contains("[[]]"));
        assert!(render_description("[[123abc]]", &schema).contains("[[123abc]]"));
        assert!(render_description("[[has space]]", &schema).contains("[[has space]]"));
        assert!(render_description("[[a-b]]", &schema).contains("[[a-b]]"));
    }

    #[test]
    fn render_description_accepts_underscore_leading_xref_ident() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("_Internal".to_string(), ClassDefinition::new("_Internal"));
        let html = render_description("[[_Internal]]", &schema);
        assert!(
            html.contains(r##"<a href="#class-_Internal""##),
            "expected underscore-leading ident to resolve; got: {html}"
        );
    }

    #[test]
    fn render_description_passes_lone_brackets_through() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("[note] and [[unclosed", &schema);
        assert!(html.contains("[note] and [[unclosed"), "got: {html}");
    }

    #[test]
    fn render_description_renders_markdown_inline_links() {
        // `[text](url)` is the canonical markdown affordance for
        // embedding a clickable link in a description. Markdown links
        // cover external URLs that don't fit the xref mechanism (book
        // chapters, papers, glossaries); the in-band `[[Name]]` marker
        // remains how a description references another schema entity.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("see the [book](../../) for context", &schema);
        assert!(
            html.contains(r#"<a href="../../">book</a>"#),
            "expected rendered markdown link; got: {html}"
        );
    }

    #[test]
    fn render_description_renders_markdown_emphasis_and_code() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description("**bold** and *italic* and `code`", &schema);
        assert!(
            html.contains("<strong>bold</strong>"),
            "expected bold; got: {html}"
        );
        assert!(
            html.contains("<em>italic</em>"),
            "expected italic; got: {html}"
        );
        assert!(
            html.contains("<code>code</code>"),
            "expected code; got: {html}"
        );
    }

    #[test]
    fn render_description_escapes_raw_html_embedded_by_author() {
        // HTML safety policy: markdown only. Raw HTML in descriptions
        // is escaped so an author can't smuggle markup (or worse,
        // scripts) into the rendered page. The schema author who needs
        // a link uses markdown `[text](url)` syntax instead.
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let html = render_description(r#"plain <a href="evil.html">click</a> tail"#, &schema);
        assert!(
            !html.contains(r#"<a href="evil.html">"#),
            "raw HTML must not survive verbatim; got: {html}"
        );
        // The literal `<a` opener must be escaped in the rendered output.
        assert!(
            html.contains("&lt;a "),
            "raw HTML must be escaped; got: {html}"
        );
    }

    #[test]
    fn render_description_preserves_xref_inside_markdown_link_text() {
        // A `[[ClassName]]` marker nested inside a markdown link's
        // text is processed by xref expansion after markdown renders,
        // so the anchor's display text becomes the resolved entity
        // link. Verifies the ordering decision: markdown first, then
        // xref substitution against the rendered HTML.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Question".to_string(), ClassDefinition::new("Question"));
        let html = render_description("[via [[Question]]](../../)", &schema);
        // Outer markdown link survives.
        assert!(html.contains(r#"<a href="../../">"#), "got: {html}");
        // Inner xref also resolves.
        assert!(
            html.contains(r##"<a href="#class-Question""##),
            "got: {html}"
        );
    }

    #[test]
    fn build_template_data_resolves_xrefs_in_class_description() {
        use crate::linkml::{ClassDefinition, EnumDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .enums
            .insert("ActStatus".to_string(), EnumDefinition::new("ActStatus"));
        let mut planned = ClassDefinition::new("PlannedAct");
        planned.description = Some("lifecycle captured by the [[ActStatus]] enum".to_string());
        schema.classes.insert("PlannedAct".to_string(), planned);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data
            .class_data
            .iter()
            .find(|c| c.id == "PlannedAct")
            .unwrap();
        let desc = card.description.as_deref().unwrap();
        assert!(
            desc.contains(r##"<a href="#enum-ActStatus""##),
            "expected resolved xref in class description; got: {desc}"
        );
    }

    #[test]
    fn build_namespaces_includes_schema_declared_prefixes() {
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("http://example.org/s".to_string());
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        schema.prefixes.insert(
            "obo".to_string(),
            "http://purl.obolibrary.org/obo/".to_string(),
        );

        let ns = build_namespaces(&schema, "http://example.org/s");
        let by_prefix: std::collections::BTreeMap<&str, &str> = ns
            .iter()
            .map(|n| (n.prefix.as_str(), n.iri.as_str()))
            .collect();
        assert_eq!(
            by_prefix.get("cco"),
            Some(&"https://www.commoncoreontologies.org/")
        );
        assert_eq!(
            by_prefix.get("obo"),
            Some(&"http://purl.obolibrary.org/obo/")
        );
    }

    #[test]
    fn build_namespaces_appends_default_prefixes_when_schema_lacks_them() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let ns = build_namespaces(&schema, "http://example.org/s");
        let prefixes: Vec<&str> = ns.iter().map(|n| n.prefix.as_str()).collect();
        for default in ["owl", "rdf", "rdfs", "xsd"] {
            assert!(
                prefixes.contains(&default),
                "missing default prefix `{default}`; got: {prefixes:?}"
            );
        }
    }

    #[test]
    fn build_namespaces_lets_schema_prefix_override_default() {
        use crate::linkml::SchemaDefinition;
        let mut schema = SchemaDefinition::new("s");
        schema.prefixes.insert(
            "xsd".to_string(),
            "https://example.org/custom-xsd#".to_string(),
        );
        let ns = build_namespaces(&schema, "http://example.org/s");
        let xsd_entries: Vec<&Namespace> = ns.iter().filter(|n| n.prefix == "xsd").collect();
        assert_eq!(xsd_entries.len(), 1, "xsd must appear exactly once");
        assert_eq!(xsd_entries[0].iri, "https://example.org/custom-xsd#");
    }

    #[test]
    fn build_namespaces_keeps_schema_local_empty_prefix() {
        use crate::linkml::SchemaDefinition;
        let schema = SchemaDefinition::new("s");
        let ns = build_namespaces(&schema, "http://example.org/local");
        assert_eq!(
            ns.iter()
                .find(|n| n.prefix.is_empty())
                .map(|n| n.iri.as_str()),
            Some("http://example.org/local")
        );
    }

    #[test]
    fn class_data_lists_resolved_slots_with_framing() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        let mut def = ClassDefinition::new("Question");
        let mut label = SlotDefinition::new("label");
        label.range = Some("string".to_string());
        label.required = true;
        def.attributes.insert("label".to_string(), label);
        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.multivalued = true;
        def.attributes.insert("tags".to_string(), tags);
        schema.classes.insert("Question".to_string(), def);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Question").unwrap();
        let by_name: std::collections::BTreeMap<&str, &SlotInClass> =
            card.slots.iter().map(|s| (s.name.as_str(), s)).collect();

        let label_slot = by_name["label"];
        assert!(label_slot.required);
        assert!(!label_slot.multivalued);
        assert!(!label_slot.refined_here);
        assert_eq!(label_slot.range.as_ref().unwrap().datatype, "string");

        let tags_slot = by_name["tags"];
        assert!(!tags_slot.required);
        assert!(tags_slot.multivalued);
    }

    #[test]
    fn class_data_flags_slot_usage_refinements_with_refined_here() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");

        // Global slot defined as optional.
        let mut global = SlotDefinition::new("status");
        global.range = Some("string".to_string());
        schema.slots.insert("status".to_string(), global);

        // Parent declares the slot reference.
        let mut parent = ClassDefinition::new("Parent");
        parent.slots.push("status".to_string());
        schema.classes.insert("Parent".to_string(), parent);

        // Child inherits from Parent AND narrows `status` to required.
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".to_string());
        let mut override_def = SlotDefinition::new("status");
        override_def.required = true;
        child.slot_usage.insert("status".to_string(), override_def);
        schema.classes.insert("Child".to_string(), child);

        let data = HtmlWriter::build_template_data(&schema);
        let parent_card = data.class_data.iter().find(|c| c.id == "Parent").unwrap();
        let child_card = data.class_data.iter().find(|c| c.id == "Child").unwrap();

        // Parent has the slot but doesn't refine it.
        let parent_status = parent_card
            .slots
            .iter()
            .find(|s| s.name == "status")
            .unwrap();
        assert!(!parent_status.refined_here);
        assert!(!parent_status.required);

        // Child refines it: required = true AND refined_here = true.
        let child_status = child_card
            .slots
            .iter()
            .find(|s| s.name == "status")
            .unwrap();
        assert!(child_status.refined_here);
        assert!(child_status.required);
    }

    #[test]
    fn class_data_resolves_any_of_branches_into_range_refs() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Hypothesis".to_string(), ClassDefinition::new("Hypothesis"));
        schema
            .classes
            .insert("Evidence".to_string(), ClassDefinition::new("Evidence"));

        let mut def = ClassDefinition::new("DesignOfExperiment");
        let mut slot = SlotDefinition::new("hasInput");
        let mut hypothesis_branch = SlotDefinition::new("hasInput");
        hypothesis_branch.range = Some("Hypothesis".to_string());
        let mut evidence_branch = SlotDefinition::new("hasInput");
        evidence_branch.range = Some("Evidence".to_string());
        slot.any_of = vec![hypothesis_branch, evidence_branch];
        def.attributes.insert("hasInput".to_string(), slot);
        schema.classes.insert("DesignOfExperiment".to_string(), def);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data
            .class_data
            .iter()
            .find(|c| c.id == "DesignOfExperiment")
            .unwrap();
        let slot = card.slots.iter().find(|s| s.name == "hasInput").unwrap();
        let any_of_ids: Vec<&str> = slot
            .any_of
            .iter()
            .filter_map(|r| r.class_ref.as_ref().map(|c| c.id.as_str()))
            .collect();
        assert_eq!(any_of_ids, vec!["Hypothesis", "Evidence"]);
    }

    #[test]
    fn slot_card_shows_bounds_badge_when_only_one_bound_is_set() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        // A slot with only `minimum_cardinality` (no max) must still get
        // a `min..*` bounds badge. Guards the
        // `min.is_some() || max.is_some()` gate against collapsing to
        // `&&`, which would hide bounds unless *both* ends are declared.
        let mut schema = SchemaDefinition::new("bounds");
        let mut members = SlotDefinition::new("members");
        members.minimum_cardinality = Some(2);
        schema.slots.insert("members".to_string(), members);

        let data = HtmlWriter::build_template_data(&schema);
        let prop = data.slot_data.iter().find(|p| p.id == "members").unwrap();
        assert!(
            prop.characteristics.iter().any(|c| c == "2..*"),
            "expected a `2..*` bounds badge; got {:?}",
            prop.characteristics
        );
    }

    #[test]
    fn class_card_shows_deprecated_badge() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // A class or slot marked `deprecated:` carries its note through to
        // the card data: classes expose the note on `ClassData::deprecated`,
        // slots surface a "Deprecated" characteristic badge alongside the
        // note on `SlotData::deprecated`. An undeprecated element carries
        // neither.
        let mut schema = SchemaDefinition::new("lifecycle");
        let mut legacy = ClassDefinition::new("LegacyPerson");
        legacy.deprecated = Some("use Person instead".to_string());
        schema.classes.insert("LegacyPerson".to_string(), legacy);
        schema
            .classes
            .insert("Person".to_string(), ClassDefinition::new("Person"));
        let mut old_slot = SlotDefinition::new("old_name");
        old_slot.deprecated = Some("use name instead".to_string());
        schema.slots.insert("old_name".to_string(), old_slot);
        schema
            .slots
            .insert("name".to_string(), SlotDefinition::new("name"));

        let data = HtmlWriter::build_template_data(&schema);

        let legacy_card = data
            .class_data
            .iter()
            .find(|c| c.id == "LegacyPerson")
            .unwrap();
        assert_eq!(
            legacy_card.deprecated.as_deref(),
            Some("use Person instead")
        );
        let person_card = data.class_data.iter().find(|c| c.id == "Person").unwrap();
        assert!(
            person_card.deprecated.is_none(),
            "undeprecated class must carry no note"
        );

        let old_card = data.slot_data.iter().find(|s| s.id == "old_name").unwrap();
        assert!(
            old_card.characteristics.iter().any(|c| c == "Deprecated"),
            "deprecated slot must get a Deprecated badge; got {:?}",
            old_card.characteristics
        );
        assert_eq!(old_card.deprecated.as_deref(), Some("use name instead"));
        let name_card = data.slot_data.iter().find(|s| s.id == "name").unwrap();
        assert!(
            !name_card.characteristics.iter().any(|c| c == "Deprecated"),
            "undeprecated slot must not get a Deprecated badge"
        );
    }

    #[test]
    fn class_card_shows_aliases_and_see_also() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // A class or slot with `aliases:` carries them through verbatim as
        // the comma-joined "Aliases" row, and `see_also:` URIorCURIEs
        // become CURIE-expanded `ExternalLink`s for the "See also" row (a
        // declared prefix becomes an `href`; an absolute IRI is its own
        // href). An element with neither carries empty lists, so no row
        // renders.
        let mut schema = SchemaDefinition::new("editorial");
        schema
            .prefixes
            .insert("schema".to_string(), "http://schema.org/".to_string());

        let mut person = ClassDefinition::new("Person");
        person.aliases = vec!["Human".to_string(), "Individual".to_string()];
        person.see_also = vec![
            "schema:Person".to_string(),
            "https://example.org/person".to_string(),
        ];
        schema.classes.insert("Person".to_string(), person);
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));

        let mut named = SlotDefinition::new("full_name");
        named.aliases = vec!["label".to_string()];
        named.see_also = vec!["schema:name".to_string()];
        schema.slots.insert("full_name".to_string(), named);
        schema
            .slots
            .insert("plain".to_string(), SlotDefinition::new("plain"));

        let data = HtmlWriter::build_template_data(&schema);

        let person_card = data.class_data.iter().find(|c| c.id == "Person").unwrap();
        assert_eq!(person_card.aliases, vec!["Human", "Individual"]);
        assert_eq!(person_card.see_also.len(), 2);
        let schema_link = person_card
            .see_also
            .iter()
            .find(|l| l.display == "schema:Person")
            .unwrap();
        assert_eq!(
            schema_link.href.as_deref(),
            Some("http://schema.org/Person")
        );
        let absolute_link = person_card
            .see_also
            .iter()
            .find(|l| l.display == "https://example.org/person")
            .unwrap();
        assert_eq!(
            absolute_link.href.as_deref(),
            Some("https://example.org/person")
        );

        let bare_card = data.class_data.iter().find(|c| c.id == "Bare").unwrap();
        assert!(
            bare_card.aliases.is_empty() && bare_card.see_also.is_empty(),
            "a class with neither field renders no aliases/see-also row"
        );

        let named_card = data.slot_data.iter().find(|s| s.id == "full_name").unwrap();
        assert_eq!(named_card.aliases, vec!["label"]);
        assert_eq!(named_card.see_also.len(), 1);
        assert_eq!(
            named_card.see_also[0].href.as_deref(),
            Some("http://schema.org/name")
        );
        let plain_card = data.slot_data.iter().find(|s| s.id == "plain").unwrap();
        assert!(
            plain_card.aliases.is_empty() && plain_card.see_also.is_empty(),
            "a slot with neither field renders no aliases/see-also row"
        );
    }

    #[test]
    fn class_card_shows_rules() {
        use crate::linkml::{
            ClassDefinition, ClassRule, RuleConditions, SchemaDefinition, SlotCondition,
        };
        // A class's `rules:` render as a "Rules" row: each rule's title
        // and markdown description render like any other description,
        // and its pre/postconditions render as a "when … then …"
        // sentence with slot/value names as `<code>` — the human-
        // readable rendering of a conditional requirement ("an actual
        // deployment must name its environment and provider").
        let mut schema = SchemaDefinition::new("deployments");
        let mut deployment = ClassDefinition::new("Deployment");
        deployment.rules = vec![ClassRule {
            title: Some("actual deployments are located".to_string()),
            description: Some("ties status to required fields".to_string()),
            preconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        "status".to_string(),
                        SlotCondition {
                            equals_string: Some("actual".to_string()),
                            ..Default::default()
                        },
                    );
                    m
                },
            }),
            postconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert(
                        "region".to_string(),
                        SlotCondition {
                            required: true,
                            ..Default::default()
                        },
                    );
                    m
                },
            }),
        }];
        schema.classes.insert("Deployment".to_string(), deployment);
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));

        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();

        assert!(html.contains("Rules"), "expected a Rules row; got: {html}");
        assert!(
            html.contains("actual deployments are located"),
            "expected the rule title; got: {html}"
        );
        assert!(
            html.contains("ties status to required fields"),
            "expected the rendered description; got: {html}"
        );
        assert!(
            html.contains("<code>status</code>") && html.contains("<code>actual</code>"),
            "expected the precondition rendered with slot/value as code; got: {html}"
        );
        assert!(
            html.contains("<code>region</code>") && html.contains("is required"),
            "expected the postcondition rendered; got: {html}"
        );
        assert!(
            html.contains("when") && html.contains("then"),
            "expected a when…then sentence; got: {html}"
        );
    }

    #[test]
    fn class_card_renders_any_of_and_value_presence_rule_conditions() {
        use crate::linkml::{
            ClassDefinition, ClassRule, RuleConditions, SchemaDefinition, SlotCondition,
            ValuePresence,
        };
        // cuisineiq's ImageApproval shape: an `any_of` precondition (verdict
        // is approved OR rejected) and a `value_presence` postcondition
        // (approved_by must be present). Both must render as trigger and
        // consequence — not vanish, leaving a bare title.
        let mut schema = SchemaDefinition::new("approvals");
        let mut cls = ClassDefinition::new("ImageApproval");
        let alt = |val: &str| RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                "verdict".to_string(),
                SlotCondition {
                    equals_string: Some(val.to_string()),
                    ..Default::default()
                },
            )]),
        };
        cls.rules = vec![ClassRule {
            title: Some("approved or rejected images are attributed".to_string()),
            description: None,
            preconditions: Some(RuleConditions {
                slot_conditions: std::collections::BTreeMap::new(),
                any_of: vec![alt("approved"), alt("rejected")],
            }),
            postconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "approved_by".to_string(),
                    SlotCondition {
                        value_presence: Some(ValuePresence::Present),
                        ..Default::default()
                    },
                )]),
            }),
        }];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();

        // Both `any_of` alternatives render as the trigger, joined by "or".
        assert!(
            html.contains("<code>approved</code>") && html.contains("<code>rejected</code>"),
            "any_of precondition must render both alternatives; got: {html}"
        );
        assert!(
            html.contains(" or "),
            "any_of alternatives must be joined with 'or'; got: {html}"
        );
        // The `value_presence` postcondition renders its consequence.
        assert!(
            html.contains("<code>approved_by</code>") && html.contains("is present"),
            "value_presence postcondition must render; got: {html}"
        );
    }

    #[test]
    fn class_card_shows_unique_keys() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, UniqueKey};
        // A class's `unique_keys:` render as a "Unique keys" row, one entry
        // per key, listing its slot tuple as `<code>` names and its
        // optional description. A class with none renders no such row.
        let mut schema = SchemaDefinition::new("offerings");
        let mut offering = ClassDefinition::new("Offering");
        offering.unique_keys.insert(
            "service_provider_key".to_string(),
            UniqueKey {
                unique_key_slots: vec!["service_type".to_string(), "offered_by".to_string()],
                description: Some("unique per service type and provider".to_string()),
            },
        );
        schema.classes.insert("Offering".to_string(), offering);
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));

        let out = tempfile::tempdir().unwrap();
        let writer = HtmlWriter::with_options(false);
        crate::io::Writer::write(&writer, &schema, out.path()).unwrap();
        let html = std::fs::read_to_string(out.path().join("index.html")).unwrap();

        assert!(
            html.contains("Unique keys"),
            "expected a Unique keys row; got: {html}"
        );
        assert!(
            html.contains("<code class=\"mono\">service_type</code>")
                && html.contains("<code class=\"mono\">offered_by</code>"),
            "expected the key's slot tuple rendered as code; got: {html}"
        );
        assert!(
            html.contains("unique per service type and provider"),
            "expected the key description; got: {html}"
        );
    }

    #[test]
    fn slot_card_shows_examples() {
        use crate::linkml::{ClassDefinition, Example, SchemaDefinition, SlotDefinition};
        // A class or slot with `examples:` carries each `value` and its
        // optional `description` through to the card-data `examples`
        // list, ready for the "Examples" section. An element with no
        // examples carries an empty list, so no section renders.
        let mut schema = SchemaDefinition::new("editorial");

        let mut region = ClassDefinition::new("Region");
        region.examples = vec![
            Example {
                value: "us-east-1".to_string(),
                description: Some("an AWS region".to_string()),
            },
            Example {
                value: "eastus".to_string(),
                description: None,
            },
        ];
        schema.classes.insert("Region".to_string(), region);
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));

        let mut code = SlotDefinition::new("region_code");
        code.examples = vec![Example {
            value: "eu-west-2".to_string(),
            description: None,
        }];
        schema.slots.insert("region_code".to_string(), code);
        schema
            .slots
            .insert("plain".to_string(), SlotDefinition::new("plain"));

        let data = HtmlWriter::build_template_data(&schema);

        let region_card = data.class_data.iter().find(|c| c.id == "Region").unwrap();
        assert_eq!(region_card.examples.len(), 2);
        assert_eq!(region_card.examples[0].value, "us-east-1");
        assert_eq!(
            region_card.examples[0].description.as_deref(),
            Some("an AWS region")
        );
        assert_eq!(region_card.examples[1].value, "eastus");
        assert!(region_card.examples[1].description.is_none());

        let bare_card = data.class_data.iter().find(|c| c.id == "Bare").unwrap();
        assert!(
            bare_card.examples.is_empty(),
            "a class with no examples renders no Examples section"
        );

        let code_card = data
            .slot_data
            .iter()
            .find(|s| s.id == "region_code")
            .unwrap();
        assert_eq!(code_card.examples.len(), 1);
        assert_eq!(code_card.examples[0].value, "eu-west-2");
        assert!(code_card.examples[0].description.is_none());

        let plain_card = data.slot_data.iter().find(|s| s.id == "plain").unwrap();
        assert!(
            plain_card.examples.is_empty(),
            "a slot with no examples renders no Examples section"
        );
    }

    #[test]
    fn slot_card_shows_owl_characteristic_badges() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        // A slot declaring OWL relationship characteristics gets a badge
        // per set flag, and none for the unset ones.
        let mut schema = SchemaDefinition::new("characteristics");
        let mut refines = SlotDefinition::new("refines");
        refines.transitive = true;
        refines.symmetric = true;
        schema.slots.insert("refines".to_string(), refines);

        let data = HtmlWriter::build_template_data(&schema);
        let prop = data.slot_data.iter().find(|p| p.id == "refines").unwrap();
        assert!(prop.characteristics.iter().any(|c| c == "Transitive"));
        assert!(prop.characteristics.iter().any(|c| c == "Symmetric"));
        assert!(
            !prop.characteristics.iter().any(|c| c == "Reflexive"),
            "unset characteristics must not render; got {:?}",
            prop.characteristics
        );
    }

    #[test]
    fn slot_card_shows_value_bound_badges() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        // Numeric value bounds render as `≥`/`≤` badges (whole numbers
        // without a trailing `.0`), distinct from the `min..max`
        // cardinality badge.
        let mut schema = SchemaDefinition::new("bounds");
        let mut strength = SlotDefinition::new("strength");
        strength.minimum_value = Some(0.0);
        strength.maximum_value = Some(1.0);
        schema.slots.insert("strength".to_string(), strength);
        // A fractional bound keeps its decimals; a whole one drops `.0`.
        let mut ratio = SlotDefinition::new("ratio");
        ratio.minimum_value = Some(0.5);
        schema.slots.insert("ratio".to_string(), ratio);

        let data = HtmlWriter::build_template_data(&schema);
        let strength_c = &data
            .slot_data
            .iter()
            .find(|p| p.id == "strength")
            .unwrap()
            .characteristics;
        assert!(
            strength_c.iter().any(|c| c == "≥ 0"),
            "expected `≥ 0` (no trailing .0); got {strength_c:?}"
        );
        assert!(strength_c.iter().any(|c| c == "≤ 1"));
        let ratio_c = &data
            .slot_data
            .iter()
            .find(|p| p.id == "ratio")
            .unwrap()
            .characteristics;
        assert!(
            ratio_c.iter().any(|c| c == "≥ 0.5"),
            "fractional bound keeps decimals; got {ratio_c:?}"
        );
    }

    #[test]
    fn html_writer_builds_slot_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 5 slots
        assert_eq!(data.slot_refs.len(), 5);
        assert_eq!(data.slot_data.len(), 5);

        // Find hasOwner property
        let has_owner = data.slot_data.iter().find(|p| p.id == "hasOwner").unwrap();
        assert_eq!(has_owner.slot_type, "Slot");
        assert!(!has_owner.domains.is_empty());
        assert!(has_owner.range.is_some());
    }

    #[test]
    fn html_writer_builds_individual_data() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let data = HtmlWriter::build_template_data(&schema);

        // Should have 1 individual
        assert_eq!(data.individual_refs.len(), 1);
        assert_eq!(data.individual_data.len(), 1);

        let fido = &data.individual_data[0];
        assert_eq!(fido.id, "fido");
    }

    #[test]
    fn html_writer_writes_to_output_directory() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_html_writer_test");
        let _ = fs::remove_dir_all(&temp_dir);

        let result = writer.write(&schema, &temp_dir);
        assert!(result.is_ok(), "Write should succeed");

        let output_path = temp_dir.join("index.html");
        assert!(output_path.exists(), "index.html should be created");

        let html = fs::read_to_string(&output_path).expect("Failed to read output");
        assert!(
            html.contains("panschema Reference Ontology"),
            "HTML should contain title"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_roundtrip_produces_valid_html() {
        // TTL → OwlReader → IR → HtmlWriter → HTML
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_roundtrip_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Verify key elements are present
        assert!(html.contains("panschema Reference Ontology"));
        assert!(html.contains("0.2.0"));
        assert!(html.contains("class-Animal"));
        assert!(html.contains("class-Dog"));
        assert!(html.contains("slot-hasOwner"));
        assert!(html.contains("ind-fido"));

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    /// Parse `html` with `html5ever` — the same spec-conformant HTML5
    /// engine Servo/Firefox use — and return the list of parse errors it
    /// records. A real HTML5-grammar oracle: unlike a browser's forgiving
    /// silent repair, or this module's own `.contains(...)` assertions,
    /// this reports every spec violation the tree builder recovers from.
    fn html5_parse_errors(html: &str) -> Vec<String> {
        use html5ever::tendril::TendrilSink;
        use html5ever::{ParseOpts, parse_document};
        use markup5ever_rcdom::RcDom;

        let dom = parse_document(RcDom::default(), ParseOpts::default()).one(html);
        dom.errors.borrow().iter().map(|e| e.to_string()).collect()
    }

    #[test]
    fn html5_parse_errors_catches_malformed_markup() {
        // The oracle must have teeth: a document with a mis-nested tag is
        // a spec violation html5ever recovers from but records. If this
        // returned empty, the conformance check below would be vacuous.
        let errors = html5_parse_errors(
            "<!DOCTYPE html><html><head></head><body><p><div></p></div></body></html>",
        );
        assert!(
            !errors.is_empty(),
            "html5ever should record a parse error for the mis-nested <p>/<div>"
        );
    }

    #[test]
    fn rendered_html_is_spec_valid_html5() {
        // The generated documentation page parses cleanly under a real
        // HTML5-conformance parser — no mis-nesting, unclosed tags, or
        // stray markup that a forgiving browser would silently repair
        // (and that this module's own string `.contains(...)` checks
        // can't see).
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_html5_validity_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");
        let _ = fs::remove_dir_all(&temp_dir);

        let errors = html5_parse_errors(&html);
        assert!(
            errors.is_empty(),
            "generated HTML has {} HTML5 conformance error(s):\n{}",
            errors.len(),
            errors.join("\n")
        );
    }

    #[test]
    fn schema_strings_cannot_break_out_of_the_embedded_graph_json_script() {
        // Schema-provided strings flow into the graph JSON embedded in an
        // inline <script>. A `</script>` inside a description would end
        // the script element mid-JSON and execute whatever follows —
        // stored XSS in the generated docs. The serialized JSON must
        // therefore never contain a literal `<`.
        let mut schema = crate::linkml::SchemaDefinition::new("s");
        schema.id = Some("http://example.org/xss".to_string());
        let mut class = crate::linkml::ClassDefinition::new("Innocent");
        class.description = Some("</script><img src=x onerror=alert(1)><script>".to_string());
        schema.classes.insert("Innocent".to_string(), class);

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_graph_json_xss_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");
        let _ = fs::remove_dir_all(&temp_dir);

        let json_line = html
            .lines()
            .find(|l| l.contains("__PANSCHEMA_GRAPH_DATA__"))
            .expect("the embedded graph JSON assignment");
        assert!(
            !json_line.contains('<'),
            "embedded graph JSON must escape every `<` so schema content \
             cannot close the script element; got:\n{json_line}"
        );
    }

    #[test]
    fn html_writer_emits_responsive_card_grid_and_aspect_ratio_graph() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_responsive_layout_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Card grid uses `auto-fill` so it tiles at wide viewports and
        // collapses to one column when the minimum can't fit twice.
        assert!(
            html.contains("repeat(auto-fill, minmax(380px, 1fr))"),
            "responsive card grid template missing from rendered HTML"
        );
        // Graph container uses aspect-ratio instead of a fixed height
        // so it scales with the available content area. Default is 16:8
        // — fits a laptop screen plus browser chrome + OS task bar.
        // The ratio is set via a `--graph-aspect` custom property on the
        // container (inline) so the stylesheet stays valid CSS for IDE
        // linters, and the stylesheet reads from `var(...)` with the same
        // 16/8 fallback.
        assert!(
            html.contains("--graph-aspect: 16 / 8"),
            "graph container --graph-aspect inline custom property missing"
        );
        assert!(
            html.contains("aspect-ratio: var(--graph-aspect, 16 / 8)"),
            "graph container aspect-ratio CSS rule missing from rendered HTML"
        );
        // Old fixed-height rule must be gone.
        assert!(
            !html.contains("height: 500px"),
            "stale `height: 500px` rule still present in graph container CSS"
        );
        // `.content-area`'s hard max-width cap must be gone so the page
        // can expand fluidly with the viewport.
        assert!(
            !html.contains("max-width: var(--content-max-width)"),
            "content-area max-width cap still constrains the layout"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_with_graph_aspect_overrides_the_default() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();
        let writer = HtmlWriter::new().with_graph_aspect(4, 3);
        let temp_dir = std::env::temp_dir().join("panschema_aspect_override_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("Write failed");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");
        assert!(
            html.contains("--graph-aspect: 4 / 3"),
            "expected overridden 4:3 aspect ratio in inline custom property"
        );
        // The stylesheet keeps `var(--graph-aspect, 16 / 8)` as a fallback
        // regardless of override (so the default applies if the inline
        // attribute is somehow stripped). The override is on the
        // container's inline style, asserted above.
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn parse_graph_aspect_accepts_valid_ratios() {
        assert_eq!(parse_graph_aspect("16:9").unwrap(), (16, 9));
        assert_eq!(parse_graph_aspect("16:8").unwrap(), (16, 8));
        assert_eq!(parse_graph_aspect("4:3").unwrap(), (4, 3));
        // Whitespace tolerance.
        assert_eq!(parse_graph_aspect(" 21 : 9 ").unwrap(), (21, 9));
        // Upper-bound boundary: the sanity cap is `<= 9999`, so 9999
        // itself must round-trip on both sides.
        assert_eq!(parse_graph_aspect("9999:1").unwrap(), (9999, 1));
        assert_eq!(parse_graph_aspect("1:9999").unwrap(), (1, 9999));
        assert_eq!(parse_graph_aspect("9999:9999").unwrap(), (9999, 9999));
    }

    #[test]
    fn parse_graph_aspect_rejects_malformed_input() {
        assert!(parse_graph_aspect("16").is_err(), "missing colon");
        assert!(parse_graph_aspect("16x9").is_err(), "wrong separator");
        assert!(parse_graph_aspect("16:0").is_err(), "zero height");
        assert!(parse_graph_aspect("0:9").is_err(), "zero width");
        assert!(parse_graph_aspect("a:b").is_err(), "non-numeric");
        assert!(parse_graph_aspect("10000:1").is_err(), "exceeds sanity cap");
        assert!(
            parse_graph_aspect("1:10000").is_err(),
            "exceeds sanity cap on height side"
        );
    }

    #[test]
    fn html_writer_includes_schema_graph_sidebar_with_counts() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_sidebar_graph_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Verify Schema Graph link is in sidebar
        assert!(
            html.contains("href=\"#graph-visualization\""),
            "Sidebar should contain Schema Graph link"
        );
        assert!(
            html.contains("Schema Graph"),
            "Sidebar should contain 'Schema Graph' text"
        );

        // Verify the badge contains node/edge counts (format: "X / Y")
        // Reference ontology has 5 classes + 4 slots + 1 individual = nodes
        // and corresponding edges for subclass relationships, domain/range, etc.
        assert!(
            html.contains("<span class=\"badge\">"),
            "Sidebar should contain badge with counts"
        );

        // Schema Graph link should appear between Metadata and Namespaces
        let metadata_pos = html
            .find("href=\"#metadata\"")
            .expect("Metadata link not found");
        let graph_pos = html
            .find("href=\"#graph-visualization\"")
            .expect("Graph link not found");
        let namespaces_pos = html
            .find("href=\"#namespaces\"")
            .expect("Namespaces link not found");

        assert!(
            metadata_pos < graph_pos,
            "Schema Graph should appear after Metadata"
        );
        assert!(
            graph_pos < namespaces_pos,
            "Schema Graph should appear before Namespaces"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn html_writer_without_graph_excludes_sidebar_link() {
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::with_options(false); // No graph
        let temp_dir = std::env::temp_dir().join("panschema_sidebar_no_graph_test");
        let _ = fs::remove_dir_all(&temp_dir);

        writer.write(&schema, &temp_dir).expect("Write failed");

        let html = fs::read_to_string(temp_dir.join("index.html")).expect("Failed to read");

        // Schema Graph link should NOT be present when graph is disabled
        assert!(
            !html.contains("href=\"#graph-visualization\""),
            "Sidebar should not contain Schema Graph link when graph is disabled"
        );

        // Cleanup
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn class_data_surfaces_mappings_with_expanded_iris() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());
        let mut act = ClassDefinition::new("Act");
        act.exact_mappings = vec!["cito:supports".into()];
        act.close_mappings = vec!["http://example.org/already-absolute".into()];
        act.related_mappings = vec!["unknown:Foo".into()];
        schema.classes.insert("Act".to_string(), act);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Act").unwrap();

        assert_eq!(card.mappings.len(), 3);
        let exact = card.mappings.iter().find(|m| m.kind == "exact").unwrap();
        assert_eq!(exact.display, "cito:supports");
        assert_eq!(
            exact.href.as_deref(),
            Some("http://purl.org/spar/cito/supports")
        );
        let close = card.mappings.iter().find(|m| m.kind == "close").unwrap();
        assert_eq!(
            close.href.as_deref(),
            Some("http://example.org/already-absolute"),
            "absolute URL should pass through"
        );
        let related = card.mappings.iter().find(|m| m.kind == "related").unwrap();
        assert!(
            related.href.is_none(),
            "unresolved prefix should leave href None for template fallback"
        );
    }

    #[test]
    fn slot_data_surfaces_mappings_with_expanded_iris() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());
        let mut supports = SlotDefinition::new("supports");
        supports.exact_mappings = vec!["cito:supports".into()];
        schema.slots.insert("supports".to_string(), supports);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.slot_data.iter().find(|p| p.id == "supports").unwrap();

        assert_eq!(card.mappings.len(), 1);
        assert_eq!(card.mappings[0].kind, "exact");
        assert_eq!(card.mappings[0].display, "cito:supports");
        assert_eq!(
            card.mappings[0].href.as_deref(),
            Some("http://purl.org/spar/cito/supports")
        );
    }

    #[test]
    fn class_data_expands_class_uri_to_iri_href() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cco".to_string(), "http://example.org/cco/".to_string());

        let mut grounded = ClassDefinition::new("Grounded");
        grounded.class_uri = Some("cco:ont00000005".to_string());
        schema.classes.insert("Grounded".to_string(), grounded);

        // No class_uri, no default_prefix — bare name has nowhere to resolve.
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));

        // Unknown prefix.
        let mut orphan = ClassDefinition::new("Orphan");
        orphan.class_uri = Some("unknown:Foo".to_string());
        schema.classes.insert("Orphan".to_string(), orphan);

        let data = HtmlWriter::build_template_data(&schema);
        let grounded_card = data.class_data.iter().find(|c| c.id == "Grounded").unwrap();
        assert_eq!(
            grounded_card.iri_href.as_deref(),
            Some("http://example.org/cco/ont00000005")
        );
        let bare_card = data.class_data.iter().find(|c| c.id == "Bare").unwrap();
        assert!(
            bare_card.iri_href.is_none(),
            "no class_uri AND no default_prefix → no hyperlink target"
        );
        let orphan_card = data.class_data.iter().find(|c| c.id == "Orphan").unwrap();
        assert!(
            orphan_card.iri_href.is_none(),
            "unresolved prefix → template falls back to plain text"
        );
    }

    #[test]
    fn class_data_falls_back_to_default_prefix_expansion_for_bare_classes() {
        // The common LinkML schema pattern: no explicit class_uri,
        // schema-local classes resolve via default_prefix. Without this
        // fallback the copy-IRI button on the rendered card would copy
        // the bare class name instead of a usable IRI.
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("scimantic");
        schema.prefixes.insert(
            "scimantic".to_string(),
            "https://w3id.org/scimantic/".to_string(),
        );
        schema.default_prefix = Some("scimantic".to_string());
        schema
            .classes
            .insert("Act".to_string(), ClassDefinition::new("Act"));

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Act").unwrap();
        assert_eq!(
            card.iri_href.as_deref(),
            Some("https://w3id.org/scimantic/Act")
        );
    }

    #[test]
    fn class_data_threads_external_subclass_of_with_expanded_iri() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("scimantic");
        schema
            .prefixes
            .insert("cco".to_string(), "http://example.org/cco/".to_string());

        let mut grounded = ClassDefinition::new("Act");
        grounded.subclass_of = Some("cco:ont00000005".to_string());
        schema.classes.insert("Act".to_string(), grounded);

        let mut unknown = ClassDefinition::new("Orphan");
        unknown.subclass_of = Some("unknown:NotDeclared".to_string());
        schema.classes.insert("Orphan".to_string(), unknown);

        let data = HtmlWriter::build_template_data(&schema);
        let act = data.class_data.iter().find(|c| c.id == "Act").unwrap();
        assert_eq!(act.external_superclasses.len(), 1);
        assert_eq!(act.external_superclasses[0].display, "cco:ont00000005");
        assert_eq!(
            act.external_superclasses[0].href.as_deref(),
            Some("http://example.org/cco/ont00000005")
        );
        let orphan = data.class_data.iter().find(|c| c.id == "Orphan").unwrap();
        assert!(
            orphan.external_superclasses[0].href.is_none(),
            "undeclared prefix falls through to plain-text rendering"
        );
    }

    #[test]
    fn class_data_carries_upstream_labels_when_store_has_them() {
        use crate::labels::LabelStore;
        use crate::linkml::{ClassDefinition, SchemaDefinition};

        let cache_dir = std::env::temp_dir().join("panschema_html_label_test");
        let _ = std::fs::remove_dir_all(&cache_dir);
        let mut store = LabelStore::open(&cache_dir).unwrap();
        store
            .insert_source(
                "https://example.org/cco.ttl",
                std::collections::BTreeMap::from([
                    (
                        "http://example.org/cco/ont00000958".to_string(),
                        crate::labels::TermInfo {
                            label: Some("Process".to_string()),
                            definitions: vec![
                                "A series of events that unfold over time.".to_string(),
                            ],
                        },
                    ),
                    (
                        "http://purl.org/spar/cito/supports".to_string(),
                        crate::labels::TermInfo {
                            label: Some("supports".to_string()),
                            definitions: Vec::new(),
                        },
                    ),
                ]),
            )
            .unwrap();

        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cco".to_string(), "http://example.org/cco/".to_string());
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());

        let mut act = ClassDefinition::new("Act");
        act.subclass_of = Some("cco:ont00000958".to_string());
        act.exact_mappings = vec!["cito:supports".to_string()];
        // This mapping's IRI is not in the store — label stays None.
        act.close_mappings = vec!["cco:ont99999999".to_string()];
        schema.classes.insert("Act".to_string(), act);

        let data = HtmlWriter::build_template_data_with_labels(&schema, Some(&store));
        let card = data.class_data.iter().find(|c| c.id == "Act").unwrap();

        assert_eq!(
            card.external_superclasses[0].label.as_deref(),
            Some("Process")
        );
        let exact = card.mappings.iter().find(|m| m.kind == "exact").unwrap();
        assert_eq!(exact.label.as_deref(), Some("supports"));
        let close = card.mappings.iter().find(|m| m.kind == "close").unwrap();
        assert!(close.label.is_none(), "uncached IRI renders unlabeled");

        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[test]
    fn tooltip_carries_identity_line_and_definition_when_present() {
        let with_definition = ExternalLink {
            display: "cco:ont00000958".to_string(),
            href: Some("https://example.org/cco/ont00000958".to_string()),
            label: Some("Process".to_string()),
            definitions: vec!["A series of events.".to_string()],
        };
        assert_eq!(
            with_definition.tooltip(),
            "cco:ont00000958 = https://example.org/cco/ont00000958\n\nA series of events."
        );

        // Multiple annotations each get their own paragraph.
        let multi = ExternalLink {
            display: "cito:disputes".to_string(),
            href: Some("http://purl.org/spar/cito/disputes".to_string()),
            label: Some("disputes".to_string()),
            definitions: vec![
                "The citing entity disputes the cited entity.".to_string(),
                "Example: We doubt that Galileo is right.".to_string(),
            ],
        };
        assert_eq!(
            multi.tooltip(),
            "cito:disputes = http://purl.org/spar/cito/disputes\n\nThe citing entity disputes the cited entity.\n\nExample: We doubt that Galileo is right."
        );

        let without_definition = ExternalLink {
            display: "cco:ont00000958".to_string(),
            href: Some("https://example.org/cco/ont00000958".to_string()),
            label: None,
            definitions: Vec::new(),
        };
        assert_eq!(
            without_definition.tooltip(),
            "cco:ont00000958 = https://example.org/cco/ont00000958"
        );

        let mapping = Mapping {
            kind: "exact",
            display: "cito:supports".to_string(),
            href: Some("http://purl.org/spar/cito/supports".to_string()),
            label: Some("supports".to_string()),
            definitions: vec!["One claim bears positively on another.".to_string()],
        };
        assert_eq!(
            mapping.tooltip(),
            "cito:supports = http://purl.org/spar/cito/supports\n\nOne claim bears positively on another."
        );
    }

    #[test]
    fn class_data_labels_are_none_without_a_store() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cco".to_string(), "http://example.org/cco/".to_string());
        let mut act = ClassDefinition::new("Act");
        act.subclass_of = Some("cco:ont00000958".to_string());
        schema.classes.insert("Act".to_string(), act);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.class_data.iter().find(|c| c.id == "Act").unwrap();
        assert!(card.external_superclasses[0].label.is_none());
    }

    #[test]
    fn class_data_threads_is_abstract_from_class_definition() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");

        let mut foundation = ClassDefinition::new("Foundation");
        foundation.r#abstract = true;
        schema.classes.insert("Foundation".to_string(), foundation);

        schema
            .classes
            .insert("Concrete".to_string(), ClassDefinition::new("Concrete"));

        let data = HtmlWriter::build_template_data(&schema);
        let foundation_card = data
            .class_data
            .iter()
            .find(|c| c.id == "Foundation")
            .unwrap();
        let concrete_card = data.class_data.iter().find(|c| c.id == "Concrete").unwrap();
        assert!(foundation_card.is_abstract);
        assert!(!concrete_card.is_abstract);
    }

    #[test]
    fn slot_data_expands_slot_uri_to_iri_href() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());
        let mut supports = SlotDefinition::new("supports");
        supports.slot_uri = Some("cito:supports".to_string());
        schema.slots.insert("supports".to_string(), supports);

        let data = HtmlWriter::build_template_data(&schema);
        let card = data.slot_data.iter().find(|p| p.id == "supports").unwrap();
        assert_eq!(
            card.iri_href.as_deref(),
            Some("http://purl.org/spar/cito/supports")
        );
    }
}
