use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub version: String,
    pub description: String,
    pub abstract_text: String,
    pub namespaces: Vec<(String, String)>,
    pub classes: Vec<Entity>,
    pub object_properties: Vec<Entity>,
    pub data_properties: Vec<Entity>,
    pub annotation_properties: Vec<Entity>,
    pub named_individuals: Vec<Entity>,
}

#[derive(Clone)]
pub struct Entity {
    pub id: String,
    pub label: String,
    pub iri: String,
    pub comment: String,
    pub superclasses: Vec<TermRef>, // (IRI, Label)
    pub disjoints: Vec<TermRef>,    // (IRI, Label)
}

#[derive(Clone)]
pub struct TermRef {
    pub iri: String,
    pub label: String,
}
