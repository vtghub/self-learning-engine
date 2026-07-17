//! Pluggable embedding backends and the proximity scanner that uses them to
//! find (and link) semantically related nodes across trees.
//!
//! The default `HashingEmbeddingProvider` is a zero-dependency, deterministic
//! feature-hashing + random-projection embedding: no model download, no
//! network call, fully reproducible. It's a legitimate lightweight embedding
//! technique (related to "random indexing"), good enough to demonstrate and
//! test proximity detection. A real sentence-embedding model can implement
//! the same `EmbeddingProvider` trait as a drop-in replacement with no
//! changes anywhere else in the engine.

use sle_core::{EdgeKey, EdgeKind, EmbeddingProvider, Forest, NodeId};

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

pub struct HashingEmbeddingProvider {
    dims: usize,
}

impl HashingEmbeddingProvider {
    pub fn new(dims: usize) -> Self {
        Self { dims }
    }
}

impl Default for HashingEmbeddingProvider {
    fn default() -> Self {
        Self::new(64)
    }
}

impl EmbeddingProvider for HashingEmbeddingProvider {
    fn dims(&self) -> usize {
        self.dims
    }

    /// Classic "hashing trick" (feature hashing): each token is hashed
    /// straight to one of `dims` buckets with a hashed sign, and buckets
    /// accumulate token counts. Two texts sharing vocabulary land hits on
    /// the same buckets with the same sign, so their cosine similarity
    /// tracks vocabulary overlap directly — unlike spreading each token
    /// across every dimension, collisions here only add noise on the
    /// dimensions unrelated texts don't share.
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut acc = vec![0f32; self.dims];
        for token in tokenize(text) {
            let h = fnv1a(token.as_bytes());
            let idx = (h % self.dims as u64) as usize;
            let sign = if (h >> 63) & 1 == 0 { 1.0 } else { -1.0 };
            acc[idx] += sign;
        }
        let norm: f32 = acc.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for slot in acc.iter_mut() {
                *slot /= norm;
            }
        }
        acc
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

#[derive(Debug, Clone)]
pub struct ProximityLink {
    pub a: NodeId,
    pub b: NodeId,
    pub score: f32,
}

/// Brute-force nearest-neighbor scan over every embedded node in the forest.
/// Fine at demo scale (dozens-to-low-thousands of nodes); a real deployment
/// would swap this for an ANN index (e.g. HNSW) behind the same signature —
/// the rest of the engine only depends on getting back scored node pairs.
pub fn scan_proximity(forest: &Forest, threshold: f32) -> Vec<ProximityLink> {
    let embedded: Vec<(NodeId, &Vec<f32>)> = forest
        .nodes()
        .filter_map(|(id, payload)| payload.embedding.as_ref().map(|e| (*id, e)))
        .collect();

    let mut links = Vec::new();
    for i in 0..embedded.len() {
        for j in (i + 1)..embedded.len() {
            let (a, ea) = embedded[i];
            let (b, eb) = embedded[j];
            let score = cosine_similarity(ea, eb);
            if score >= threshold {
                links.push(ProximityLink { a, b, score });
            }
        }
    }
    links
}

/// Materializes proximity links as symmetric `SimilarTo` edges in the
/// forest, with the similarity score (scaled to an integer) as the initial
/// weight. This is the step that "connects trees when they are in
/// proximity": once applied, these edges participate in the same
/// reinforcement/decay learning loop as any structural edge.
pub fn apply_proximity_links(forest: &mut Forest, links: &[ProximityLink]) {
    let kind = EdgeKind::new("SimilarTo");
    for link in links {
        let weight = (link.score * 100.0).round() as i64;
        forest.add_edge(EdgeKey { src: link.a, dst: link.b, kind: kind.clone() }, weight);
        forest.add_edge(EdgeKey { src: link.b, dst: link.a, kind: kind.clone() }, weight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_maximally_similar() {
        let p = HashingEmbeddingProvider::new(32);
        let a = p.embed("authenticate the user with a password token");
        let b = p.embed("authenticate the user with a password token");
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn shared_vocabulary_scores_higher_than_unrelated_text() {
        let p = HashingEmbeddingProvider::new(64);
        let auth_a = p.embed("verify user credentials and issue an auth token");
        let auth_b = p.embed("the login flow checks the password token before granting access");
        let unrelated = p.embed("bake bread with flour yeast water and salt");

        let related_score = cosine_similarity(&auth_a, &auth_b);
        let unrelated_score = cosine_similarity(&auth_a, &unrelated);
        assert!(
            related_score > unrelated_score,
            "expected shared-vocabulary text to score higher: related={related_score} unrelated={unrelated_score}"
        );
    }
}
