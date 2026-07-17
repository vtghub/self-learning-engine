use serde::{Deserialize, Serialize};

use crate::id::{NodeId, ReplicaId, Tag};
use crate::model::{EdgeKey, NodePayload};

/// Every mutation the engine can make. The operation log (a `Vec<StampedOp>`)
/// is the source of truth; the in-memory `Forest` graph is a materialized
/// view derived by folding these operations, so replay and network sync both
/// reduce to "apply this list of operations."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    AddNode { id: NodeId, payload: NodePayload },
    RemoveNode { id: NodeId },
    AddEdge { key: EdgeKey, initial_weight: i64 },
    RemoveEdge { key: EdgeKey },
    /// Reinforce/Decay always carry the replica applying them so the
    /// PN-Counter can credit the right replica's bucket, even when the op
    /// is replayed on a different replica during sync.
    Reinforce { key: EdgeKey, replica: ReplicaId, amount: u64 },
    Decay { key: EdgeKey, replica: ReplicaId, amount: u64 },
    /// Last-writer-wins by tag: a later SetEmbedding for the same node
    /// overwrites an earlier one once merged.
    SetEmbedding { id: NodeId, embedding: Vec<f32> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StampedOp {
    pub tag: Tag,
    pub op: Operation,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpLog {
    ops: Vec<StampedOp>,
}

impl OpLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, stamped: StampedOp) {
        self.ops.push(stamped);
    }

    pub fn all(&self) -> &[StampedOp] {
        &self.ops
    }

    /// Ops strictly newer than `hlc`, in log order — what a `SyncProvider`
    /// ships to a peer that already has everything up to `hlc`.
    pub fn since(&self, hlc: crate::hlc::Hlc) -> Vec<StampedOp> {
        self.ops.iter().filter(|o| o.tag.hlc > hlc).cloned().collect()
    }

    pub fn latest_hlc(&self) -> crate::hlc::Hlc {
        self.ops.iter().map(|o| o.tag.hlc).max().unwrap_or(crate::hlc::Hlc::ZERO)
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}
