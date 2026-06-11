//! Upstream ontology label cache.
//!
//! Stores `{IRI → rdfs:label}` maps fetched from upstream ontologies
//! (BFO, CCO, PROV-O, …) so rendered docs can show human-readable
//! names for external CURIEs instead of opaque identifiers. One JSON
//! file per upstream source, keyed by SHA-256 of the source URL, under
//! a caller-supplied cache directory (production:
//! `~/.cache/panschema/labels`).
//!
//! Everything here fails open: a corrupt cache file is skipped with a
//! warning, a missing label is a `None` — generation never blocks on
//! this layer.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LabelStoreError {
    #[error("failed to create label cache dir {dir}: {source}")]
    CreateDir {
        dir: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write label cache file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to serialize label map: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Label + definition for one upstream term. Either field may be
/// absent — an ontology can label a term without defining it and
/// vice versa.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TermInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition: Option<String>,
}

/// On-disk term cache: one JSON `{iri: {label, definition}}` map per
/// upstream source URL. Pre-definition cache files (flat
/// `{iri: label}`) fail to parse, get skipped, and refetch on the
/// next run — the fail-open path doubles as format migration.
pub struct LabelStore {
    cache_dir: PathBuf,
    /// Loaded term maps, in directory-read order. `lookup` returns
    /// the first hit.
    sources: Vec<BTreeMap<String, TermInfo>>,
    /// Cache-file names already present (sha256 hex of source URL),
    /// so orchestration can skip re-fetching cached sources.
    cached_keys: Vec<String>,
}

impl LabelStore {
    /// Open the store rooted at `cache_dir`, loading every readable
    /// `.json` label map already present. Unreadable or unparseable
    /// files are skipped with a warning.
    pub fn open(cache_dir: impl Into<PathBuf>) -> Result<Self, LabelStoreError> {
        let cache_dir = cache_dir.into();
        fs::create_dir_all(&cache_dir).map_err(|source| LabelStoreError::CreateDir {
            dir: cache_dir.clone(),
            source,
        })?;

        let mut sources = Vec::new();
        let mut cached_keys = Vec::new();
        if let Ok(entries) = fs::read_dir(&cache_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "json") {
                    continue;
                }
                match fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|body| {
                        serde_json::from_str::<BTreeMap<String, TermInfo>>(&body)
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(labels) => {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            cached_keys.push(stem.to_string());
                        }
                        sources.push(labels);
                    }
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
                            error = %err,
                            "skipping unreadable label cache file"
                        );
                    }
                }
            }
        }

        Ok(Self {
            cache_dir,
            sources,
            cached_keys,
        })
    }

    /// Persist a label map for `source_url` and make it available to
    /// `lookup` immediately.
    pub fn insert_source(
        &mut self,
        source_url: &str,
        labels: BTreeMap<String, TermInfo>,
    ) -> Result<(), LabelStoreError> {
        let key = source_key(source_url);
        let path = self.cache_dir.join(format!("{key}.json"));
        let body = serde_json::to_string(&labels)?;
        fs::write(&path, body).map_err(|source| LabelStoreError::Write { path, source })?;
        self.cached_keys.push(key);
        self.sources.push(labels);
        Ok(())
    }

    /// `true` when a label map for `source_url` is already cached —
    /// orchestration uses this to skip re-fetching.
    pub fn has_source(&self, source_url: &str) -> bool {
        self.cached_keys.contains(&source_key(source_url))
    }

    /// Look an expanded IRI up across all loaded sources; first hit
    /// wins.
    pub fn lookup(&self, iri: &str) -> Option<&TermInfo> {
        self.sources.iter().find_map(|terms| terms.get(iri))
    }
}

fn source_key(source_url: &str) -> String {
    hex::encode(Sha256::digest(source_url.as_bytes()))
}

#[derive(Debug, Error)]
pub enum LabelFetchError {
    #[error("failed to fetch {url}: {message}")]
    Http { url: String, message: String },
}

