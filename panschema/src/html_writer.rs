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
    /// pre/postconditions (e.g. "when `status` has value `actual`, then `region`
    /// is required"). `None` when the rule has neither — a
    /// title/description-only entry.
    pub summary: Option<String>,
    /// Space-separated graph node ids this rule touches (`class:<C>` plus a
    /// `slot:<s>` per participant slot), for `data-participants` — the graph
    /// highlights these nodes when the rule entry is hovered.
    pub participants: String,
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
    /// The rules that key on this value, one pointer per class: choosing
    /// the value entails what those rules require. The canonical rule
    /// text stays on the class card; the pointer links there and carries
    /// the rules' hover participants.
    pub rule_pointers: Vec<ValueRulePointer>,
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
    /// Class rules that reference this slot (on either side), each naming
    /// the class and carrying the rendered rule summary — the slot card's
    /// "Governed by" section. Empty when no rule names the slot.
    pub governing_rule_groups: Vec<GoverningRuleGroup>,
    /// Name each rule group's class. `false` when every governing rule
    /// comes from the slot's sole domain — the label would only repeat
    /// the Domain row; `true` whenever the class disambiguates (several
    /// governing classes, or a governing class among several domains).
    pub show_rule_group_labels: bool,
}

/// One class's rules governing a slot, for the slot card's Rules
/// section. The entries are the same [`RuleInClass`] blocks the class
/// card renders — one struct, one builder, one template partial, so
/// the two cards cannot drift apart. The group label renders only when
/// it says something the card's Domain row does not — see
/// `show_rule_group_labels`.
#[derive(Debug, Clone)]
pub struct GoverningRuleGroup {
    pub class: EntityRef,
    pub rules: Vec<RuleInClass>,
}

/// One class's rules keying on a permissible value, for the enum
/// card's value rows: how many test the value to fire (`triggers`) and
/// how many require it of a record (`governed`), with the union of
/// those rules' hover participants.
#[derive(Debug, Clone)]
pub struct ValueRulePointer {
    pub class: EntityRef,
    pub triggers: usize,
    pub governed: usize,
    pub participants: String,
}

impl ValueRulePointer {
    /// The counts as prose — "triggers 2 rules, required by 1 rule" —
    /// with each side elided at zero and pluralized on its own count.
    pub fn phrase(&self) -> String {
        let side =
            |verb: &str, n: usize| format!("{verb} {n} rule{}", if n == 1 { "" } else { "s" });
        match (self.triggers, self.governed) {
            (0, g) => side("required by", g),
            (t, 0) => side("triggers", t),
            (t, g) => format!("{}, {}", side("triggers", t), side("required by", g)),
        }
    }
}

/// A resolved property value for rendering individual cards.
#[derive(Debug, Clone)]
pub struct PropertyValueData {
    pub property_label: String,
    pub property_ref: Option<EntityRef>,
    pub value: String,
    /// When the value is a reference to another individual, links the
    /// value text to that individual's card.
    pub value_ref: Option<EntityRef>,
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

/// One curated A-box as the page renders it: its label, its individuals,
/// and its own viz payload.
struct InstanceDatasetView<'a> {
    name: &'a str,
    /// Prefix for this dataset's element ids and in-page anchors. Empty for a
    /// lone dataset, so its published `#ind-<id>` links keep working; with
    /// several, a preview that shares records with the worked example would
    /// otherwise duplicate ids and cross-link into a hidden panel.
    anchor_prefix: String,
    /// Whether this is the dataset the page opens on.
    is_default: bool,
    /// The dataset's own metadata: the container's scalar values (a data
    /// file's top-level `title:` / `description:`), in file order.
    metadata: &'a [(String, String)],
    /// The name as a JSON string literal, for the payload array.
    name_json: &'a str,
    provenance: Option<&'a str>,
    individuals: &'a [EntityRef],
    individual_data: &'a [IndividualData],
    /// `None` when the graph viz is disabled or this A-box has no nodes.
    graph_json: Option<&'a str>,
    node_count: usize,
    edge_count: usize,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    title: &'a str,
    /// Cache stamp for the viz asset URLs (see `wasm_files::viz_stamp`).
    viz_asset_stamp: &'a str,
    /// The brand link's text: the site's identity, not the page's —
    /// identical across every page of one published site.
    site_title: &'a str,
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
    namespaces: &'a [Namespace],
    /// Empty slice for class cards that don't have slots yet
    /// Graph data JSON for visualization (None = no graph)
    graph_json: Option<&'a str>,
    /// The curated A-boxes rendered in the Instance Graph section, in
    /// declaration order. Empty when the schema declares no individuals;
    /// the first is the one shown before the reader picks another.
    instance_datasets: &'a [InstanceDatasetView<'a>],
    /// The default dataset's node/edge counts, reported by both the sidebar
    /// entry and the section heading. Every graph count reads nodes/edges.
    instance_node_count: usize,
    instance_edge_count: usize,
    /// Whether any A-box is on the page, gating the sidebar entry.
    has_instances: bool,
    /// How many datasets, for the sidebar's singular/plural label.
    instance_dataset_count: usize,
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
    /// Pages of the published site for the header nav; empty hides it.
    page_links: &'a [PageLink],
    /// URL the header brand link targets. `"./"` for single-version
    /// output (page sits at the deploy root). `panschema publish`
    /// supplies this explicitly from the manifest's
    /// `[publishing].site_root_url` (default `"../current/"`).
    site_root_href: &'a str,
    /// Page composition: lead with the instance section.
    instances_first: bool,
    /// Page composition: render the schema reference sections.
    show_schema_sections: bool,
}

/// One entry in the header's page nav: a page of the published site,
/// labeled by the schema it documents. `href` is relative to the page
/// the nav renders on; every page of a site sits at one depth per
/// page kind, so the value holds for each of its versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLink {
    /// The name the site knows the page's schema by: the repo's own
    /// schema name, or the dependency's manifest key — which also
    /// names the page's directory by default.
    pub label: String,
    /// Relative URL from the rendering page's version directory.
    pub href: String,
    /// This entry is the page being rendered; drawn as a marker, not a
    /// link.
    pub active: bool,
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
/// Which half of a composed page leads. One shared type for the
/// manifest's `html_page_layout`, the publish spec's `layout`, and the
/// writer, so the accepted spellings and the default live in one place
/// and a bad value fails at parse wherever it is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageLayout {
    #[default]
    SchemaFirst,
    InstancesFirst,
}

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
    /// Lead the page with the instance section instead of the schema
    /// reference. Off by default — today's page order.
    pub instances_first: bool,
    /// Render the schema reference sections (schema graph, namespaces,
    /// class/slot/enumeration/type cards). On by default; a page built
    /// around its data alone turns it off.
    pub schema_sections: bool,
    /// Optional multi-version cohort context. Set by `panschema publish`;
    /// `None` for the single-version `panschema generate` path. When
    /// present, the rendered page gains a version dropdown in the header
    /// and a banner when `viewing` differs from `current` or matches `edge`.
    pub version_context: Option<VersionContext>,
    /// The published site's pages, for the header's page nav. Set by
    /// `panschema publish` when the site has more than one page; empty
    /// renders no nav — the single-page and `generate` default.
    pub page_links: Vec<PageLink>,
    /// Override for the header brand link target. `None` means use the
    /// per-flow default: `"./"` for single-version output (page sits at
    /// the deploy root) — `panschema publish` always sets this explicitly
    /// from the manifest's `site_root_url`.
    pub site_root_href: Option<String>,
    /// Override for the header brand link text. `None` falls back to
    /// the schema's title — `panschema publish` sets it from the
    /// manifest's `site_title` so every page of one site carries the
    /// same identity.
    pub site_title: Option<String>,
    /// Upstream label cache. `None` renders external references as
    /// CURIEs (the historical behavior); the CLI generate path wires
    /// a populated store so they render as upstream labels.
    pub label_store: Option<crate::labels::LabelStore>,
    /// Curated A-boxes to render in the Instance Graph section, in
    /// declaration order. Empty falls back to the OWL worked-example
    /// individuals embedded in the schema. More than one renders a
    /// selector; the first is shown by default.
    pub instance_datasets: Vec<InstanceDataset>,
}

/// One curated A-box rendered in the Instance Graph section.
///
/// A schema page carries a few of these — a small teaching preview, a full
/// worked example — and the reader switches between them in place. The
/// name labels the selector entry; it is unused when only one is declared.
#[derive(Debug, Clone)]
pub struct InstanceDataset {
    /// Selector label for this A-box.
    pub name: String,
    /// Where the data came from (the instance-data file name), shown as
    /// the provenance line. `None` for individuals embedded in the schema.
    pub provenance: Option<String>,
    /// The individuals themselves.
    pub set: crate::instances::InstanceSet,
    /// Show this dataset before the reader picks another. At most one
    /// should be marked; with none marked the first declared wins.
    pub default_selected: bool,
}

impl InstanceDataset {
    /// A dataset labelled `name`, with no provenance recorded yet.
    pub fn new(name: impl Into<String>, set: crate::instances::InstanceSet) -> Self {
        Self {
            name: name.into(),
            provenance: None,
            set,
            default_selected: false,
        }
    }

    /// Open this dataset first — the `exemplar = true` role.
    #[must_use]
    pub fn as_default(mut self) -> Self {
        self.default_selected = true;
        self
    }

    /// Name the file this A-box was read from, for the provenance line.
    #[must_use]
    pub fn with_provenance(mut self, source: impl Into<String>) -> Self {
        self.provenance = Some(source.into());
        self
    }
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
    pub const VIZ_JS: &str = include_str!(concat!(env!("OUT_DIR"), "/panschema_viz.js"));

    /// Compiled WASM binary
    pub const VIZ_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/panschema_viz_bg.wasm"));

    /// Content stamp for the viz asset URLs: an FNV-1a hash of the
    /// embedded bundle, so a page's `?v=` changes exactly when the
    /// binary shipping it does. Stable across page views — the browser
    /// caches the multi-MB wasm between visits — while a rebuilt
    /// bundle (a dev iteration included) busts the cache, which a
    /// crate-version stamp would miss.
    /// FNV-1a (64-bit) of `bytes`, as 16 lowercase hex digits. The
    /// XOR-then-multiply order is what makes it FNV-1a rather than
    /// FNV-1; a known-vector test pins that.
    pub fn fnv1a_hex(bytes: &[u8]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
    }

