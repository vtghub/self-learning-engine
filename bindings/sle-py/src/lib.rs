//! Python bindings for the engine core, built with PyO3/maturin. This is
//! the "language-agnostic on the consumption side" story made concrete:
//! everything here is a thin wrapper over the same `sle-core` traits and
//! `Forest` that the Rust CLI drives directly — no logic is duplicated.

use std::path::Path;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use sle_core::{Adapter, EdgeKey, EdgeKind, EmbeddingProvider, EventKind, Forest, Hlc, NodeId};
use sle_embeddings::{apply_proximity_links, scan_proximity, HashingEmbeddingProvider};

fn to_pyerr(e: anyhow::Error) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn adapter_for(path: &Path) -> Option<Box<dyn Adapter>> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => sle_adapters_code::CodeAdapter::for_extension("rs").map(|a| Box::new(a) as Box<dyn Adapter>),
        Some("py") => sle_adapters_code::CodeAdapter::for_extension("py").map(|a| Box::new(a) as Box<dyn Adapter>),
        Some("md") | Some("txt") => Some(Box::new(sle_adapters_text::TextAdapter)),
        _ => None,
    }
}

/// One replica's forest, exposed to Python. Node/edge ids cross the FFI
/// boundary as plain strings ("replica.local" for nodes) rather than a
/// bespoke Python type, so callers can store/compare/hash them trivially.
#[pyclass]
struct Engine {
    forest: Forest,
}

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (replica_id=1))]
    fn new(replica_id: u64) -> Self {
        Self { forest: Forest::new(replica_id) }
    }

    /// Ingest a file, picking an adapter by extension (.rs/.py/.md/.txt).
    /// Returns the ingested tree's root node id.
    fn ingest(&mut self, path: String) -> PyResult<String> {
        let p = Path::new(&path);
        let adapter = adapter_for(p).ok_or_else(|| PyValueError::new_err(format!("no adapter for {path}")))?;
        let content = std::fs::read_to_string(p).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let tree_id = p.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string();
        let root = adapter.ingest(&tree_id, &content, &mut self.forest).map_err(to_pyerr)?;
        Ok(root.to_string())
    }

    /// Embed every content-bearing node (hashing-trick, zero-dependency) and
    /// materialize `SimilarTo` edges between nodes scoring above
    /// `threshold`. Returns how many links were found.
    #[pyo3(signature = (threshold=0.5, dims=128))]
    fn scan_proximity(&mut self, threshold: f32, dims: usize) -> PyResult<usize> {
        let embedder = HashingEmbeddingProvider::new(dims);
        let targets: Vec<(NodeId, String)> = self
            .forest
            .nodes()
            .filter(|(_, p)| !matches!(p.kind.to_string().as_str(), "Module" | "Document"))
            .map(|(id, p)| {
                let basis = if p.text.trim().is_empty() { p.label.clone() } else { p.text.clone() };
                (*id, basis)
            })
            .collect();
        for (id, basis) in targets {
            self.forest.set_embedding(id, embedder.embed(&basis));
        }
        let links = scan_proximity(&self.forest, threshold);
        let count = links.len();
        apply_proximity_links(&mut self.forest, &links);
        Ok(count)
    }

    fn stats(&self) -> (usize, usize) {
        (self.forest.node_count(), self.forest.edge_count())
    }

    /// (dst_node_id, edge_kind, weight) for every outgoing edge of `node_id`.
    fn query_neighbors(&self, node_id: String) -> PyResult<Vec<(String, String, i64)>> {
        let id: NodeId = node_id.parse().map_err(to_pyerr)?;
        Ok(self
            .forest
            .edges_from(&id)
            .into_iter()
            .map(|(k, w)| (k.dst.to_string(), k.kind.to_string(), w))
            .collect())
    }

    /// Report a usage event — one of Accessed, EditedTogether,
    /// SuggestionAccepted, SuggestionRejected, TestPassed, TestFailed — on
    /// the edge (src, dst, kind). Returns the edge's weight afterward.
    fn report_event(&mut self, src: String, dst: String, kind: String, event: String) -> PyResult<i64> {
        let src: NodeId = src.parse().map_err(to_pyerr)?;
        let dst: NodeId = dst.parse().map_err(to_pyerr)?;
        let event: EventKind = event.parse().map_err(to_pyerr)?;
        let key = EdgeKey { src, dst, kind: EdgeKind::new(kind) };
        self.forest.report_event(&key, event);
        Ok(self.forest.weight(&key))
    }

    /// Serialize this engine's full operation log as JSON bytes — hand it
    /// to another `Engine`'s `import_delta` to sync them (order-independent,
    /// safe to call more than once; see `sle-sync`'s convergence tests).
    fn export_delta(&self) -> PyResult<Vec<u8>> {
        serde_json::to_vec(&self.forest.log().since(Hlc::ZERO)).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn import_delta(&mut self, bytes: Vec<u8>) -> PyResult<()> {
        let ops: Vec<sle_core::StampedOp> =
            serde_json::from_slice(&bytes).map_err(|e| PyValueError::new_err(e.to_string()))?;
        self.forest.apply_all(ops);
        Ok(())
    }
}

#[pymodule]
fn sle_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    Ok(())
}
