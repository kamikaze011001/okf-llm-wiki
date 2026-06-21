use crate::core::slug::slugify;
use crate::core::store::OkfStore;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub text: String,
    pub target_slug: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    Text(String),
    Link { text: String, target_slug: String },
}

/// Split a body into ordered `Text` and `Link` runs.
///
/// A link is `[[inner]]` where `inner` trims to non-empty and slugifies to non-empty.
/// Empty (`[[]]`), whitespace-only, and unbalanced (`[[` with no closing `]]`) brackets are
/// left as plain text. The scan only matches ASCII `[`/`]`, so slice boundaries are always
/// valid UTF-8 and it never panics.
pub fn segment_body(body: &str) -> Vec<Segment> {
    let bytes = body.as_bytes();
    let mut segments = Vec::new();
    let mut text_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let opens = bytes[i] == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[';
        if opens {
            if let Some(rel) = body[i + 2..].find("]]") {
                let inner_raw = &body[i + 2..i + 2 + rel];
                let inner = inner_raw.trim();
                let slug = slugify(inner);
                // Reject if the inner span itself contains [[  — that means the opening
                // [[ was really an unbalanced bracket, not the start of a valid link.
                if !inner.is_empty() && !slug.is_empty() && !inner_raw.contains("[[") {
                    if text_start < i {
                        segments.push(Segment::Text(body[text_start..i].to_string()));
                    }
                    segments.push(Segment::Link {
                        text: inner.to_string(),
                        target_slug: slug,
                    });
                    i = i + 2 + rel + 2;
                    text_start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if text_start < body.len() {
        segments.push(Segment::Text(body[text_start..].to_string()));
    }
    segments
}

/// Every `[[link]]` in the body, in order.
pub fn extract_links(body: &str) -> Vec<Link> {
    segment_body(body)
        .into_iter()
        .filter_map(|s| match s {
            Segment::Link { text, target_slug } => Some(Link { text, target_slug }),
            Segment::Text(_) => None,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct BacklinkRef {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConceptRef {
    pub slug: String,
    pub title: String,
}

/// A concept page as a graph node. `degree` counts undirected `[[link]]` edges touching it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub path: String,
    pub title: String,
    pub degree: usize,
}

/// An undirected `[[link]]` edge between two existing pages. `source`/`target` are page
/// paths ordered lexically — the direction is not semantically meaningful.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
}

/// The whole-wiki concept graph: every existing page plus its deduped link edges.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Resolved link graph for the whole wiki. Built from disk, held in memory.
#[derive(Debug, Clone, Default)]
pub struct LinkGraph {
    slug_to_path: HashMap<String, String>,
    slug_to_title: HashMap<String, String>,
    /// target slug -> source page paths that link to it
    backlinks: HashMap<String, Vec<String>>,
}

/// Rewrite the body so every `[[X]]` whose `slugify(X)` is not in `known` becomes plain `X`.
/// Known links are re-emitted as `[[trimmed text]]`. Pure and deterministic.
pub fn validate_links(body: &str, known: &HashSet<String>) -> String {
    let mut out = String::new();
    for seg in segment_body(body) {
        match seg {
            Segment::Text(t) => out.push_str(&t),
            Segment::Link { text, target_slug } => {
                if known.contains(&target_slug) {
                    out.push_str("[[");
                    out.push_str(&text);
                    out.push_str("]]");
                } else {
                    out.push_str(&text);
                }
            }
        }
    }
    out
}

/// Filename stem of a page path: `concepts/vitamin-d-sleep.md` -> `vitamin-d-sleep`.
pub fn slug_of(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.strip_suffix(".md").unwrap_or(file)
}

impl LinkGraph {
    pub fn path_for(&self, slug: &str) -> Option<&str> {
        self.slug_to_path.get(slug).map(String::as_str)
    }

    pub fn exists(&self, slug: &str) -> bool {
        self.slug_to_path.contains_key(slug)
    }

    /// The whole-wiki concept graph. Nodes are all existing pages (orphans included,
    /// degree 0). Edges are `[[link]]`s between existing pages: undirected, deduplicated
    /// (a mutual A<->B link is one edge), self-links and unresolved targets excluded.
    /// Nodes and edges are sorted by path for deterministic output.
    pub fn graph_data(&self) -> GraphData {
        // Collect undirected, deduped edges keyed by the lexically-ordered pair of paths.
        let mut pairs: HashSet<(String, String)> = HashSet::new();
        for (target_slug, sources) in &self.backlinks {
            let Some(target_path) = self.slug_to_path.get(target_slug) else {
                continue; // unresolved target
            };
            for source_path in sources {
                if source_path == target_path {
                    continue; // self-link (defensive)
                }
                let pair = if source_path <= target_path {
                    (source_path.clone(), target_path.clone())
                } else {
                    (target_path.clone(), source_path.clone())
                };
                pairs.insert(pair);
            }
        }

        // Degree per page from the deduped pairs.
        let mut degree: HashMap<String, usize> = HashMap::new();
        for (a, b) in &pairs {
            *degree.entry(a.clone()).or_default() += 1;
            *degree.entry(b.clone()).or_default() += 1;
        }

        let mut nodes: Vec<GraphNode> = self
            .slug_to_path
            .iter()
            .map(|(slug, path)| GraphNode {
                path: path.clone(),
                title: self
                    .slug_to_title
                    .get(slug)
                    .cloned()
                    .unwrap_or_else(|| slug.clone()),
                degree: degree.get(path).copied().unwrap_or(0),
            })
            .collect();
        nodes.sort_by(|a, b| a.path.cmp(&b.path));

        let mut edges: Vec<GraphEdge> = pairs
            .into_iter()
            .map(|(source, target)| GraphEdge { source, target })
            .collect();
        edges.sort_by(|a, b| {
            (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
        });

        GraphData { nodes, edges }
    }

    /// Pages that link to `path`, resolved to `{path, title}`, sorted by path.
    pub fn backlinks(&self, path: &str) -> Vec<BacklinkRef> {
        let slug = slug_of(path);
        let mut sources = self.backlinks.get(slug).cloned().unwrap_or_default();
        sources.sort();
        sources
            .into_iter()
            .map(|p| {
                let s = slug_of(&p);
                let title = self
                    .slug_to_title
                    .get(s)
                    .cloned()
                    .unwrap_or_else(|| s.to_string());
                BacklinkRef { path: p, title }
            })
            .collect()
    }
}

/// Read every page and build the slug->path/title maps plus the backlink index.
/// Self-links and links to non-existent slugs produce no backlink.
pub fn build_link_graph(store: &OkfStore) -> Result<LinkGraph> {
    let mut graph = LinkGraph::default();
    let paths = store.list_pages()?;

    let mut bodies: Vec<(String, String)> = Vec::with_capacity(paths.len());
    for path in &paths {
        let page = store.read_page(path)?;
        let slug = slug_of(path).to_string();
        let title = page
            .frontmatter
            .title
            .clone()
            .unwrap_or_else(|| slug.clone());
        graph.slug_to_path.insert(slug.clone(), path.clone());
        graph.slug_to_title.insert(slug, title);
        bodies.push((path.clone(), page.body));
    }

    for (path, body) in &bodies {
        let source_slug = slug_of(path);
        let mut seen = HashSet::new();
        for link in extract_links(body) {
            if link.target_slug == source_slug {
                continue; // self-link
            }
            if !graph.slug_to_path.contains_key(&link.target_slug) {
                continue; // unresolved target
            }
            if seen.insert(link.target_slug.clone()) {
                graph
                    .backlinks
                    .entry(link.target_slug.clone())
                    .or_default()
                    .push(path.clone());
            }
        }
    }

    Ok(graph)
}

/// Lightweight list of every existing concept (slug + title), for the digest allow-list.
pub fn concept_refs(store: &OkfStore) -> Result<Vec<ConceptRef>> {
    let mut refs = Vec::new();
    for path in store.list_pages()? {
        let page = store.read_page(&path)?;
        let slug = slug_of(&path).to_string();
        let title = page
            .frontmatter
            .title
            .clone()
            .unwrap_or_else(|| slug.clone());
        refs.push(ConceptRef { slug, title });
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::page::{Frontmatter, Page};
    use crate::core::store::OkfStore;
    use std::collections::BTreeMap;

    fn tmp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("okf-links-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn page(path: &str, title: &str, body: &str) -> Page {
        Page {
            path: path.into(),
            frontmatter: Frontmatter {
                type_: "Concept".into(),
                title: Some(title.into()),
                description: None,
                tags: vec![],
                resource: None,
                timestamp: None,
                note: None,
                extra: BTreeMap::new(),
            },
            body: body.into(),
        }
    }

    #[test]
    fn slug_of_strips_dir_and_extension() {
        assert_eq!(slug_of("concepts/vitamin-d-sleep.md"), "vitamin-d-sleep");
        assert_eq!(slug_of("alpha.md"), "alpha");
    }

    #[test]
    fn backlinks_resolve_and_exclude_self() {
        let store = OkfStore::new(tmp());
        store
            .write_page(&page(
                "concepts/alpha.md",
                "Alpha",
                "I mention [[Alpha]] myself.",
            ))
            .unwrap();
        store
            .write_page(&page(
                "concepts/beta.md",
                "Beta",
                "Beta links to [[Alpha]] and [[Ghost]].",
            ))
            .unwrap();
        let graph = build_link_graph(&store).unwrap();

        assert!(graph.exists("alpha"));
        assert_eq!(graph.path_for("alpha"), Some("concepts/alpha.md"));
        assert!(!graph.exists("ghost"));

        let back = graph.backlinks("concepts/alpha.md");
        // Beta links Alpha; Alpha's self-link is excluded; Ghost is unresolved.
        assert_eq!(
            back,
            vec![BacklinkRef {
                path: "concepts/beta.md".into(),
                title: "Beta".into()
            }]
        );
        assert!(graph.backlinks("concepts/beta.md").is_empty());
    }

    #[test]
    fn concept_refs_lists_slug_and_title() {
        let store = OkfStore::new(tmp());
        store
            .write_page(&page("concepts/alpha.md", "Alpha", "body"))
            .unwrap();
        let refs = concept_refs(&store).unwrap();
        assert_eq!(
            refs,
            vec![ConceptRef {
                slug: "alpha".into(),
                title: "Alpha".into()
            }]
        );
    }

    #[test]
    fn segments_text_and_links_in_order() {
        let segs = segment_body("See [[Vitamin D & Sleep]] today.");
        assert_eq!(
            segs,
            vec![
                Segment::Text("See ".into()),
                Segment::Link {
                    text: "Vitamin D & Sleep".into(),
                    target_slug: "vitamin-d-sleep".into()
                },
                Segment::Text(" today.".into()),
            ]
        );
    }

    #[test]
    fn body_with_no_links_is_one_text_segment() {
        assert_eq!(
            segment_body("plain body"),
            vec![Segment::Text("plain body".into())]
        );
    }

    #[test]
    fn adjacent_links_have_no_empty_text_between() {
        let segs = segment_body("[[Alpha]][[Beta]]");
        assert_eq!(
            segs,
            vec![
                Segment::Link {
                    text: "Alpha".into(),
                    target_slug: "alpha".into()
                },
                Segment::Link {
                    text: "Beta".into(),
                    target_slug: "beta".into()
                },
            ]
        );
    }

    #[test]
    fn unbalanced_and_empty_brackets_stay_text() {
        // No closing ]], empty inner, whitespace-only inner -> all plain text, no panic.
        assert_eq!(
            segment_body("a [[unclosed and [[]] and [[   ]] b"),
            vec![Segment::Text("a [[unclosed and [[]] and [[   ]] b".into())]
        );
    }

    #[test]
    fn extract_links_returns_only_links() {
        let links = extract_links("x [[Alpha]] y [[Beta Gamma]] z");
        assert_eq!(
            links,
            vec![
                Link {
                    text: "Alpha".into(),
                    target_slug: "alpha".into()
                },
                Link {
                    text: "Beta Gamma".into(),
                    target_slug: "beta-gamma".into()
                },
            ]
        );
    }

    #[test]
    fn trims_inner_whitespace_for_slug_and_text() {
        let segs = segment_body("[[  Spaced Title  ]]");
        assert_eq!(
            segs,
            vec![Segment::Link {
                text: "Spaced Title".into(),
                target_slug: "spaced-title".into()
            }]
        );
    }

    #[test]
    fn validate_keeps_known_unwraps_unknown() {
        let known: std::collections::HashSet<String> =
            ["vitamin-d-sleep".to_string()].into_iter().collect();
        let out = validate_links("See [[Vitamin D & Sleep]] and [[Made Up]].", &known);
        assert_eq!(out, "See [[Vitamin D & Sleep]] and Made Up.");
    }

    #[test]
    fn validate_leaves_plain_text_untouched() {
        let known = std::collections::HashSet::new();
        assert_eq!(validate_links("no links here", &known), "no links here");
    }

    #[test]
    fn validate_is_case_insensitive_via_slug() {
        let known: std::collections::HashSet<String> = ["alpha".to_string()].into_iter().collect();
        assert_eq!(validate_links("[[ALPHA]]", &known), "[[ALPHA]]");
    }

    #[test]
    fn graph_data_nodes_edges_and_degree() {
        let store = OkfStore::new(tmp());
        store
            .write_page(&page(
                "concepts/alpha.md",
                "Alpha",
                "links [[Beta]] and [[Gamma]], and itself [[Alpha]].",
            ))
            .unwrap();
        store
            .write_page(&page(
                "concepts/beta.md",
                "Beta",
                "links back to [[Alpha]].",
            ))
            .unwrap();
        store
            .write_page(&page("concepts/gamma.md", "Gamma", "no links here."))
            .unwrap();
        store
            .write_page(&page(
                "concepts/orphan.md",
                "Orphan",
                "alone, mentions [[Ghost]].",
            ))
            .unwrap();
        let data = build_link_graph(&store).unwrap().graph_data();

        // All four existing pages are nodes (orphan included), sorted by path.
        let node_paths: Vec<&str> = data.nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            node_paths,
            vec![
                "concepts/alpha.md",
                "concepts/beta.md",
                "concepts/gamma.md",
                "concepts/orphan.md",
            ]
        );

        // Edges: Alpha-Beta is mutual -> one edge; Alpha-Gamma; no edge to unresolved
        // Ghost; no self-loop from Alpha's own [[Alpha]].
        assert_eq!(
            data.edges,
            vec![
                GraphEdge {
                    source: "concepts/alpha.md".into(),
                    target: "concepts/beta.md".into()
                },
                GraphEdge {
                    source: "concepts/alpha.md".into(),
                    target: "concepts/gamma.md".into()
                },
            ]
        );

        // Degrees from the deduped edge set.
        let deg = |p: &str| data.nodes.iter().find(|n| n.path == p).unwrap().degree;
        assert_eq!(deg("concepts/alpha.md"), 2);
        assert_eq!(deg("concepts/beta.md"), 1);
        assert_eq!(deg("concepts/gamma.md"), 1);
        assert_eq!(deg("concepts/orphan.md"), 0);
    }

    #[test]
    fn graph_data_empty_store_is_empty() {
        let store = OkfStore::new(tmp());
        let data = build_link_graph(&store).unwrap().graph_data();
        assert!(data.nodes.is_empty());
        assert!(data.edges.is_empty());
    }
}