/// Abstraction over "download the RDF document at this URL" so the
/// orchestration layer and its tests never touch the network
/// directly. Mirrors the `TarballSource` pattern in the schema
/// manager.
pub trait LabelSource {
    fn fetch(&self, url: &str) -> Result<String, LabelFetchError>;
}

/// Production source: plain HTTPS GET via ureq.
pub struct HttpLabelSource;

impl LabelSource for HttpLabelSource {
    // `#[mutants::skip]`: thin ureq wrapper — exercising it requires
    // live network; the orchestration above it is tested via the
    // counting test double.
    #[mutants::skip]
    fn fetch(&self, url: &str) -> Result<String, LabelFetchError> {
        ureq::get(url)
            .call()
            .map_err(|e| LabelFetchError::Http {
                url: url.to_string(),
                message: e.to_string(),
            })?
            .into_string()
            .map_err(|e| LabelFetchError::Http {
                url: url.to_string(),
                message: e.to_string(),
            })
    }
}

#[derive(Debug, Error)]
#[error("failed to parse upstream RDF: {0}")]
pub struct LabelExtractError(String);

/// Built-in `namespace IRI → label-source URL` map for the
/// well-known upstream ecosystem. Matched by exact namespace IRI as
/// declared in the schema's `prefixes:`. The URLs point at
/// latest-release documents; authors who need version-pinned labels
/// override per-prefix in the manifest (slice 13.5).
pub const BUILTIN_LABEL_SOURCES: &[(&str, &str)] = &[
    (
        "https://www.commoncoreontologies.org/",
        "https://raw.githubusercontent.com/CommonCoreOntology/CommonCoreOntologies/master/src/cco-merged/CommonCoreOntologiesMerged.ttl",
    ),
    (
        "http://purl.obolibrary.org/obo/BFO_",
        "http://purl.obolibrary.org/obo/bfo.owl",
    ),
    (
        "http://purl.obolibrary.org/obo/RO_",
        "http://purl.obolibrary.org/obo/ro.owl",
    ),
    (
        "http://purl.obolibrary.org/obo/IAO_",
        "http://purl.obolibrary.org/obo/iao.owl",
    ),
    (
        "http://www.w3.org/ns/prov#",
        "http://www.w3.org/ns/prov.ttl",
    ),
    (
        "http://www.w3.org/2004/02/skos/core#",
        "http://www.w3.org/2009/08/skos-reference/skos.rdf",
    ),
    (
        "http://purl.org/dc/terms/",
        "https://www.dublincore.org/specifications/dublin-core/dcmi-terms/dublin_core_terms.ttl",
    ),
    (
        "http://purl.org/spar/cito/",
        "https://raw.githubusercontent.com/SPAROntologies/cito/master/docs/current/cito.ttl",
    ),
    ("http://www.w3.org/ns/oa#", "http://www.w3.org/ns/oa.ttl"),
];

/// Extract `{subject IRI → TermInfo}` from an RDF document — Turtle
/// first, falling back to RDF/XML (OBO PURLs serve the latter).
///
/// Labels: `rdfs:label` wins over `skos:prefLabel`. Definitions:
/// `skos:definition` (CCO) > `IAO:0000115` (OBO/BFO) >
/// `dc:description` > `rdfs:comment` (CiTO / W3C vocabularies).
/// Within each predicate, `@en` or untagged literals win and other
/// languages are ignored.
pub fn extract_terms(rdf: &str) -> Result<BTreeMap<String, TermInfo>, LabelExtractError> {
    use sophia::api::prelude::TripleSource;
    use sophia::inmem::graph::FastGraph;

    let graph: FastGraph = match sophia::turtle::parser::turtle::parse_str(rdf).collect_triples() {
        Ok(graph) => graph,
        Err(ttl_err) => sophia::xml::parser::parse_str(rdf)
            .collect_triples()
            .map_err(|xml_err| {
                LabelExtractError(format!("not Turtle ({ttl_err}) nor RDF/XML ({xml_err})"))
            })?,
    };

    const LABEL_PREDICATES: &[&str] = &[
        "http://www.w3.org/2000/01/rdf-schema#label",
        "http://www.w3.org/2004/02/skos/core#prefLabel",
    ];
    const DEFINITION_PREDICATES: &[&str] = &[
        "http://www.w3.org/2004/02/skos/core#definition",
        "http://purl.obolibrary.org/obo/IAO_0000115",
        "http://purl.org/dc/elements/1.1/description",
        "http://www.w3.org/2000/01/rdf-schema#comment",
    ];

    let mut out: BTreeMap<String, TermInfo> = BTreeMap::new();
    collect_first_literal(&graph, LABEL_PREDICATES, &mut out, |info| &mut info.label);
    collect_first_literal(&graph, DEFINITION_PREDICATES, &mut out, |info| {
        &mut info.definition
    });
    out.retain(|_, info| info.label.is_some() || info.definition.is_some());
    Ok(out)
}