    pub fn viz_stamp() -> &'static str {
        use std::sync::OnceLock;
        static STAMP: OnceLock<String> = OnceLock::new();
        STAMP.get_or_init(|| fnv1a_hex(VIZ_WASM))
    }
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
            instances_first: false,
            schema_sections: true,
            version_context: None,
            page_links: Vec::new(),
            site_root_href: None,
            site_title: None,
            label_store: None,
            instance_datasets: Vec::new(),
        }
    }

    /// Create a new HTML writer with custom options
    pub fn with_options(include_graph: bool) -> Self {
        Self {
            include_graph,
            ..Self::new()
        }
    }

    /// Lead the page with the instance section instead of the schema
    /// reference.
    #[must_use]
    pub fn with_instances_first(mut self, instances_first: bool) -> Self {
        self.instances_first = instances_first;
        self
    }

    /// Render (or omit) the schema reference sections — the schema
    /// graph, namespaces, and class/slot/enumeration/type cards.
    #[must_use]
    pub fn with_schema_sections(mut self, schema_sections: bool) -> Self {
        self.schema_sections = schema_sections;
        self
    }

    /// Attach a populated upstream-label cache so external CURIEs
    /// render as human-readable labels.
    #[must_use]
    pub fn with_label_store(mut self, store: crate::labels::LabelStore) -> Self {
        self.label_store = Some(store);
        self
    }

    /// Add a named curated A-box. Several may be attached; the first is
    /// shown by default and the rest are reachable through the selector.
    #[must_use]
    pub fn with_instance_dataset(mut self, dataset: InstanceDataset) -> Self {
        self.instance_datasets.push(dataset);
        self
    }

    /// Attach a multi-version cohort context. Used by `panschema publish`
    /// to inject the dropdown + banner UX into each per-version page.
    #[must_use]
    pub fn with_version_context(mut self, ctx: VersionContext) -> Self {
        self.version_context = Some(ctx);
        self
    }

    /// Give the header a nav across the published site's pages. Used by
    /// `panschema publish` when the site publishes more than one page;
    /// the entry for the page being rendered has `active` set and
    /// renders as a marker rather than a link.
    #[must_use]
    pub fn with_page_links(mut self, links: Vec<PageLink>) -> Self {
        self.page_links = links;
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

    /// Override the header brand-link text. Consumed by
    /// `panschema publish` to carry the manifest's `site_title` onto
    /// every page of the site.
    #[must_use]
    pub fn with_site_title(mut self, title: impl Into<String>) -> Self {
        self.site_title = Some(title.into());
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
        Self::build_template_data_with_labels(schema, None, true)
    }

    /// Individual card data from the instance model: one entry per
    /// instance, IRIs from the shared minting (so the card shows the same
    /// IRI the RDF and graph exports use), scalar values as text, and
    /// references linked to the referenced individual's card.
    fn build_individual_data(
        schema: &SchemaDefinition,
        set: &crate::instances::InstanceSet,
    ) -> (Vec<EntityRef>, Vec<IndividualData>) {
        let mut refs = Vec::new();
        let mut data = Vec::new();
        for inst in &set.instances {
            let types: Vec<EntityRef> = inst
                .types
                .iter()
                .filter_map(|type_id| {
                    schema.classes.get(type_id).map(|c| EntityRef {
                        id: type_id.clone(),
                        label: c.annotations.label_or(type_id),
                    })
                })
                .collect();

            let slot_ref = |property: &str| {
                schema.slots.get(property).map(|slot| EntityRef {
                    id: property.to_string(),
                    label: slot.annotations.label_or(property),
                })
            };

            let mut property_values = Vec::new();
            for (property, value) in &inst.literals {
                let property_ref = slot_ref(property);
                property_values.push(PropertyValueData {
                    property_label: property_ref
                        .as_ref()
                        .map(|r| r.label.clone())
                        .unwrap_or_else(|| property.clone()),
                    property_ref,
                    value: value.clone(),
                    value_ref: None,
                });
            }
            for reference in &inst.references {
                let property_ref = slot_ref(&reference.property);
                let target = set.instances.iter().find(|i| i.id == reference.target);
                property_values.push(PropertyValueData {
                    property_label: property_ref
                        .as_ref()
                        .map(|r| r.label.clone())
                        .unwrap_or_else(|| reference.property.clone()),
                    property_ref,
                    value: target
                        .map(|t| t.label.clone())
                        .unwrap_or_else(|| reference.target.clone()),
                    value_ref: target.map(|t| EntityRef {
                        id: t.id.clone(),
                        label: t.label.clone(),
                    }),
                });
            }
            property_values.sort_by(|a, b| a.property_label.cmp(&b.property_label));

            refs.push(EntityRef {
                id: inst.id.clone(),
                label: inst.label.clone(),
            });
            data.push(IndividualData {
                id: inst.id.clone(),
                label: inst.label.clone(),
                iri: crate::rdf_serializers::instance_iri_string(schema, inst),
                description: inst.description.clone(),
                types,
                property_values,
            });
        }
        (refs, data)
    }

    /// Whether this composition produces a page with nothing on it: the
    /// schema reference omitted and no dataset loaded to feature.
    fn renders_empty(schema_sections: bool, datasets_loaded: usize) -> bool {
        !schema_sections && datasets_loaded == 0
    }

    /// Build template data, rendering upstream labels for external
    /// references when a populated [`crate::labels::LabelStore`] is
    /// supplied.
    fn build_template_data_with_labels(
        schema: &SchemaDefinition,
        labels: Option<&crate::labels::LabelStore>,
        schema_sections: bool,
    ) -> TemplateData {
        let iri = schema.id.clone().unwrap_or_else(|| schema.name.clone());
        let title = schema.title.clone().unwrap_or_else(|| schema.name.clone());

        // The namespace table renders on every composition — a data-only
        // page still expands its instance cards' CURIEs through it.
        let namespaces = build_namespaces(schema, &iri);

        // A page without the schema reference renders none of the card
        // sections, so their (quadratic, per-entity) builds are skipped.
        if !schema_sections {
            return TemplateData {
                title,
                iri,
                version: schema.version.clone(),
                comment: schema.description.clone(),
                namespaces,
                class_refs: Vec::new(),
                class_data: Vec::new(),
                class_tree: Vec::new(),
                slot_refs: Vec::new(),
                slot_data: Vec::new(),
                enum_refs: Vec::new(),
                enum_data: Vec::new(),
                type_refs: Vec::new(),
                type_data: Vec::new(),
            };
        }

        // Build class data
        let mut class_refs = Vec::new();
        let mut class_data_list = Vec::new();

        // Sort classes by name for consistent ordering
        let mut sorted_classes: Vec<_> = schema.classes.iter().collect();
        sorted_classes.sort_by(|a, b| {
            let label_a = a.1.annotations.label_or_ref(a.0);
            let label_b = b.1.annotations.label_or_ref(b.0);
            label_a.cmp(label_b)
        });

        // Each class's rule blocks are built exactly once, here, and
        // distributed to every participant slot's group — the slot loop
        // below only collects. Groups therefore arrive in the class
        // cards' order.
        let mut rules_by_slot: std::collections::BTreeMap<String, Vec<GoverningRuleGroup>> =
            std::collections::BTreeMap::new();
        // Per (enum, permissible-value key): the classes whose rules key
        // on that value, accumulated while each class's rules are walked
        // once.
        let mut value_rule_pointers: std::collections::BTreeMap<
            (String, String),
            Vec<ValuePointerDraft>,
        > = std::collections::BTreeMap::new();

        for (class_id, class_def) in &sorted_classes {
            let label = class_def.annotations.label_or(class_id);

            // One inheritance walk per class serves the rule blocks, the
            // enum-value pointers, and the slot-card rows below.
            let resolved =
                crate::linkml_resolve::resolve_effective_slots_with_provenance(class_def, schema);
            let rule_blocks = build_rule_blocks(class_id, class_def, &resolved, schema);
            build_value_rule_pointers(
                class_id,
                &label,
                class_def,
                &resolved,
                schema,
                &mut value_rule_pointers,
            );
            for (block, participants) in &rule_blocks {
                for slot in participants.all_slots() {
                    let groups = rules_by_slot.entry(slot.to_string()).or_default();
                    if groups.last().is_none_or(|g| g.class.id != **class_id) {
                        groups.push(GoverningRuleGroup {
                            class: EntityRef {
                                id: (*class_id).clone(),
                                label: label.clone(),
                            },
                            rules: Vec::new(),
                        });
                    }
                    groups
                        .last_mut()
                        .expect("group pushed above")
                        .rules
                        .push(block.clone());
                }
            }

            class_refs.push(EntityRef {
                id: (*class_id).clone(),
                label: label.clone(),
            });

            // Find superclass
            let superclass = class_def.is_a.as_ref().and_then(|parent_id| {
                schema.classes.get(parent_id).map(|parent| {
                    let parent_label = parent.annotations.label_or(parent_id);
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
                    let sub_label = sub_def.annotations.label_or(sub_id);
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
                        let mixin_label = mixin_def.annotations.label_or(mixin_id);
                        EntityRef {
                            id: mixin_id.clone(),
                            label: mixin_label,
                        }
                    })
                })
                .collect();

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
                rules: rule_blocks.into_iter().map(|(block, _)| block).collect(),
                unique_keys: build_unique_keys(&class_def.unique_keys, schema),
            });
        }

        // Build property (slot) data
        let mut slot_refs = Vec::new();
        let mut slot_data_list = Vec::new();

        // Sort slots by label for consistent ordering
        let mut sorted_slots: Vec<_> = schema.slots.iter().collect();
        sorted_slots.sort_by(|a, b| {
            let label_a = a.1.annotations.label_or_ref(a.0);
            let label_b = b.1.annotations.label_or_ref(b.0);
            label_a.cmp(label_b)
        });

        for (slot_id, slot_def) in &sorted_slots {
            let label = slot_def.annotations.label_or(slot_id);

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
                            let domain_label = c.annotations.label_or(&domain_id);
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
                    let range_label = c.annotations.label_or(range_id);
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
                characteristics.push(format!(
                    "Inverse of: {}",
                    slot_display_label(schema, inverse_id)
                ));
            }
            if let Some(parent_id) = &slot_def.is_a {
                characteristics.push(format!(
                    "Specializes: {}",
                    slot_display_label(schema, parent_id)
                ));
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

            let governing_rule_groups = rules_by_slot.remove(*slot_id).unwrap_or_default();
            // The group label earns its place only when it says something
            // the Domain row does not: labels hide only when every rule
            // comes from the slot's sole domain. A governing class need
            // not be a domain at all — a subclass ruling an inherited
            // slot, an explicit `domain:` — so this checks every group.
            let show_rule_group_labels = !governing_rule_groups.is_empty()
                && !(domains.len() == 1
                    && governing_rule_groups
                        .iter()
                        .all(|g| g.class.id == domains[0].id));

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
                governing_rule_groups,
                show_rule_group_labels,
            });
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
                    rule_pointers: value_rule_pointers
                        .remove(&(enum_id.clone(), text.clone()))
                        .unwrap_or_default()
                        .into_iter()
                        .map(|draft| ValueRulePointer {
                            class: draft.class,
                            triggers: draft.triggers,
                            governed: draft.governed,
                            participants: draft
                                .participant_ids
                                .into_iter()
                                .collect::<Vec<_>>()
                                .join(" "),
                        })
                        .collect(),
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
}

