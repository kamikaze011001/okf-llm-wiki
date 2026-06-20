use crate::core::embed::Embedder;
use crate::core::index_store::{flatten, Chunk, PersistedIndex};
use anyhow::Result;

const DIM: usize = 256;

/// Deterministic local embedding: hashes word tokens into a fixed vector.
/// Keeps v1 fully offline and provider-independent for search.
pub fn hash_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; DIM];
    for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let mut h: u64 = 1469598103934665603;
        for b in word.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        v[(h as usize) % DIM] += 1.0;
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}

/// Similarity score between two vectors. `zip` truncates to the shorter slice, so
/// callers must pass equal-length vectors — `search` enforces this by filtering out
/// chunks whose stored dimension differs from the query embedding.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Target maximum size (in characters) for a single chunk.
pub const MAX_CHUNK_CHARS: usize = 800;

/// Split a page body into retrieval chunks.
///
/// Paragraphs are delimited by blank lines. Markdown headings (`#`-prefixed lines)
/// also start a new paragraph. Paragraphs are greedily packed into chunks up to
/// `MAX_CHUNK_CHARS`; a paragraph is never split across chunks. A single paragraph
/// longer than the limit becomes its own (oversized) chunk. An empty/whitespace-only
/// body yields no chunks.
pub fn chunk_body(body: &str) -> Vec<String> {
    let paragraphs = split_paragraphs(body);
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for para in paragraphs {
        if current.is_empty() {
            current = para;
        } else if current.chars().count() + 2 + para.chars().count() <= MAX_CHUNK_CHARS {
            current.push_str("\n\n");
            current.push_str(&para);
        } else {
            chunks.push(std::mem::take(&mut current));
            current = para;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Group raw lines into trimmed paragraphs. Blank lines separate paragraphs; a
/// heading line (starts with `#`) forces a boundary before itself.
fn split_paragraphs(body: &str) -> Vec<String> {
    let mut paras: Vec<String> = Vec::new();
    let mut buf: Vec<&str> = Vec::new();
    let flush = |buf: &mut Vec<&str>, paras: &mut Vec<String>| {
        if !buf.is_empty() {
            let joined = buf.join("\n");
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                paras.push(trimmed.to_string());
            }
            buf.clear();
        }
    };
    for line in body.lines() {
        if line.trim().is_empty() {
            flush(&mut buf, &mut paras);
        } else if line.trim_start().starts_with('#') {
            flush(&mut buf, &mut paras);
            buf.push(line);
            flush(&mut buf, &mut paras);
        } else {
            buf.push(line);
        }
    }
    flush(&mut buf, &mut paras);
    paras
}

/// Embed the query and return the top-`k` chunks by cosine similarity.
///
/// Chunks whose stored vector dimension differs from the freshly embedded query
/// (a stale index built by a different embedder) are skipped rather than scored —
/// no panic, they simply don't match. Ties break by `(path, chunk_id)` for stable
/// ordering.
pub async fn search<'a>(
    embedder: &dyn Embedder,
    query: &str,
    index: &'a PersistedIndex,
    k: usize,
) -> Result<Vec<&'a Chunk>> {
    let q = embedder.embed(query).await?;
    let mut scored: Vec<(f32, &Chunk)> = flatten(index)
        .into_iter()
        .filter(|c| c.vector.len() == q.len())
        .map(|c| (cosine(&q, &c.vector), c))
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (a.1.path.as_str(), a.1.chunk_id).cmp(&(b.1.path.as_str(), b.1.chunk_id)))
    });
    Ok(scored.into_iter().take(k).map(|(_, c)| c).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_yields_no_chunks() {
        assert!(chunk_body("").is_empty());
        assert!(chunk_body("   \n\n  ").is_empty());
    }

    #[test]
    fn short_body_is_one_chunk() {
        let chunks = chunk_body("para one\n\npara two");
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("para one"));
        assert!(chunks[0].contains("para two"));
    }

    #[test]
    fn packs_paragraphs_up_to_limit_without_splitting() {
        let p = "x".repeat(500);
        let chunks = chunk_body(&format!("{p}\n\n{p}"));
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 500);
        assert_eq!(chunks[1].chars().count(), 500);
    }

    #[test]
    fn oversized_paragraph_becomes_its_own_chunk() {
        let big = "y".repeat(2000);
        let chunks = chunk_body(&big);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chars().count(), 2000);
    }

    #[test]
    fn headings_start_new_paragraph_boundaries() {
        let body = "intro text\n\n## Section\n\nbody text";
        let chunks = chunk_body(body);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("## Section"));
    }

    #[tokio::test]
    async fn search_ranks_by_cosine_and_skips_dim_mismatch() {
        use crate::core::embed::{Embedder, HashEmbedder};
        use crate::core::index_store::{Chunk, PageEntry, PersistedIndex};

        let e = HashEmbedder;
        let mut idx = PersistedIndex {
            embedder_id: e.id(),
            ..Default::default()
        };
        // relevant chunk: embed the same phrase we will query
        let good = hash_embed("vitamin d improves sleep");
        // a stale chunk with the WRONG dimension must be skipped, not panic
        let stale = vec![0.5f32; 8];
        idx.pages.insert(
            "concepts/vd.md".into(),
            PageEntry {
                content_hash: 1,
                chunks: vec![
                    Chunk {
                        path: "concepts/vd.md".into(),
                        chunk_id: 0,
                        text: "vitamin d improves sleep".into(),
                        vector: good,
                    },
                    Chunk {
                        path: "concepts/vd.md".into(),
                        chunk_id: 1,
                        text: "stale".into(),
                        vector: stale,
                    },
                ],
            },
        );
        idx.pages.insert(
            "concepts/rust.md".into(),
            PageEntry {
                content_hash: 1,
                chunks: vec![Chunk {
                    path: "concepts/rust.md".into(),
                    chunk_id: 0,
                    text: "rust tauri desktop".into(),
                    vector: hash_embed("rust tauri desktop"),
                }],
            },
        );

        let hits = search(&e, "vitamin d improves sleep", &idx, 2)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "concepts/vd.md");
        assert_eq!(hits[0].chunk_id, 0);
        // the dim-mismatched "stale" chunk must never be returned
        assert!(hits.iter().all(|h| h.text != "stale"));
    }
}