/// For each subject, store the first `@en`-or-untagged literal found
/// across `predicates` (listed in priority order) into the `TermInfo`
/// field selected by `field`.
fn collect_first_literal(
    graph: &sophia::inmem::graph::FastGraph,
    predicates: &[&str],
    out: &mut BTreeMap<String, TermInfo>,
    field: impl Fn(&mut TermInfo) -> &mut Option<String>,
) {
    use sophia::api::graph::Graph;
    use sophia::api::term::Term;
    use sophia::api::triple::Triple;

    for predicate in predicates {
        for triple in graph.triples().flatten() {
            if triple.p().iri().is_none_or(|i| i.as_str() != *predicate) {
                continue;
            }
            let english_or_untagged = triple
                .o()
                .language_tag()
                .is_none_or(|tag| tag.as_str().eq_ignore_ascii_case("en"));
            if !english_or_untagged {
                continue;
            }
            if let (Some(subject), Some(value)) = (triple.s().iri(), triple.o().lexical_form()) {
                let info = out.entry(subject.as_str().to_string()).or_default();
                let slot = field(info);
                if slot.is_none() {
                    *slot = Some(value.to_string());
                }
            }
        }
    }
}

/// Open the production label store (under the panschema cache root)
/// and, unless `offline`, fetch label maps for the schema's known
/// prefixes. Fail-open: `None` means external references render as
/// CURIEs.
// `#[mutants::skip]`: fail-open glue binding the process cache root
// and the live-network source to components that are each tested in
// isolation (`LabelStore::open`, `ensure_labels`); its mutants are
// only observable with a real cache dir + network.
#[mutants::skip]
pub fn open_default_store(
    schema: &crate::linkml::SchemaDefinition,
    offline: bool,
) -> Option<LabelStore> {
    let labels_dir = match crate::cache::cache_root() {
        Ok(root) => root.join("labels"),
        Err(err) => {
            tracing::warn!(error = %err, "label cache unavailable; rendering CURIEs");
            return None;
        }
    };
    let mut store = match LabelStore::open(&labels_dir) {
        Ok(store) => store,
        Err(err) => {
            tracing::warn!(error = %err, "label cache unavailable; rendering CURIEs");
            return None;
        }
    };
    if !offline {
        ensure_labels(schema, &mut store, &HttpLabelSource);
    }
    Some(store)
}

