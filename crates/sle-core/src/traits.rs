use crate::forest::Forest;
use crate::id::{NodeId, TreeId};
use crate::model::DomainTag;
use crate::ops::StampedOp;

/// A pluggable ingestion source. One `Adapter` implementation per input
/// domain/language (e.g. tree-sitter-based Rust/Python code, Markdown
/// prose). The core never knows about any specific language or file
/// format — it only calls this trait.
pub trait Adapter {
    fn domain(&self) -> DomainTag;

    /// Parse `source` and add its nodes/edges into `forest` under `tree_id`.
    /// Returns the id of the tree's root node.
    fn ingest(&self, tree_id: &TreeId, source: &str, forest: &mut Forest) -> anyhow::Result<NodeId>;
}

/// A pluggable embedding backend. The default (`sle-embeddings`'s
/// `HashingEmbeddingProvider`) is a zero-dependency deterministic
/// feature-hashing embedding; a real sentence-embedding model can implement
/// the same trait with no core changes.
pub trait EmbeddingProvider {
    fn dims(&self) -> usize;
    fn embed(&self, text: &str) -> Vec<f32>;
}

/// A pluggable distribution transport. `sle-sync`'s `LocalSyncProvider` is
/// an in-process reference implementation; a networked implementation
/// (gRPC/QUIC gossip) would implement the same trait without any change to
/// `Forest` or its CRDT merge rules.
pub trait SyncProvider {
    fn export_since(&self, forest: &Forest, since_hlc: crate::hlc::Hlc) -> Vec<StampedOp>;
    fn import(&self, forest: &mut Forest, ops: Vec<StampedOp>);
}

/// Usage signals the host application reports back to the engine. These are
/// what drive the "self-updating" half of the engine, independent of
/// re-parsing: an edge that keeps getting exercised is reinforced, one that
/// gets contradicted is decayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Accessed,
    EditedTogether,
    SuggestionAccepted,
    SuggestionRejected,
    TestPassed,
    TestFailed,
}

impl std::str::FromStr for EventKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Accessed" => Ok(EventKind::Accessed),
            "EditedTogether" => Ok(EventKind::EditedTogether),
            "SuggestionAccepted" => Ok(EventKind::SuggestionAccepted),
            "SuggestionRejected" => Ok(EventKind::SuggestionRejected),
            "TestPassed" => Ok(EventKind::TestPassed),
            "TestFailed" => Ok(EventKind::TestFailed),
            other => Err(anyhow::anyhow!("unknown event kind: {other}")),
        }
    }
}

impl EventKind {
    /// Positive events reinforce by this much, negative events decay by
    /// this much. Kept as a simple fixed step for v1; a smarter learning
    /// rule can replace this without touching callers.
    pub fn magnitude(&self) -> (bool, u64) {
        match self {
            EventKind::Accessed => (true, 1),
            EventKind::EditedTogether => (true, 2),
            EventKind::SuggestionAccepted => (true, 3),
            EventKind::TestPassed => (true, 2),
            EventKind::SuggestionRejected => (false, 3),
            EventKind::TestFailed => (false, 2),
        }
    }
}