impl HtmlWriter {
    /// The datasets to render. With none attached, the schema's own embedded
    /// OWL individuals are the subject.
    fn effective_datasets(&self, schema: &SchemaDefinition) -> Vec<InstanceDataset> {
        if self.instance_datasets.is_empty() {
            return vec![InstanceDataset {
                name: String::new(),
                provenance: None,
                set: crate::instances::InstanceSet::from_owl_annotations(schema),
                default_selected: true,
            }];
        }
        self.instance_datasets.clone()
    }
}

impl Writer for HtmlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        // Create output directory if it doesn't exist
        fs::create_dir_all(output).map_err(IoError::Io)?;

        let datasets = self.effective_datasets(schema);
        let data = Self::build_template_data_with_labels(
            schema,
            self.label_store.as_ref(),
            self.schema_sections,
        );

        // Generate graph JSON for visualization — only when a section
        // will carry it: the schema graph lives in the schema reference,
        // so a page without those sections never embeds it.
        let (graph_json_string, graph_node_count, graph_edge_count) =
            if self.include_graph && self.schema_sections {
                let graph_data = GraphWriter::new()
                    .schema_to_graph_with_labels(schema, self.label_store.as_ref());
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

        // Each curated A-box gets its own cards and its own viz payload,
        // rendered as a graph distinct from the schema (T-box) one. A
        // payload is emitted only for a non-empty A-box, so an
        // individual-free schema gets no instance graph. Escaped the same
        // way as the schema graph JSON (see above).
        let instance_graph = GraphWriter::new();
        let mut dataset_parts = Vec::with_capacity(datasets.len());
        for dataset in &datasets {
            let (individual_refs, individual_data) =
                Self::build_individual_data(schema, &dataset.set);
            let (json, node_count, edge_count) = if self.include_graph {
                let instance_data = instance_graph.instance_set_to_graph(schema, &dataset.set);
                if instance_data.nodes.is_empty() {
                    (None, 0, 0)
                } else {
                    let nodes = instance_data.nodes.len();
                    let edges = instance_data.edges.len();
                    let json = serde_json::to_string(&instance_data)
                        .map_err(|e| IoError::Write(e.to_string()))?
                        .replace('<', "\\u003c");
                    (Some(json), nodes, edges)
                }
            } else {
                (None, 0, 0)
            };
            let name_json = serde_json::to_string(&dataset.name)
                .map_err(|e| IoError::Write(e.to_string()))?
                .replace('<', "\\u003c");
            dataset_parts.push((
                individual_refs,
                individual_data,
                json,
                node_count,
                edge_count,
                name_json,
            ));
        }
        // A dataset with neither cards nor a payload has nothing to show.
        let dataset_views: Vec<InstanceDatasetView<'_>> = datasets
            .iter()
            .zip(&dataset_parts)
            .filter(|(_, (refs, _, json, _, _, _))| !refs.is_empty() || json.is_some())
            .map(
                |(dataset, (refs, cards, json, node_count, edge_count, name_json))| {
                    InstanceDatasetView {
                        name: &dataset.name,
                        anchor_prefix: String::new(),
                        is_default: dataset.default_selected,
                        metadata: &dataset.set.metadata,
                        name_json,
                        provenance: dataset.provenance.as_deref(),
                        individuals: refs,
                        individual_data: cards,
                        graph_json: json.as_deref(),
                        node_count: *node_count,
                        edge_count: *edge_count,
                    }
                },
            )
            .collect();

        // The one composition that renders an empty page: no schema
        // reference and no data to feature. Said out loud rather than
        // shipped silently.
        if Self::renders_empty(self.schema_sections, dataset_views.len()) {
            eprintln!(
                "warning: schema sections are off and no instance dataset loaded — the page \
                 holds only the metadata card and namespace table"
            );
        }

        let mut dataset_views = dataset_views;
        // Several datasets share one page, and their records may overlap, so
        // each gets its own anchor namespace. A lone dataset keeps the bare
        // `ind-<id>` form its deep links already use.
        if dataset_views.len() > 1 {
            for (i, view) in dataset_views.iter_mut().enumerate() {
                view.anchor_prefix = format!("d{i}-");
            }
        }
        // With nothing marked — or with the marked dataset dropped for having
        // nothing to show — the first surviving one opens.
        if !dataset_views.iter().any(|v| v.is_default)
            && let Some(first) = dataset_views.first_mut()
        {
            first.is_default = true;
        }
        let dataset_views = dataset_views;

        // The sidebar badge describes the dataset the reader sees first.
        let (instance_node_count, instance_edge_count) = dataset_views
            .iter()
            .find(|v| v.is_default)
            .map_or((0, 0), |v| (v.node_count, v.edge_count));

        let template = IndexTemplate {
            title: &data.title,
            viz_asset_stamp: wasm_files::viz_stamp(),
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
            namespaces: &data.namespaces,
            graph_json: graph_json_string.as_deref(),
            instance_datasets: &dataset_views,
            instance_node_count,
            instance_edge_count,
            has_instances: !dataset_views.is_empty(),
            instance_dataset_count: dataset_views.len(),
            graph_node_count,
            graph_edge_count,
            graph_aspect_w: self.graph_aspect.0,
            graph_aspect_h: self.graph_aspect.1,
            graph_default_layout: &self.graph_default_layout,
            version_context: self.version_context.as_ref(),
            page_links: &self.page_links,
            // `panschema generate` writes the page at the output root, so
            // `./` always resolves to the deploy root. `panschema publish`
            // sets this explicitly from the manifest's `site_root_url`.
            site_root_href: self.site_root_href.as_deref().unwrap_or("./"),
            site_title: self
                .site_title
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or(&data.title),
            instances_first: self.instances_first,
            show_schema_sections: self.schema_sections,
        };

        let html = template
            .render()
            .map_err(|e| IoError::Write(e.to_string()))?;

        let output_path = output.join("index.html");
        fs::write(&output_path, html).map_err(IoError::Io)?;

        // Copy the viz assets only when some canvas on the page imports
        // them — a composed page with neither a schema graph nor any
        // instance graph would otherwise ship megabytes of dead wasm.
        let any_viz = graph_json_string.is_some()
            || dataset_parts
                .iter()
                .any(|(_, _, json, _, _, _)| json.is_some());
        // `any_viz` is only ever set when `include_graph` allowed a
        // build, so it alone decides.
        if any_viz {
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

/// Display label for a slot referenced by *name* — its `panschema:label`
/// annotation when declared, else the raw name. Resolved through the
/// shared by-name lookup, so a reference to an attribute-declared slot
/// (an `inverse` or a slot-level `is_a` parent) shows the same label
/// that slot's own card does.
fn slot_display_label(schema: &SchemaDefinition, name: &str) -> String {
    schema.slot_display_label(name)
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
    // A marker substitutes only in prose text. `<code>` content is
    // literal by definition — a `[[Name]]` there (a regex character
    // class, a quoted value) must stay exactly as authored — and a
    // marker inside a tag (an image's alt text, a link title) must stay
    // too, since the injected anchor's quotes would terminate the
    // attribute. The renderer emits the tags, so counting them tracks
    // both regions reliably.
    let mut code_depth = 0usize;
    let mut in_tag = false;
    while let Some((before, after_open)) = remainder.split_once("[[") {
        out.push_str(before);
        code_depth += before.matches("<code").count();
        code_depth = code_depth.saturating_sub(before.matches("</code>").count());
        for c in before.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ => {}
            }
        }
        if code_depth == 0
            && !in_tag
            && let Some((name, after_close)) = after_open.split_once("]]")
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
        let label = class_def.annotations.label_or(range);
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

/// Graph node ids a rule on `class_id` touches — `class:<id>`, a
/// `slot:<s>` for each participant slot, and an `enum:<E>` for each
/// participant slot whose class-effective range (directly or through an
/// `any_of` branch) is an enumeration: the rule's condition constrains
/// that enum's value space, so the enum participates through its values
/// and lights up with the others on hover. One space-separated string
/// for the `data-participants` attribute the graph highlight-on-hover
/// reads.
fn rule_participant_ids(
    class_id: &str,
    participants: &crate::rules::RuleParticipants,
    resolved: &std::collections::BTreeMap<String, crate::linkml_resolve::ResolvedSlot>,
    schema: &SchemaDefinition,
) -> String {
    rule_participant_id_vec(class_id, participants, resolved, schema).join(" ")
}

/// The ids as a list — the form set operations consume, so callers
/// union and dedup ids without re-splitting the joined attribute
/// string.
fn rule_participant_id_vec(
    class_id: &str,
    participants: &crate::rules::RuleParticipants,
    resolved: &std::collections::BTreeMap<String, crate::linkml_resolve::ResolvedSlot>,
    schema: &SchemaDefinition,
) -> Vec<String> {
    let mut ids = vec![format!("class:{class_id}")];
    for s in participants.all_slots() {
        ids.push(format!("slot:{s}"));
    }
    for e in crate::rules::participant_enums(participants, resolved, schema) {
        ids.push(format!("enum:{e}"));
    }
    ids
}

/// Accumulator for one class's contribution to a permissible value's
/// rule pointers, finalized into [`ValueRulePointer`] once every class
/// is walked.
struct ValuePointerDraft {
    class: EntityRef,
    triggers: usize,
    governed: usize,
    participant_ids: std::collections::BTreeSet<String>,
}

/// Walk one class's rules for the enum values they key on, folding the
/// results into `out` keyed by (enum, permissible-value key). Counts
/// are per rule, not per constant — a rule naming a value on several
/// slots or in several alternatives still counts once — and a
/// postcondition alternative does not count as "required", since
/// satisfying a sibling alternative suffices. Constants resolve
/// through [`crate::rules::permitted_value_key`], the membership
/// `validate` enforces; a constant naming no value is the never-fires
/// diagnostic's business, not a pointer on a row that does not exist.
fn build_value_rule_pointers(
    class_id: &str,
    class_label: &str,
    class_def: &crate::linkml::ClassDefinition,
    resolved: &std::collections::BTreeMap<String, crate::linkml_resolve::ResolvedSlot>,
    schema: &SchemaDefinition,
    out: &mut std::collections::BTreeMap<(String, String), Vec<ValuePointerDraft>>,
) {
    for rule in &class_def.rules {
        let participants = crate::rules::rule_participants(rule);
        // Built only when the rule actually keys on a resolvable value —
        // most rules touch no enum constant and never need the ids.
        let mut participant_ids: Option<Vec<String>> = None;
        for (is_trigger, conditions) in [
            (true, rule.preconditions.as_ref()),
            (false, rule.postconditions.as_ref()),
        ] {
            let Some(conditions) = conditions else {
                continue;
            };
            // Dedup within the rule: one rule, one count per value.
            let mut keyed: std::collections::BTreeSet<(String, String)> =
                std::collections::BTreeSet::new();
            for constant in crate::rules::enum_equals_constants(conditions, resolved, schema) {
                if !is_trigger && constant.alternative {
                    continue;
                }
                let Some(enum_def) = schema.enums.get(&constant.enum_name) else {
                    continue;
                };
                let Some(key) = crate::rules::permitted_value_key(enum_def, &constant.value) else {
                    continue;
                };
                keyed.insert((constant.enum_name, key.to_string()));
            }
            for slot_key in keyed {
                let ids = participant_ids.get_or_insert_with(|| {
                    rule_participant_id_vec(class_id, &participants, resolved, schema)
                });
                let pointers = out.entry(slot_key).or_default();
                if pointers
                    .last()
                    .is_none_or(|p: &ValuePointerDraft| p.class.id != class_id)
                {
                    pointers.push(ValuePointerDraft {
                        class: EntityRef {
                            id: class_id.to_string(),
                            label: class_label.to_string(),
                        },
                        triggers: 0,
                        governed: 0,
                        participant_ids: std::collections::BTreeSet::new(),
                    });
                }
                let draft = pointers.last_mut().expect("pushed above");
                if is_trigger {
                    draft.triggers += 1;
                } else {
                    draft.governed += 1;
                }
                draft.participant_ids.extend(ids.iter().cloned());
            }
        }
    }
}

/// Build one class's rendered rule blocks, each paired with the slots
/// it names — built once per class and shared by the class card's
/// Rules section and every participant slot's card, so the two present
/// a rule identically. The description and generated summary pass
/// through the markdown pipeline (block form — the card gives each its
/// own block), so slot/value names come out as `<code>`; the title
/// renders as escaped literal text, and a blank title is treated as
/// absent. A rule with nothing renderable at all is skipped: an empty
/// block would be an invisible hover target.
fn build_rule_blocks(
    class_id: &str,
    class_def: &crate::linkml::ClassDefinition,
    resolved: &std::collections::BTreeMap<String, crate::linkml_resolve::ResolvedSlot>,
    schema: &SchemaDefinition,
) -> Vec<(RuleInClass, crate::rules::RuleParticipants)> {
    if class_def.rules.is_empty() {
        return Vec::new();
    }
    class_def
        .rules
        .iter()
        .filter_map(|rule| {
            let participants = crate::rules::rule_participants(rule);
            let block = RuleInClass {
                title: rule.title.clone().filter(|t| !t.trim().is_empty()),
                description: rule
                    .description
                    .as_deref()
                    .map(|d| render_description(d, schema)),
                summary: crate::rules::rule_summary(rule).map(|s| render_description(&s, schema)),
                participants: rule_participant_ids(class_id, &participants, resolved, schema),
            };
            (block.title.is_some() || block.description.is_some() || block.summary.is_some())
                .then_some((block, participants))
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

    /// FNV-1a-64 known vectors: the empty input is the offset basis,
    /// and "a" is the canonical published value. Pins the XOR-then-
    /// multiply order — FNV-1 (multiply-then-XOR) or a swapped operator
    /// would give a different digest for these.
    #[test]
    fn viz_stamp_hash_is_fnv1a() {
        assert_eq!(wasm_files::fnv1a_hex(b""), "cbf29ce484222325");
        assert_eq!(wasm_files::fnv1a_hex(b"a"), "af63dc4c8601ec8c");
        assert_eq!(wasm_files::fnv1a_hex(b"foobar"), "85944171f73967e8");
        // Different content, different stamp.
        assert_ne!(wasm_files::fnv1a_hex(b"a"), wasm_files::fnv1a_hex(b"b"));
    }

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
    fn xref_markers_inside_html_attributes_stay_literal() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Ab".to_string(), ClassDefinition::new("Ab"));
        let html = render_description("![see [[Ab]] here](d.png)", &schema);
        assert!(
            html.contains(r#"alt="see [[Ab]] here""#),
            "a marker inside an attribute stays literal — an injected anchor's \
             quote would terminate the attribute and corrupt the markup; got: {html}"
        );
    }

    #[test]
    fn xref_markers_inside_code_spans_stay_literal() {
        use crate::linkml::{ClassDefinition, SchemaDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema
            .classes
            .insert("Ab".to_string(), ClassDefinition::new("Ab"));
        let html = render_description("match `^[[Ab]]+$` or see [[Ab]]", &schema);
        assert!(
            html.contains("<code>^[[Ab]]+$</code>"),
            "a code span's content is literal — no link or warning is injected \
             into it; got: {html}"
        );
        assert_eq!(
            html.matches(r##"href="#class-Ab""##).count(),
            1,
            "the marker outside the span still links; got: {html}"
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
        // A real-world `ImageApproval` shape: an `any_of` precondition (verdict
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

    /// A slot specializing another (slot-level `is_a`) shows the relation
    /// on its card, labeled by the parent's display label — the same
    /// surfacing `inverse` gets.
    #[test]
    fn slot_card_shows_the_slot_it_specializes() {
        use crate::linkml::{SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("s");
        schema.slots.insert("expected_anchors".to_string(), {
            let mut s = SlotDefinition::new("expected_anchors");
            s.annotations.insert(
                "panschema:label".to_string(),
                "expected anchors".to_string(),
            );
            s
        });
        let mut citations = SlotDefinition::new("expected_citations");
        citations.is_a = Some("expected_anchors".to_string());
        schema
            .slots
            .insert("expected_citations".to_string(), citations);

        let data = HtmlWriter::build_template_data(&schema);
        let child = data
            .slot_data
            .iter()
            .find(|p| p.id == "expected_citations")
            .unwrap();
        assert!(
            child
                .characteristics
                .iter()
                .any(|c| c == "Specializes: expected anchors"),
            "the child card names its parent; got {:?}",
            child.characteristics
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
    fn slot_card_lists_the_rules_that_govern_the_slot() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // A slot named in a class rule shows that rule on its card, grouped
        // by the class whose rule it is — so a reader on the slot sees why
        // it is conditional.
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        // `image` is a slot no rule references — it must carry zero rules.
        schema
            .slots
            .insert("image".to_string(), SlotDefinition::new("image"));
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into(), "image".into()];
        cls.rules = vec![approval_rule(None)];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let approved_by = data
            .slot_data
            .iter()
            .find(|s| s.id == "approved_by")
            .expect("approved_by slot card");
        assert_eq!(
            approved_by.governing_rule_groups.len(),
            1,
            "approved_by is governed by one class's rules"
        );
        let group = &approved_by.governing_rule_groups[0];
        assert_eq!(group.class.id, "ImageApproval");
        assert_eq!(group.rules.len(), 1);
        // Participants carry the graph node ids the rule touches (class +
        // each participant slot), for highlight-on-hover.
        let parts = &group.rules[0].participants;
        assert!(
            parts.contains("class:ImageApproval")
                && parts.contains("slot:verdict")
                && parts.contains("slot:approved_by"),
            "participants should list the class and each participant slot; got: {parts}"
        );
        let summary = group.rules[0]
            .summary
            .as_deref()
            .expect("a rendered rule summary");
        assert!(
            summary.contains("approved_by") && summary.contains("present"),
            "summary should name the slot and its consequence; got: {summary}"
        );
        assert!(
            !approved_by.show_rule_group_labels,
            "every rule comes from the slot's sole domain, so the group label \
             would only repeat the Domain row"
        );

        // The trigger slot also lists the rule (it's a participant).
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        assert_eq!(
            verdict.governing_rule_groups.len(),
            1,
            "verdict triggers the rule"
        );

        // A slot no rule references carries none — pins the membership test
        // (a slot is included only when a rule actually names it).
        let image = data.slot_data.iter().find(|s| s.id == "image").unwrap();
        assert!(
            image.governing_rule_groups.is_empty(),
            "a slot no rule references must list no rules"
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

        let set = crate::instances::InstanceSet::from_owl_annotations(&schema);
        let (refs, cards) = HtmlWriter::build_individual_data(&schema, &set);

        // Should have 1 individual
        assert_eq!(refs.len(), 1);
        assert_eq!(cards.len(), 1);

        let fido = &cards[0];
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
    fn reference_values_link_to_the_referenced_individuals_card() {
        use crate::linkml::{ClassDefinition, SlotDefinition};
        // A two-record A-box with a reference: the card's value must be the
        // *referenced* record's label, linked to that record's card — a
        // wrong-target lookup would surface some other instance here.
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let mut container = ClassDefinition::new("Cellar");
        container.tree_root = true;
        for (slot, range) in [("bottles", "Bottle"), ("racks", "Rack")] {
            let mut sd = SlotDefinition::new(slot);
            sd.range = Some(range.to_string());
            sd.multivalued = true;
            container.attributes.insert(slot.to_string(), sd);
        }
        schema.classes.insert("Cellar".to_string(), container);
        for class in ["Bottle", "Rack"] {
            let mut c = ClassDefinition::new(class);
            let mut id = SlotDefinition::new("id");
            id.identifier = true;
            c.attributes.insert("id".to_string(), id);
            let mut name = SlotDefinition::new("name");
            name.range = Some("string".to_string());
            c.attributes.insert("name".to_string(), name);
            if class == "Bottle" {
                let mut stored_in = SlotDefinition::new("stored_in");
                stored_in.range = Some("Rack".to_string());
                c.attributes.insert("stored_in".to_string(), stored_in);
            }
            schema.classes.insert(class.to_string(), c);
        }
        let data: serde_norway::Value = serde_norway::from_str(
            "bottles:\n  - id: b1\n    name: Morgon\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        )
        .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);

        let (_, cards) = HtmlWriter::build_individual_data(&schema, &set);
        let bottle = cards.iter().find(|c| c.id == "b1").expect("bottle card");
        let stored_in = bottle
            .property_values
            .iter()
            .find(|pv| pv.property_label == "stored_in")
            .expect("stored_in value");
        assert_eq!(stored_in.value, "North Rack", "value is the target's label");
        assert_eq!(
            stored_in.value_ref.as_ref().map(|r| r.id.as_str()),
            Some("r1"),
            "value links to the referenced individual's card"
        );
    }

    /// Two-class schema (`Bottle` referencing `Rack`) plus the `tree_root`
    /// container the LinkML data loader keys off, the minimum shape for
    /// building distinct A-boxes.
    /// A when-verdict-approved-then-approved-by-present rule, the shape
    /// several rule-rendering tests share.
    fn approval_rule(title: Option<&str>) -> crate::linkml::ClassRule {
        use crate::linkml::{ClassRule, RuleConditions, SlotCondition, ValuePresence};
        ClassRule {
            title: title.map(String::from),
            description: None,
            preconditions: Some(RuleConditions {
                any_of: Vec::new(),
                slot_conditions: std::collections::BTreeMap::from([(
                    "verdict".to_string(),
                    SlotCondition {
                        equals_string: Some("approved".to_string()),
                        ..Default::default()
                    },
                )]),
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
        }
    }

    fn bottle_rack_schema() -> SchemaDefinition {
        use crate::linkml::{ClassDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("cellar");
        let mut container = ClassDefinition::new("Cellar");
        container.tree_root = true;
        for (slot, range) in [("bottles", "Bottle"), ("racks", "Rack")] {
            let mut sd = SlotDefinition::new(slot);
            sd.range = Some(range.to_string());
            sd.multivalued = true;
            container.attributes.insert(slot.to_string(), sd);
        }
        schema.classes.insert("Cellar".to_string(), container);
        for class in ["Bottle", "Rack"] {
            let mut c = ClassDefinition::new(class);
            let mut id = SlotDefinition::new("id");
            id.identifier = true;
            id.range = Some("string".to_string());
            c.attributes.insert("id".to_string(), id);
            let mut name = SlotDefinition::new("name");
            name.range = Some("string".to_string());
            c.attributes.insert("name".to_string(), name);
            if class == "Bottle" {
                let mut stored_in = SlotDefinition::new("stored_in");
                stored_in.range = Some("Rack".to_string());
                c.attributes.insert("stored_in".to_string(), stored_in);
            }
            schema.classes.insert(class.to_string(), c);
        }
        schema
    }

    fn instance_set_from_yaml(
        schema: &SchemaDefinition,
        yaml: &str,
    ) -> crate::instances::InstanceSet {
        let data: serde_norway::Value = serde_norway::from_str(yaml).unwrap();
        crate::instances::InstanceSet::from_linkml_data(schema, &data)
    }

    #[test]
    fn governing_rule_renders_as_one_line_with_its_title() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into()];
        cls.rules = vec![
            approval_rule(Some("approvals_are_signed")),
            approval_rule(None),
            approval_rule(Some("   ")),
        ];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let out = tempfile::tempdir().unwrap();
        HtmlWriter::with_options(false)
            .write(&schema, out.path())
            .expect("write");
        let html = fs::read_to_string(out.path().join("index.html")).expect("read");

        assert!(
            html.contains(r#"<div class="rule-title">approvals_are_signed</div>"#),
            "a titled rule renders its title line exactly as the class card does"
        );
        assert!(
            html.contains(
                r#"<div class="rule-summary"><p>when <code>verdict</code> has value <code>approved</code>"#
            ),
            "the summary is the same block the class card renders"
        );
        let rules_at = html.find("governing-rules").expect("rules section renders");
        let rules_region = &html[rules_at..];
        let rules_region =
            &rules_region[..rules_region.find("</ul>").unwrap_or(rules_region.len())];
        assert!(
            !rules_region.contains(r#"entity-link">ImageApproval"#),
            "every rule comes from the slot's sole domain, so no entry renders \
             the class — the Domain row already names it (the hover metadata \
             still carries it); got: {rules_region}"
        );
        assert!(
            !html.contains(r#"<div class="rule-title"></div>"#),
            "a blank title is treated as absent, never rendered as an empty line"
        );
    }

    #[test]
    fn a_governing_class_among_several_domains_is_named() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // Three classes carry the slot but only one governs it: the group
        // label renders, because the Domain row alone cannot say which of
        // the three the rule belongs to.
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        for name in ["AlphaReview", "BetaReview", "GammaReview"] {
            let mut cls = ClassDefinition::new(name);
            cls.slots = vec!["verdict".into(), "approved_by".into()];
            if name == "AlphaReview" {
                cls.rules = vec![approval_rule(None)];
            }
            schema.classes.insert(name.to_string(), cls);
        }
        let data = HtmlWriter::build_template_data(&schema);
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        assert_eq!(verdict.governing_rule_groups.len(), 1);
        assert!(
            verdict.show_rule_group_labels,
            "one governing class among three domains must be named"
        );
    }

    #[test]
    fn a_subclass_governing_an_inherited_slot_is_named() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // The parent lists the slot (so it is the sole domain); the
        // subclass inherits it without relisting and adds its own rule.
        // Both classes govern, one class is a domain — the labels must
        // render, or the subclass's rule reads as the parent's.
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        let mut parent = ClassDefinition::new("Approval");
        parent.slots = vec!["verdict".into(), "approved_by".into()];
        parent.rules = vec![approval_rule(None)];
        schema.classes.insert("Approval".to_string(), parent);
        let mut child = ClassDefinition::new("PriorityApproval");
        child.is_a = Some("Approval".to_string());
        child.rules = vec![approval_rule(Some("priority_reviews_are_signed"))];
        schema.classes.insert("PriorityApproval".to_string(), child);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        assert_eq!(
            verdict.governing_rule_groups.len(),
            2,
            "both the parent's and the subclass's rules govern the slot"
        );
        assert!(
            verdict.show_rule_group_labels,
            "rules from several classes need their class named even when only \
             one of them is a domain"
        );
    }

    #[test]
    fn an_any_of_enum_range_joins_the_rule_participants() {
        use crate::linkml::{ClassDefinition, EnumDefinition, SchemaDefinition, SlotDefinition};
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .enums
            .insert("Verdict".to_string(), EnumDefinition::new("Verdict"));
        let mut verdict = SlotDefinition::new("verdict");
        let mut branch = SlotDefinition::new("verdict");
        branch.range = Some("Verdict".to_string());
        verdict.any_of = vec![branch];
        schema.slots.insert("verdict".to_string(), verdict);
        let mut approved_by = SlotDefinition::new("approved_by");
        approved_by.range = Some("string".to_string());
        schema.slots.insert("approved_by".to_string(), approved_by);
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into()];
        cls.rules = vec![approval_rule(None)];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        let parts = &verdict.governing_rule_groups[0].rules[0].participants;
        assert!(
            parts.contains("enum:Verdict"),
            "an enum reached through any_of participates like a direct range; got: {parts}"
        );
        assert_eq!(
            parts.matches("enum:").count(),
            1,
            "a participant slot's non-enum range contributes no enum id; got: {parts}"
        );
    }

    #[test]
    fn a_rule_rendering_nothing_produces_no_block() {
        use crate::linkml::{ClassDefinition, ClassRule, SchemaDefinition, SlotDefinition};
        // No title, no description, and conditions the summary cannot
        // describe: there is nothing to show, so no entry renders on
        // either card — not an invisible hover target.
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into()];
        cls.rules = vec![ClassRule {
            title: None,
            description: None,
            preconditions: None,
            postconditions: None,
        }];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let class = data
            .class_data
            .iter()
            .find(|c| c.id == "ImageApproval")
            .unwrap();
        assert!(
            class.rules.is_empty(),
            "the class card renders no empty rule entry"
        );

        // A title alone is renderable content: the block stays.
        let mut schema = crate::linkml::SchemaDefinition::new("approvals");
        schema.slots.insert(
            "verdict".to_string(),
            crate::linkml::SlotDefinition::new("verdict"),
        );
        let mut cls = crate::linkml::ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into()];
        cls.rules = vec![ClassRule {
            title: Some("reviewers_sign_off".to_string()),
            description: None,
            preconditions: None,
            postconditions: None,
        }];
        schema.classes.insert("ImageApproval".to_string(), cls);
        let data = HtmlWriter::build_template_data(&schema);
        let class = data
            .class_data
            .iter()
            .find(|c| c.id == "ImageApproval")
            .unwrap();
        assert_eq!(
            class.rules.len(),
            1,
            "a title-only rule renders its title block"
        );
        assert_eq!(class.rules[0].title.as_deref(), Some("reviewers_sign_off"));
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        assert!(
            verdict.governing_rule_groups.is_empty(),
            "the slot card renders no empty group"
        );
    }

    #[test]
    fn a_permissible_value_a_rule_keys_on_points_at_the_rules_class() {
        use crate::linkml::{
            ClassDefinition, EnumDefinition, PermissibleValue, SchemaDefinition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("approvals");
        let mut kinds = EnumDefinition::new("Verdict");
        for v in ["approved", "rejected"] {
            kinds
                .permissible_values
                .insert(v.to_string(), PermissibleValue::new(v));
        }
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        verdict.range = Some("Verdict".to_string());
        schema.slots.insert("verdict".to_string(), verdict);
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into()];
        // Two rules key on `approved` (trigger side); one requires
        // `rejected` of a record (governed side).
        let mut demoting = approval_rule(None);
        demoting.postconditions = Some(crate::linkml::RuleConditions {
            any_of: Vec::new(),
            slot_conditions: std::collections::BTreeMap::from([(
                "verdict".to_string(),
                crate::linkml::SlotCondition {
                    equals_string: Some("rejected".to_string()),
                    ..Default::default()
                },
            )]),
        });
        cls.rules = vec![approval_rule(None), demoting];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict_enum = data
            .enum_data
            .iter()
            .find(|e| e.id == "Verdict")
            .expect("enum card");
        let approved = verdict_enum
            .permissible_values
            .iter()
            .find(|pv| pv.text == "approved")
            .unwrap();
        assert_eq!(
            approved.rule_pointers.len(),
            1,
            "one class's rules merge into one pointer, however many key the value"
        );
        let ptr = &approved.rule_pointers[0];
        assert_eq!(ptr.class.id, "ImageApproval");
        assert_eq!(
            (ptr.triggers, ptr.governed),
            (2, 0),
            "both rules test the value on their trigger side"
        );
        let rejected_ptr = &verdict_enum
            .permissible_values
            .iter()
            .find(|pv| pv.text == "rejected")
            .unwrap()
            .rule_pointers;
        assert_eq!(
            (rejected_ptr[0].triggers, rejected_ptr[0].governed),
            (0, 1),
            "a postcondition constant counts on the governed side"
        );
        assert!(
            ptr.participants.contains("class:ImageApproval")
                && ptr.participants.contains("enum:Verdict"),
            "the pointer carries the rule's hover participants; got: {}",
            ptr.participants
        );
        // A value no rule keys on carries no pointer at all.
        let mut kinds2 = verdict_enum
            .permissible_values
            .iter()
            .filter(|pv| pv.rule_pointers.is_empty());
        assert!(
            kinds2.next().is_none(),
            "every value of this enum is keyed by some rule"
        );

        let out = tempfile::tempdir().unwrap();
        HtmlWriter::with_options(false)
            .write(&schema, out.path())
            .expect("write");
        let html = fs::read_to_string(out.path().join("index.html")).expect("read");
        assert!(
            html.contains("triggers 2 rules on")
                && html.contains("required by 1 rule on")
                && html.contains(r#"class="entity-link">ImageApproval</a>"#),
            "the enum card renders each side's count with the class linked"
        );
    }

    #[test]
    fn value_pointers_count_rules_not_constants() {
        use crate::linkml::{
            ClassDefinition, ClassRule, EnumDefinition, PermissibleValue, RuleConditions,
            SchemaDefinition, SlotCondition, SlotDefinition,
        };
        let mut schema = SchemaDefinition::new("approvals");
        let mut kinds = EnumDefinition::new("Verdict");
        for v in ["approved", "rejected"] {
            kinds
                .permissible_values
                .insert(v.to_string(), PermissibleValue::new(v));
        }
        schema.enums.insert("Verdict".to_string(), kinds);
        for slot in ["verdict", "second_verdict"] {
            let mut def = SlotDefinition::new(slot);
            def.range = Some("Verdict".to_string());
            schema.slots.insert(slot.to_string(), def);
        }
        let equals = |slot: &str, v: &str| {
            (
                slot.to_string(),
                SlotCondition {
                    equals_string: Some(v.to_string()),
                    ..Default::default()
                },
            )
        };
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "second_verdict".into()];
        cls.rules = vec![
            // One rule testing the same value on two same-enum slots.
            ClassRule {
                title: None,
                description: None,
                preconditions: Some(RuleConditions {
                    any_of: Vec::new(),
                    slot_conditions: std::collections::BTreeMap::from([
                        equals("verdict", "approved"),
                        equals("second_verdict", "approved"),
                    ]),
                }),
                postconditions: None,
            },
            // A governed alternation: satisfying either value suffices, so
            // neither is "required by" the rule.
            ClassRule {
                title: None,
                description: None,
                preconditions: None,
                postconditions: Some(RuleConditions {
                    any_of: vec![
                        RuleConditions {
                            any_of: Vec::new(),
                            slot_conditions: std::collections::BTreeMap::from([equals(
                                "verdict", "approved",
                            )]),
                        },
                        RuleConditions {
                            any_of: Vec::new(),
                            slot_conditions: std::collections::BTreeMap::from([equals(
                                "verdict", "rejected",
                            )]),
                        },
                    ],
                    slot_conditions: std::collections::BTreeMap::new(),
                }),
            },
        ];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict_enum = data.enum_data.iter().find(|e| e.id == "Verdict").unwrap();
        let approved = verdict_enum
            .permissible_values
            .iter()
            .find(|pv| pv.text == "approved")
            .unwrap();
        assert_eq!(
            (
                approved.rule_pointers[0].triggers,
                approved.rule_pointers[0].governed
            ),
            (1, 0),
            "one rule counts once however many of its constants name the value, \
             and an alternation's values are not required"
        );
        let rejected = verdict_enum
            .permissible_values
            .iter()
            .find(|pv| pv.text == "rejected")
            .unwrap();
        assert!(
            rejected.rule_pointers.is_empty(),
            "a value only an alternation names is neither trigger nor requirement"
        );
    }

    #[test]
    fn a_text_matched_constant_points_at_its_values_row() {
        use crate::linkml::{
            ClassDefinition, EnumDefinition, PermissibleValue, SchemaDefinition, SlotDefinition,
        };
        // OWL-loaded shape: the row's map key is `approved`, its display
        // text `Approved`; a rule constant spelled like the text attaches
        // to that row — the membership `validate` enforces.
        let mut schema = SchemaDefinition::new("approvals");
        let mut kinds = EnumDefinition::new("Verdict");
        let mut pv = PermissibleValue::new("Approved");
        pv.text = "Approved".to_string();
        kinds.permissible_values.insert("approved".to_string(), pv);
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        verdict.range = Some("Verdict".to_string());
        schema.slots.insert("verdict".to_string(), verdict);
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into()];
        let mut rule = approval_rule(None);
        rule.preconditions
            .as_mut()
            .unwrap()
            .slot_conditions
            .get_mut("verdict")
            .unwrap()
            .equals_string = Some("Approved".to_string());
        cls.rules = vec![rule];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict_enum = data.enum_data.iter().find(|e| e.id == "Verdict").unwrap();
        let row = verdict_enum
            .permissible_values
            .iter()
            .find(|pv| pv.text == "approved")
            .unwrap();
        assert_eq!(
            row.rule_pointers.len(),
            1,
            "the constant resolves to the row whose text it matches"
        );
    }

    #[test]
    fn a_union_ranged_slot_rings_its_enum_but_stakes_no_value_claim() {
        use crate::linkml::{
            ClassDefinition, EnumDefinition, PermissibleValue, SchemaDefinition, SlotDefinition,
        };
        // The deliberate asymmetry, pinned: a rule on a slot whose induced
        // range is a union touching an enum participates in that enum (the
        // graph ring), but a value-level claim — a card pointer, a
        // never-fires finding — needs the certainty a union cannot give.
        let mut schema = SchemaDefinition::new("approvals");
        let mut kinds = EnumDefinition::new("Verdict");
        kinds
            .permissible_values
            .insert("approved".to_string(), PermissibleValue::new("approved"));
        schema.enums.insert("Verdict".to_string(), kinds);
        let mut verdict = SlotDefinition::new("verdict");
        let mut enum_branch = SlotDefinition::new("verdict");
        enum_branch.range = Some("Verdict".to_string());
        let mut string_branch = SlotDefinition::new("verdict");
        string_branch.range = Some("string".to_string());
        verdict.any_of = vec![enum_branch, string_branch];
        schema.slots.insert("verdict".to_string(), verdict);
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        let mut cls = ClassDefinition::new("ImageApproval");
        cls.slots = vec!["verdict".into(), "approved_by".into()];
        cls.rules = vec![approval_rule(None)];
        schema.classes.insert("ImageApproval".to_string(), cls);

        let data = HtmlWriter::build_template_data(&schema);
        let verdict_slot = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        let parts = &verdict_slot.governing_rule_groups[0].rules[0].participants;
        assert!(
            parts.contains("enum:Verdict"),
            "the union's enum participates for the ring; got: {parts}"
        );
        let verdict_enum = data.enum_data.iter().find(|e| e.id == "Verdict").unwrap();
        assert!(
            verdict_enum
                .permissible_values
                .iter()
                .all(|pv| pv.rule_pointers.is_empty()),
            "no value-level pointer from a union-ranged constant"
        );
        assert!(
            crate::diagnostics::impossible_rule_values(&schema).is_empty(),
            "and no never-fires finding either — the union admits other values"
        );
    }

    #[test]
    fn shared_slot_rules_group_by_class() {
        use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
        // Two classes carry the slot and each governs it with a rule: the
        // card groups the rules under each class's name, once per class.
        let mut schema = SchemaDefinition::new("approvals");
        schema
            .slots
            .insert("verdict".to_string(), SlotDefinition::new("verdict"));
        schema.slots.insert(
            "approved_by".to_string(),
            SlotDefinition::new("approved_by"),
        );
        for name in ["AlphaReview", "BetaReview"] {
            let mut cls = ClassDefinition::new(name);
            cls.slots = vec!["verdict".into(), "approved_by".into()];
            cls.rules = vec![approval_rule(None)];
            schema.classes.insert(name.to_string(), cls);
        }

        let data = HtmlWriter::build_template_data(&schema);
        let verdict = data.slot_data.iter().find(|s| s.id == "verdict").unwrap();
        assert_eq!(
            verdict.governing_rule_groups.len(),
            2,
            "each governing class is its own group"
        );
        assert!(
            verdict.show_rule_group_labels,
            "rules from more than one class need their class named"
        );

        let out = tempfile::tempdir().unwrap();
        HtmlWriter::with_options(false)
            .write(&schema, out.path())
            .expect("write");
        let html = fs::read_to_string(out.path().join("index.html")).expect("read");
        assert!(
            html.contains(r#"governing-rule-group"#)
                && html.contains(r#"#class-AlphaReview" class="entity-link">AlphaReview"#)
                && html.contains(r#"#class-BetaReview" class="entity-link">BetaReview"#),
            "the shared slot's card names each governing class once as a group label"
        );
    }

    #[test]
    fn rule_participants_list_a_slot_once_across_both_sides() {
        use crate::linkml::{SlotCondition, ValuePresence};
        // `verdict` triggers the rule AND is governed by it; the graph
        // attribute lists it once.
        let mut rule = approval_rule(None);
        rule.postconditions
            .as_mut()
            .unwrap()
            .slot_conditions
            .insert(
                "verdict".to_string(),
                SlotCondition {
                    value_presence: Some(ValuePresence::Present),
                    ..Default::default()
                },
            );
        let schema = crate::linkml::SchemaDefinition::new("approvals");
        let ids = rule_participant_ids(
            "ImageApproval",
            &crate::rules::rule_participants(&rule),
            &std::collections::BTreeMap::new(),
            &schema,
        );
        assert_eq!(
            ids.matches("slot:verdict").count(),
            1,
            "a slot on both rule sides appears once; got: {ids}"
        );
        assert!(ids.starts_with("class:ImageApproval"));
    }

    #[test]
    fn page_composition_orders_and_omits_sections() {
        let schema = bottle_rack_schema();
        let data = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");

        let render = |writer: HtmlWriter| {
            let temp_dir = std::env::temp_dir()
                .join(format!("panschema_composition_test_{}", std::process::id()));
            let _ = fs::remove_dir_all(&temp_dir);
            writer.write(&schema, &temp_dir).expect("write");
            let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
            let wasm = temp_dir.join("panschema_viz_bg.wasm").is_file();
            let _ = fs::remove_dir_all(&temp_dir);
            (html, wasm)
        };

        let (html, wasm) = render(
            HtmlWriter::with_options(false)
                .with_instance_dataset(InstanceDataset::new("catalog", data.clone())),
        );
        assert!(!wasm, "no graph requested, no wasm shipped");
        let classes = html
            .find(r#"<section id="classes""#)
            .expect("schema sections render by default");
        let individuals = html
            .find(r#"<section id="individuals""#)
            .expect("instance section renders");
        assert!(classes < individuals, "schema-first is the default order");

        let (html, _) = render(
            HtmlWriter::with_options(false)
                .with_instance_dataset(InstanceDataset::new("catalog", data.clone()))
                .with_instances_first(true),
        );
        let classes = html
            .find(r#"<section id="classes""#)
            .expect("still present");
        let individuals = html
            .find(r#"<section id="individuals""#)
            .expect("still present");
        assert!(
            individuals < classes,
            "instances-first puts the instance section before the schema reference"
        );
        let sidebar_individuals = html.find(r##"href="#individuals""##).expect("sidebar link");
        let sidebar_classes = html.find(r##"href="#classes""##).expect("sidebar link");
        assert!(
            sidebar_individuals < sidebar_classes,
            "the sidebar follows the page order"
        );

        let (html, wasm) = render(
            HtmlWriter::new()
                .with_instance_dataset(InstanceDataset::new("catalog", data))
                .with_schema_sections(false),
        );
        assert!(
            wasm,
            "the data-only page's instance canvas still ships its wasm"
        );
        for gone in [
            r#"<section id="classes""#,
            r#"<section id="slots""#,
            r#"<section id="enums""#,
            r#"<section id="types""#,
            r##"href="#classes""##,
            r##"href="#graph-visualization""##,
            r##"href="#class-"##,
            r##"href="#slot-"##,
        ] {
            assert!(
                !html.contains(gone),
                "schema_sections = false omits the schema reference: found `{gone}`"
            );
        }
        assert!(
            html.contains(r#"<section id="individuals""#),
            "the instance section stays"
        );
        assert!(
            html.contains(r#"<section id="namespaces""#),
            "the namespace table stays — the instance cards' CURIEs expand through it"
        );
        assert!(
            html.contains("window.PanschemaGraphShell"),
            "the graph shell script ships with the instance canvas it serves"
        );

        let (_, wasm) = render(HtmlWriter::new().with_schema_sections(false));
        assert!(
            !wasm,
            "a page with no canvas at all ships no wasm, whatever the graph flag says"
        );

        assert!(
            HtmlWriter::renders_empty(false, 0),
            "no sections and no data is the empty page"
        );
        assert!(
            !HtmlWriter::renders_empty(true, 0) && !HtmlWriter::renders_empty(false, 1),
            "either half present means the page has content"
        );
    }

    #[test]
    fn multiple_instance_datasets_render_a_switchable_selector() {
        let schema = bottle_rack_schema();
        let preview = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let worked = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b2\n    name: Fleurie\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );

        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("preview", preview))
            .with_instance_dataset(InstanceDataset::new("worked-example", worked));
        let temp_dir = std::env::temp_dir().join("panschema_instance_selector_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            html.contains(r#"role="tablist""#),
            "several datasets must offer a selector"
        );
        assert_eq!(
            html.matches(r#"role="tab""#).count(),
            2,
            "one selector entry per declared dataset"
        );
        assert_eq!(
            html.matches("data-instance-dataset=").count(),
            6,
            "each dataset needs a selector entry plus its metadata and individuals panels"
        );
        assert!(
            html.contains(">preview") && html.contains(">worked-example"),
            "the selector must name every declared dataset"
        );
        assert!(
            html.contains("Morgon") && html.contains("Fleurie") && html.contains("North Rack"),
            "every dataset's individuals must be present in the page"
        );
        assert_eq!(
            html.matches(r#"class="instance-dataset-panel""#).count(),
            2,
            "each dataset gets its own content panel"
        );
        assert!(
            html.contains(r#"data-instance-dataset="0">"#),
            "the first dataset's panel shows by default"
        );
        assert!(
            html.contains(r#"data-instance-dataset="1" hidden>"#),
            "every other dataset's panel starts hidden"
        );
        assert_eq!(
            html.matches("__PANSCHEMA_INSTANCE_GRAPHS__ =").count(),
            1,
            "every dataset's viz payload rides in one array"
        );
        assert!(
            html.contains(r#"{"name": "preview""#) && html.contains(r#"{"name": "worked-example""#),
            "each payload entry is labelled with its dataset"
        );
    }

    #[test]
    fn the_dataset_marked_default_is_the_one_shown_first() {
        // `exemplar = true` on a later entry must not have to be reordered to
        // be the default: declaration order drives the selector, the flag
        // drives which panel opens.
        let schema = bottle_rack_schema();
        let preview = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let worked = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b2\n    name: Fleurie\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );

        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("preview", preview))
            .with_instance_dataset(InstanceDataset::new("worked-example", worked).as_default());
        let temp_dir = std::env::temp_dir().join("panschema_default_dataset_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        // Order is unchanged: preview is still the first selector entry.
        let first_tab = html.find(">preview").expect("preview tab present");
        let second_tab = html
            .find(">worked-example")
            .expect("worked-example tab present");
        assert!(
            first_tab < second_tab,
            "declaration order drives the selector order"
        );

        // But the second dataset is the open one.
        assert!(
            html.contains(r#"data-instance-dataset="0" hidden>"#),
            "the non-default dataset's panel starts hidden"
        );
        assert!(
            html.contains(r#"data-instance-dataset="1">"#),
            "the dataset marked default is the visible one"
        );
        assert_eq!(
            html.matches(r#"aria-selected="true""#).count(),
            1,
            "exactly one selector entry is selected"
        );

        // The sidebar badge describes the default dataset: two nodes, one edge,
        // and says which number is which.
        assert!(
            html.contains(r##"aria-label="2 nodes, 1 edge""##) && html.contains(">2 / 1<"),
            "the sidebar badge counts the default dataset's graph, labelled"
        );
    }

    #[test]
    fn a_default_dataset_with_nothing_to_show_yields_the_slot() {
        // The marked dataset holds no records, so it is dropped from the
        // page; the surviving one must open rather than leaving every panel
        // hidden and no tab selected.
        let schema = bottle_rack_schema();
        let real = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let empty = instance_set_from_yaml(&schema, "bottles: []\n");

        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("real", real))
            .with_instance_dataset(InstanceDataset::new("empty", empty).as_default());
        let temp_dir = std::env::temp_dir().join("panschema_empty_default_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert_eq!(
            html.matches(r#"class="instance-dataset-panel""#).count(),
            1,
            "the empty dataset has nothing to show and is dropped"
        );
        assert!(
            html.contains(r#"data-instance-dataset="0">"#),
            "the surviving dataset opens"
        );
        assert!(html.contains("Morgon"), "its individuals render");
    }

    #[test]
    fn datasets_sharing_record_ids_get_distinct_anchors() {
        // A teaching preview is usually a *subset* of the worked example, so the
        // same individuals appear in both panels. Emitting `ind-<id>` in each
        // would duplicate element ids and send a reference link in one panel to
        // the other panel's card — which is hidden, so the link looks dead.
        let schema = bottle_rack_schema();
        let preview = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b1\n    name: Morgon\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );
        let worked = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b1\n    name: Morgon\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );

        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("preview", preview))
            .with_instance_dataset(InstanceDataset::new("worked-example", worked));
        let temp_dir = std::env::temp_dir().join("panschema_shared_ids_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        let mut counts = std::collections::BTreeMap::new();
        let mut rest = html.as_str();
        while let Some(at) = rest.find("id=\"") {
            rest = &rest[at + 4..];
            let end = rest.find('"').unwrap_or(0);
            *counts.entry(&rest[..end]).or_insert(0usize) += 1;
        }
        let dupes: Vec<_> = counts.iter().filter(|(_, n)| **n > 1).collect();
        assert!(
            dupes.is_empty(),
            "every element id must be unique across dataset panels; duplicated: {dupes:?}"
        );

        // Each panel's own links point into its own namespace.
        assert!(
            html.contains(r##"href="#d0-ind-b1""##) && html.contains(r##"href="#d1-ind-b1""##),
            "each panel's entity link must target the card in that same panel"
        );
    }

    #[test]
    fn the_instance_section_heading_counts_nodes_and_edges() {
        // The heading used to report the number of individuals, which looks
        // like a node count and isn't one — it silently omits edges, and it
        // will diverge outright once the A-box gains nodes that aren't
        // individuals. Every graph count reads nodes/edges.
        let schema = bottle_rack_schema();
        let only = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b1\n    name: Morgon\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );

        let writer = HtmlWriter::new().with_instance_dataset(InstanceDataset::new("only", only));
        let temp_dir = std::env::temp_dir().join("panschema_instance_heading_count_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        // Two individuals joined by `stored_in`: two nodes, one edge.
        let heading = html
            .split_once(r#"id="instance-graph-count""#)
            .map(|(_, rest)| rest.chars().take(200).collect::<String>())
            .expect("the instance heading carries a graph count");
        assert!(
            heading.contains("2 / 1"),
            "the heading must report nodes and edges; got: {heading}"
        );
        // And it must say which number is which, for a reader who hasn't
        // been told the convention.
        assert!(
            heading.contains("2 nodes") && heading.contains("1 edge"),
            "the count needs a readable expansion (tooltip/label); got: {heading}"
        );
    }

    #[test]
    fn each_dataset_carries_its_own_counts_and_provenance() {
        let schema = bottle_rack_schema();
        let preview = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let worked = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: b2\n    name: Fleurie\n    stored_in: r1\nracks:\n  - id: r1\n    name: North Rack\n",
        );

        let writer = HtmlWriter::new()
            .with_instance_dataset(
                InstanceDataset::new("preview", preview).with_provenance("preview.yaml"),
            )
            .with_instance_dataset(
                InstanceDataset::new("worked-example", worked).with_provenance("worked.yaml"),
            );
        let temp_dir = std::env::temp_dir().join("panschema_dataset_counts_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            html.contains("preview.yaml") && html.contains("worked.yaml"),
            "each panel names its own source file"
        );
        // The one-bottle preview has a single node and no edges; the worked
        // example has two nodes joined by `stored_in`.
        assert!(
            html.contains(">1 / 0<"),
            "the preview's badge counts its own graph"
        );
        assert!(
            html.contains(">2 / 1<"),
            "the worked example's badge counts its own graph"
        );
    }

    #[test]
    fn a_single_instance_dataset_renders_without_selector_chrome() {
        let schema = bottle_rack_schema();
        let only = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");

        let writer = HtmlWriter::new().with_instance_dataset(InstanceDataset::new("only", only));
        let temp_dir = std::env::temp_dir().join("panschema_single_dataset_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            !html.contains(r#"role="tablist""#),
            "one dataset needs no selector"
        );
        assert_eq!(
            html.matches("data-instance-dataset=").count(),
            2,
            "the metadata and individuals panels, with no selector entry"
        );
        assert!(html.contains("Morgon"), "its individuals still render");
        // A lone dataset keeps the unprefixed anchor form, so `#ind-<id>` deep
        // links published before the selector existed still resolve.
        assert!(
            html.contains(r##"id="ind-b1""##) && html.contains(r##"href="#ind-b1""##),
            "a single dataset must not namespace its anchors"
        );
    }

    #[test]
    fn entity_list_disambiguates_shared_labels_by_class() {
        // Two individuals of different classes can legitimately share a
        // display name; a label-only list makes them indistinguishable.
        let schema = bottle_rack_schema();
        let set = instance_set_from_yaml(
            &schema,
            "bottles:\n  - id: bx\n    name: Bordeaux\nracks:\n  - id: rx\n    name: Bordeaux\n",
        );

        let writer = HtmlWriter::new().with_instance_dataset(InstanceDataset::new("only", set));
        let temp_dir = std::env::temp_dir().join("panschema_entity_class_tag_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        let list = html
            .split_once(r#"class="entity-list""#)
            .map(|(_, rest)| {
                rest.split_once("</div>")
                    .map(|(a, _)| a.to_string())
                    .unwrap_or_default()
            })
            .expect("entity list present");
        assert!(
            list.contains("Bottle") && list.contains("Rack"),
            "each entry names its class, so same-label individuals read apart; got: {list}"
        );
    }

    /// Both toolbars render from one shared macro (`graph_toolbar.html`),
    /// so their common controls cannot drift — the test proves the macro
    /// rendered each graph's prefix with the shared parts identical (the
    /// reset label, the pan/zoom hint, the keyboard hints) and the
    /// per-graph arguments applied where they should: the schema's Arrows
    /// tooltip names T-box edge types the A-box lacks, Groundings and the
    /// 3D pan hint are schema-only, and the caption names the shared node
    /// vocabulary. The layout `<option>` tooltips (still per-template)
    /// stay graph-agnostic where the wording allows.
    #[test]
    fn instance_graph_chrome_matches_the_schema_graphs() {
        let schema = bottle_rack_schema();
        let set = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let writer = HtmlWriter::new().with_instance_dataset(InstanceDataset::new("only", set));
        let out = tempfile::tempdir().unwrap();
        writer.write(&schema, out.path()).expect("write");
        let html = fs::read_to_string(out.path().join("index.html")).expect("read");

        // Slice `hay` between `from` and `to`, failing loudly when a
        // marker is missing rather than collapsing to an empty match.
        fn between<'a>(hay: &'a str, from: &str, to: &str) -> &'a str {
            let (_, rest) = hay.split_once(from).unwrap_or_else(|| {
                panic!("marker {from:?} not found");
            });
            rest.split_once(to).map(|(a, _)| a).unwrap_or_else(|| {
                panic!("no {to:?} after {from:?}");
            })
        }

        let caption = between(&html, r#"class="instance-graph-caption""#, "</p>")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !caption.contains("teal hexagons"),
            "the caption must not describe symbols the canvas does not draw"
        );
        // A prose pin: re-pick the phrase if the caption is reworded.
        assert!(
            caption.contains("same symbols") && caption.contains("schema graph"),
            "the caption names the shared vocabulary; got: {caption}"
        );

        // Both toolbars now render the shared `graph-*` classes; the
        // page is split at the instance block so each side's uniquely
        // IDed controls are read from its own region.
        let (schema_region, instance_region) = html
            .split_once(r#"class="instance-graph-block""#)
            .expect("instance graph block present");

        // Both toolbars are the shared control overlay, not two shapes.
        assert!(
            schema_region.contains(r#"class="graph-controls""#)
                && instance_region.contains(r#"class="graph-controls""#),
            "both graphs render the shared control overlay"
        );
        assert!(
            instance_region.contains(r#"class="graph-btn graph-toggle active""#),
            "the instance toolbar uses the shared button + toggle classes"
        );

        // The active-toggle look is defined once, in the shell — not
        // per template. The old instance-only rule must be gone, so an
        // active toggle can never read differently between the graphs.
        assert!(
            html.contains(".graph-toggle.active {"),
            "the shared active-toggle style is defined"
        );
        assert!(
            !html.contains(".instance-graph-toggle.active"),
            "the per-template active-toggle style is gone (shared now)"
        );

        // The visible label after the tag closes — the `title` attribute
        // also contains "Reset", so only post-`>` text proves a label.
        let visible_reset = |region: &str, id: &str| {
            let button = between(region, id, "</button>");
            let (_, text) = button.split_once('>').expect("button tag closes");
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        };
        let schema_reset = visible_reset(schema_region, r#"id="graph-reset""#);
        let instance_reset = visible_reset(instance_region, r#"id="instance-graph-reset""#);
        assert_eq!(
            instance_reset, schema_reset,
            "both toolbars label their reset control identically"
        );
        assert!(
            instance_reset.contains("Reset"),
            "the reset control shows a visible label, not just an icon; got: {instance_reset}"
        );

        assert_eq!(
            between(
                instance_region,
                r#"class="graph-help" id="instance-graph-help-2d">"#,
                "</span>"
            ),
            between(
                schema_region,
                r#"class="graph-help" id="graph-help-2d">"#,
                "</span>"
            ),
            "both toolbars carry the same pan/zoom hint"
        );

        // The keyboard-hint gap is closed: both graphs wire L/N/E, so the
        // shared macro's hints are honest on each.
        assert!(
            between(
                instance_region,
                r#"id="instance-graph-labels-all""#,
                "</button>"
            )
            .contains("(L)"),
            "the instance Labels toggle carries the shared (L) key hint"
        );

        // The two per-graph arguments landed on the right graph: the
        // schema's Arrows names T-box edge types the A-box has none of.
        assert!(
            between(schema_region, r#"id="graph-arrows""#, "</button>").contains("is_a"),
            "the schema Arrows tooltip describes T-box edge types"
        );
        assert!(
            between(
                instance_region,
                r#"id="instance-graph-arrows""#,
                "</button>"
            )
            .contains("assertion edges"),
            "the instance Arrows tooltip describes A-box assertion edges"
        );

        // Viz asset URLs carry the build-content stamp, so repeat views
        // cache the module while a rebuilt bundle busts it. A per-view
        // timestamp here would refetch the multi-MB wasm every visit.
        let stamp = wasm_files::viz_stamp();
        assert_eq!(stamp.len(), 16, "stamp is a fixed-width hash");
        assert!(
            html.contains(&format!("const v = '{stamp}'"))
                && html.contains("./panschema_viz_bg.wasm?v=${v}"),
            "the page requests the wasm with the content stamp"
        );
        assert!(
            !html.contains("Date.now"),
            "no per-view timestamp anywhere on the page busts the viz assets"
        );

        // Groundings is the one schema-only flag on the macro.
        assert!(
            schema_region.contains(r#"id="graph-toggle-external""#)
                && !instance_region.contains("toggle-external"),
            "Groundings renders on the schema graph only"
        );
        // The graph renders in 2D only; no mode toggle or orbit hint.
        assert!(
            !html.contains("help-3d") && !html.contains("graph-mode-3d"),
            "no page carries a 3D mode control or orbit hint"
        );

        // The graph-agnostic layout tooltips are shared verbatim. The
        // schema graph's select sits outside its controls strip, so
        // each side is sliced by its own select element.
        let schema_select = between(&html, r#"id="graph-layout-select""#, "</select>");
        let instance_select = between(&html, r#"id="instance-graph-layout-select""#, "</select>");
        let title_of = |select: &str, value: &str| {
            between(select, &format!(r#"<option value="{value}" title=""#), "\"").to_string()
        };
        for shared in ["stress", "kamada-kawai"] {
            assert_eq!(
                title_of(instance_select, shared),
                title_of(schema_select, shared),
                "the `{shared}` layout explains itself identically on both toolbars"
            );
        }
        for own in ["sgd", "hierarchical", "force-directed"] {
            assert!(
                !title_of(instance_select, own).is_empty(),
                "the `{own}` layout explains itself on the instance toolbar"
            );
        }
    }

    #[test]
    fn the_section_reads_graph_first_with_dataset_metadata() {
        // The section holds the dataset(s); within each, the graph precedes
        // the individuals, and the dataset's own metadata — the container's
        // scalar values plus the source — leads. One dataset reads singular,
        // several plural.
        use crate::linkml::{ClassDefinition, SlotDefinition};
        let mut schema = bottle_rack_schema();
        // The container gains its own scalar slot for the metadata.
        {
            let container = schema.classes.get_mut("Cellar").unwrap();
            let mut title = SlotDefinition::new("title");
            title.range = Some("string".to_string());
            container.attributes.insert("title".to_string(), title);
            let _ = ClassDefinition::new("unused");
        }
        let only = instance_set_from_yaml(
            &schema,
            "title: North wing cellar\nbottles:\n  - id: b1\n    name: Morgon\n",
        );
        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("only", only).with_provenance("only.yaml"));
        let temp_dir = std::env::temp_dir().join("panschema_graph_first_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        // Singular heading for one dataset.
        assert!(
            html.contains(">Instance Graph<") || html.contains("Instance Graph\n"),
            "one dataset reads singular"
        );
        assert!(
            !html.contains("Instance Graphs"),
            "no plural for one dataset"
        );

        // Metadata renders, and the order is metadata → graph → individuals.
        // Match markup, not the stylesheet's class definitions.
        let meta_at = html
            .find("North wing cellar")
            .expect("container title renders");
        let graph_at = html
            .find(r#"id="instance-graph-canvas""#)
            .expect("canvas present");
        let cards_at = html
            .find(r#"class="individual-cards""#)
            .expect("cards present");
        assert!(
            meta_at < graph_at && graph_at < cards_at,
            "order must be metadata ({meta_at}) → graph ({graph_at}) → individuals ({cards_at})"
        );
        assert!(
            html.contains(">Individuals<"),
            "the cards sit under an Individuals subheading"
        );
    }

    #[test]
    fn several_datasets_read_plural() {
        let schema = bottle_rack_schema();
        let a = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");
        let b = instance_set_from_yaml(&schema, "bottles:\n  - id: b2\n    name: Fleurie\n");
        let writer = HtmlWriter::new()
            .with_instance_dataset(InstanceDataset::new("a", a))
            .with_instance_dataset(InstanceDataset::new("b", b));
        let temp_dir = std::env::temp_dir().join("panschema_plural_heading_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);
        assert!(
            html.contains("Instance Graphs"),
            "several datasets read plural"
        );
    }

    #[test]
    fn a_schema_with_no_a_box_shows_the_placeholder_and_no_sidebar_entry() {
        // Nothing to show: no instance data attached and no embedded OWL
        // individuals. The section must say so, and the sidebar must not
        // offer an entry that leads nowhere.
        let schema = bottle_rack_schema();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_no_abox_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            html.contains("No individuals defined in this ontology."),
            "an empty A-box should render the placeholder"
        );
        assert!(
            !html.contains(r#"class="instance-dataset-panel""#),
            "an empty A-box gets no content panel"
        );
        assert!(
            !html.contains("href=\"#individuals\""),
            "the sidebar should not link to an empty Instance Graph section"
        );
    }

    #[test]
    fn individual_cards_render_with_the_graph_viz_disabled() {
        // --no-graph suppresses every viz payload, but the individuals are
        // still documented: cards, provenance, and the sidebar entry stay.
        let schema = bottle_rack_schema();
        let only = instance_set_from_yaml(&schema, "bottles:\n  - id: b1\n    name: Morgon\n");

        let writer = HtmlWriter::with_options(false)
            .with_instance_dataset(InstanceDataset::new("only", only).with_provenance("only.yaml"));
        let temp_dir = std::env::temp_dir().join("panschema_no_graph_cards_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            html.contains("Morgon"),
            "individual cards render without a viz payload"
        );
        assert!(
            html.contains("only.yaml"),
            "the panel still names its source"
        );
        assert!(
            html.contains("href=\"#individuals\""),
            "the sidebar still offers the Instance Graph entry"
        );
        assert!(
            !html.contains("No individuals defined in this ontology."),
            "the placeholder is for an absent A-box, not an absent viz"
        );
    }

    #[test]
    fn schema_embedded_individuals_say_so_in_the_provenance_line() {
        // No instance-data file attached, so the A-box is the schema's own
        // individuals — the provenance line must say that rather than name a
        // file that isn't there.
        let reader = OwlReader::new();
        let schema = reader.read(&reference_ontology_path()).unwrap();

        let writer = HtmlWriter::new();
        let temp_dir = std::env::temp_dir().join("panschema_provenance_test");
        let _ = fs::remove_dir_all(&temp_dir);
        writer.write(&schema, &temp_dir).expect("write");
        let html = fs::read_to_string(temp_dir.join("index.html")).expect("read");
        let _ = fs::remove_dir_all(&temp_dir);

        assert!(
            html.contains("Source: individuals embedded in the schema"),
            "the section must attribute the A-box to the schema itself"
        );
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

        let data = HtmlWriter::build_template_data_with_labels(&schema, Some(&store), true);
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