/// Walk the schema's declared prefixes, fetch label maps for every
/// namespace with a known source (on cache miss only), and populate
/// the store. Every failure mode is per-source fail-open: a fetch or
/// parse error is logged and the remaining prefixes still process.
pub fn ensure_labels(
    schema: &crate::linkml::SchemaDefinition,
    store: &mut LabelStore,
    source: &dyn LabelSource,
) {
    for namespace in schema.prefixes.values() {
        let Some((_, url)) = BUILTIN_LABEL_SOURCES.iter().find(|(ns, _)| ns == namespace) else {
            continue;
        };
        if store.has_source(url) {
            continue;
        }
        let labels = match source
            .fetch(url)
            .map_err(|e| e.to_string())
            .and_then(|body| extract_terms(&body).map_err(|e| e.to_string()))
        {
            Ok(labels) => labels,
            Err(err) => {
                tracing::warn!(url, error = %err, "skipping label source");
                continue;
            }
        };
        if let Err(err) = store.insert_source(url, labels) {
            tracing::warn!(url, error = %err, "failed to cache label map");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("panschema_label_store_{tag}"));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn term(label: &str) -> TermInfo {
        TermInfo {
            label: Some(label.to_string()),
            definition: None,
        }
    }

    fn cco_labels() -> BTreeMap<String, TermInfo> {
        BTreeMap::from([(
            "https://www.commoncoreontologies.org/ont00000958".to_string(),
            term("Process"),
        )])
    }

    #[test]
    fn lookup_hits_after_insert_source() {
        let dir = temp_cache_dir("insert");
        let mut store = LabelStore::open(&dir).unwrap();
        store
            .insert_source("https://example.org/cco.ttl", cco_labels())
            .unwrap();
        assert_eq!(
            store
                .lookup("https://www.commoncoreontologies.org/ont00000958")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        assert!(store.lookup("https://example.org/unknown").is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn labels_survive_reopen() {
        let dir = temp_cache_dir("reopen");
        {
            let mut store = LabelStore::open(&dir).unwrap();
            store
                .insert_source("https://example.org/cco.ttl", cco_labels())
                .unwrap();
        }
        let store = LabelStore::open(&dir).unwrap();
        assert_eq!(
            store
                .lookup("https://www.commoncoreontologies.org/ont00000958")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        assert!(store.has_source("https://example.org/cco.ttl"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn has_source_is_false_for_unfetched_url() {
        let dir = temp_cache_dir("missing");
        let store = LabelStore::open(&dir).unwrap();
        assert!(!store.has_source("https://example.org/never-fetched.ttl"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn distinct_source_urls_produce_distinct_cache_entries() {
        let dir = temp_cache_dir("distinct_keys");
        let mut store = LabelStore::open(&dir).unwrap();
        store
            .insert_source("https://example.org/cco.ttl", cco_labels())
            .unwrap();
        assert!(store.has_source("https://example.org/cco.ttl"));
        assert!(
            !store.has_source("https://example.org/other.ttl"),
            "a different URL must not collide with the cached one"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_cache_file_is_skipped_not_fatal() {
        let dir = temp_cache_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("deadbeef.json"), "{not valid json").unwrap();

        let mut store = LabelStore::open(&dir).unwrap();
        store
            .insert_source("https://example.org/cco.ttl", cco_labels())
            .unwrap();
        assert_eq!(
            store
                .lookup("https://www.commoncoreontologies.org/ont00000958")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn extract_terms_prefers_rdfs_label_and_falls_back_to_skos() {
        let ttl = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
@prefix ex: <http://example.org/> .

ex:WithRdfs rdfs:label "Process" .
ex:WithSkos skos:prefLabel "Material Entity" .
ex:WithBoth rdfs:label "Primary" ; skos:prefLabel "Secondary" .
ex:Unlabeled a ex:Thing .
"#;
        let labels = extract_terms(ttl).unwrap();
        assert_eq!(labels.len(), 3, "unlabeled subject must be absent");
        assert_eq!(
            labels
                .get("http://example.org/WithRdfs")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        assert_eq!(
            labels
                .get("http://example.org/WithSkos")
                .and_then(|t| t.label.as_deref()),
            Some("Material Entity")
        );
        assert_eq!(
            labels
                .get("http://example.org/WithBoth")
                .and_then(|t| t.label.as_deref()),
            Some("Primary"),
            "rdfs:label wins over skos:prefLabel"
        );
    }

    #[test]
    fn extract_terms_takes_english_or_untagged_and_ignores_other_languages() {
        let ttl = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix ex: <http://example.org/> .

ex:GermanOnly rdfs:label "Prozess"@de .
ex:EnglishTagged rdfs:label "Process"@en .
ex:Untagged rdfs:label "Entity" .
"#;
        let labels = extract_terms(ttl).unwrap();
        assert!(
            !labels.contains_key("http://example.org/GermanOnly"),
            "non-English-only subject must be absent"
        );
        assert_eq!(
            labels
                .get("http://example.org/EnglishTagged")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        assert_eq!(
            labels
                .get("http://example.org/Untagged")
                .and_then(|t| t.label.as_deref()),
            Some("Entity")
        );
    }

    #[test]
    fn extract_terms_errors_on_malformed_ttl() {
        assert!(extract_terms("this is not turtle {{{").is_err());
    }

    #[test]
    fn extract_terms_collects_definitions_with_skos_priority() {
        let ttl = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix skos: <http://www.w3.org/2004/02/skos/core#> .
@prefix obo: <http://purl.obolibrary.org/obo/> .
@prefix ex: <http://example.org/> .

ex:CcoStyle rdfs:label "Process" ;
    skos:definition "A series of events." ;
    rdfs:comment "Lower-priority gloss." .
ex:OboStyle rdfs:label "continuant" ;
    obo:IAO_0000115 "An entity that persists through time." .
ex:CitoStyle rdfs:label "supports" ;
    rdfs:comment "One claim bears positively on another." .
ex:DefinitionOnly skos:definition "Defined but unlabeled." .
"#;
        let terms = extract_terms(ttl).unwrap();
        let def_of = |iri: &str| terms.get(iri).and_then(|t| t.definition.as_deref());
        assert_eq!(
            def_of("http://example.org/CcoStyle"),
            Some("A series of events."),
            "skos:definition wins over rdfs:comment"
        );
        assert_eq!(
            def_of("http://example.org/OboStyle"),
            Some("An entity that persists through time.")
        );
        assert_eq!(
            def_of("http://example.org/CitoStyle"),
            Some("One claim bears positively on another.")
        );
        // A definition without a label still produces an entry —
        // the tooltip is useful even when the link text stays a CURIE.
        let unlabeled = terms.get("http://example.org/DefinitionOnly").unwrap();
        assert!(unlabeled.label.is_none());
        assert_eq!(
            unlabeled.definition.as_deref(),
            Some("Defined but unlabeled.")
        );
    }

    #[test]
    fn extract_terms_falls_back_to_rdf_xml() {
        // OBO PURLs (bfo.owl, ro.owl, …) serve RDF/XML, not Turtle.
        let rdf_xml = r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#">
  <rdf:Description rdf:about="http://purl.obolibrary.org/obo/BFO_0000015">
    <rdfs:label>process</rdfs:label>
  </rdf:Description>
</rdf:RDF>"#;
        let labels = extract_terms(rdf_xml).unwrap();
        assert_eq!(
            labels
                .get("http://purl.obolibrary.org/obo/BFO_0000015")
                .and_then(|t| t.label.as_deref()),
            Some("process")
        );
    }

    /// Counting test double: records fetched URLs, serves canned TTL.
    struct CountingSource {
        responses: BTreeMap<String, Result<String, ()>>,
        fetched: std::cell::RefCell<Vec<String>>,
    }

    impl LabelSource for CountingSource {
        fn fetch(&self, url: &str) -> Result<String, LabelFetchError> {
            self.fetched.borrow_mut().push(url.to_string());
            match self.responses.get(url) {
                Some(Ok(body)) => Ok(body.clone()),
                _ => Err(LabelFetchError::Http {
                    url: url.to_string(),
                    message: "simulated failure".to_string(),
                }),
            }
        }
    }

    const CCO_TTL: &str = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
<https://www.commoncoreontologies.org/ont00000958> rdfs:label "Process" .
"#;

    fn schema_with_cco_prefix() -> crate::linkml::SchemaDefinition {
        let mut schema = crate::linkml::SchemaDefinition::new("s");
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        schema
            .prefixes
            .insert("local".to_string(), "https://example.org/own/".to_string());
        schema
    }

    fn cco_source_url() -> &'static str {
        BUILTIN_LABEL_SOURCES
            .iter()
            .find(|(ns, _)| *ns == "https://www.commoncoreontologies.org/")
            .map(|(_, url)| *url)
            .expect("CCO must be in the built-in map")
    }

    #[test]
    fn ensure_labels_fetches_known_prefix_and_skips_unknown() {
        let dir = temp_cache_dir("ensure_fetch");
        let mut store = LabelStore::open(&dir).unwrap();
        let source = CountingSource {
            responses: BTreeMap::from([(cco_source_url().to_string(), Ok(CCO_TTL.to_string()))]),
            fetched: Default::default(),
        };

        ensure_labels(&schema_with_cco_prefix(), &mut store, &source);

        assert_eq!(
            source.fetched.borrow().as_slice(),
            &[cco_source_url().to_string()],
            "exactly the CCO source fetched; the unknown `local` prefix skipped"
        );
        assert_eq!(
            store
                .lookup("https://www.commoncoreontologies.org/ont00000958")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_labels_does_not_refetch_cached_sources() {
        let dir = temp_cache_dir("ensure_cached");
        let mut store = LabelStore::open(&dir).unwrap();
        store.insert_source(cco_source_url(), cco_labels()).unwrap();
        let source = CountingSource {
            responses: BTreeMap::new(),
            fetched: Default::default(),
        };

        ensure_labels(&schema_with_cco_prefix(), &mut store, &source);

        assert!(
            source.fetched.borrow().is_empty(),
            "cache hit must not trigger a fetch"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn ensure_labels_survives_per_source_fetch_failure() {
        let dir = temp_cache_dir("ensure_failopen");
        let mut store = LabelStore::open(&dir).unwrap();
        let mut schema = schema_with_cco_prefix();
        schema
            .prefixes
            .insert("prov".to_string(), "http://www.w3.org/ns/prov#".to_string());
        // CCO fetch fails; prov succeeds.
        let prov_url = BUILTIN_LABEL_SOURCES
            .iter()
            .find(|(ns, _)| *ns == "http://www.w3.org/ns/prov#")
            .map(|(_, url)| *url)
            .unwrap();
        let prov_ttl = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
<http://www.w3.org/ns/prov#Activity> rdfs:label "Activity" .
"#;
        let source = CountingSource {
            responses: BTreeMap::from([(prov_url.to_string(), Ok(prov_ttl.to_string()))]),
            fetched: Default::default(),
        };

        ensure_labels(&schema, &mut store, &source);

        assert_eq!(
            store
                .lookup("http://www.w3.org/ns/prov#Activity")
                .and_then(|t| t.label.as_deref()),
            Some("Activity"),
            "prov labels land despite the CCO failure"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn builtin_label_sources_have_wellformed_https_urls() {
        for (namespace, url) in BUILTIN_LABEL_SOURCES {
            assert!(
                namespace.starts_with("http"),
                "namespace {namespace} must be an IRI"
            );
            assert!(
                url.starts_with("https://") || url.starts_with("http://"),
                "source URL {url} must be fetchable"
            );
        }
    }

    #[test]
    fn lookup_searches_across_multiple_sources() {
        let dir = temp_cache_dir("multi");
        let mut store = LabelStore::open(&dir).unwrap();
        store
            .insert_source("https://example.org/cco.ttl", cco_labels())
            .unwrap();
        store
            .insert_source(
                "https://example.org/prov.ttl",
                BTreeMap::from([(
                    "http://www.w3.org/ns/prov#Activity".to_string(),
                    term("Activity"),
                )]),
            )
            .unwrap();
        assert_eq!(
            store
                .lookup("http://www.w3.org/ns/prov#Activity")
                .and_then(|t| t.label.as_deref()),
            Some("Activity")
        );
        assert_eq!(
            store
                .lookup("https://www.commoncoreontologies.org/ont00000958")
                .and_then(|t| t.label.as_deref()),
            Some("Process")
        );
        let _ = fs::remove_dir_all(dir);
    }
}
