use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::hlc::HlcClock;
use crate::id::{NodeId, ReplicaId, Tag};
use crate::model::{EdgeKey, NodePayload, PnCounter};
use crate::ops::{OpLog, Operation, StampedOp};
use crate::orset::OrSet;

/// A forest is one replica's whole working set of semantic trees: every node
/// and edge it knows about, materialized from an append-only operation log.
/// Multiple `SemanticTree`s (one per ingested artifact) live side by side
/// here; a cross-tree edge is just an ordinary `Edge` whose endpoints happen
/// to carry different `tree` labels on their payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Forest {
    replica: ReplicaId,
    clock: HlcClock,
    next_local_id: u64,

    nodes: OrSet<NodeId>,
    node_payload: BTreeMap<NodeId, NodePayload>,
    node_payload_tag: BTreeMap<NodeId, Tag>,
    node_embedding_tag: BTreeMap<NodeId, Tag>,

    edges: OrSet<EdgeKey>,
    edge_weight: BTreeMap<EdgeKey, PnCounter>,

    /// Every tag whose op has already been applied. Since a `Tag` is unique
    /// per (replica, HLC tick), this lets `apply` be a true no-op on replay
    /// — required for sync/merge to be safe when the same operation might
    /// arrive more than once (e.g. overlapping export batches).
    applied_tags: BTreeSet<Tag>,

    log: OpLog,
}

impl Forest {
    pub fn new(replica: ReplicaId) -> Self {
        Self {
            replica,
            clock: HlcClock::new(replica),
            next_local_id: 0,
            nodes: OrSet::new(),
            node_payload: BTreeMap::new(),
            node_payload_tag: BTreeMap::new(),
            node_embedding_tag: BTreeMap::new(),
            edges: OrSet::new(),
            edge_weight: BTreeMap::new(),
            applied_tags: BTreeSet::new(),
            log: OpLog::new(),
        }
    }

    pub fn replica(&self) -> ReplicaId {
        self.replica
    }

    pub fn log(&self) -> &OpLog {
        &self.log
    }

    // ---- local mutation API (mints ops locally, then applies them) ----

    pub fn add_node(&mut self, payload: NodePayload) -> NodeId {
        let id = NodeId(self.replica, self.next_local_id);
        self.next_local_id += 1;
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp { tag, op: Operation::AddNode { id, payload } });
        id
    }

    pub fn remove_node(&mut self, id: NodeId) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp { tag, op: Operation::RemoveNode { id } });
    }

    pub fn add_edge(&mut self, key: EdgeKey, initial_weight: i64) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp { tag, op: Operation::AddEdge { key, initial_weight } });
    }

    pub fn remove_edge(&mut self, key: EdgeKey) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp { tag, op: Operation::RemoveEdge { key } });
    }

    pub fn reinforce(&mut self, key: &EdgeKey, amount: u64) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp {
            tag,
            op: Operation::Reinforce { key: key.clone(), replica: self.replica, amount },
        });
    }

    pub fn decay(&mut self, key: &EdgeKey, amount: u64) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp {
            tag,
            op: Operation::Decay { key: key.clone(), replica: self.replica, amount },
        });
    }

    pub fn set_embedding(&mut self, id: NodeId, embedding: Vec<f32>) {
        let tag = Tag { replica: self.replica, hlc: self.clock.tick() };
        self.apply(StampedOp { tag, op: Operation::SetEmbedding { id, embedding } });
    }

    /// Global decay tick: every currently-present edge loses `amount` from
    /// this replica's perspective. Called on a schedule so unused knowledge
    /// fades even without an explicit negative-feedback event.
    pub fn decay_all(&mut self, amount: u64) {
        let keys: Vec<EdgeKey> = self.edges.iter_present().cloned().collect();
        for key in keys {
            self.decay(&key, amount);
        }
    }

    // ---- applying ops (local or remote) — the one place CRDT rules live ----

    pub fn apply(&mut self, stamped: StampedOp) {
        if !self.applied_tags.insert(stamped.tag) {
            // Already applied this exact operation — replaying it must be a
            // no-op, or a merge that re-sends overlapping ops would corrupt
            // PN-Counter weights and duplicate the log.
            return;
        }
        self.clock.observe(stamped.tag.hlc);
        match &stamped.op {
            Operation::AddNode { id, payload } => {
                self.nodes.add(*id, stamped.tag);
                let replace = match self.node_payload_tag.get(id) {
                    Some(existing) => stamped.tag > *existing,
                    None => true,
                };
                if replace {
                    self.node_payload.insert(*id, payload.clone());
                    self.node_payload_tag.insert(*id, stamped.tag);
                }
            }
            Operation::RemoveNode { id } => {
                self.nodes.remove(id);
            }
            Operation::AddEdge { key, initial_weight } => {
                self.edges.add(key.clone(), stamped.tag);
                let counter = self.edge_weight.entry(key.clone()).or_default();
                if *initial_weight > 0 {
                    counter.incr(stamped.tag.replica, *initial_weight as u64);
                } else if *initial_weight < 0 {
                    counter.decr(stamped.tag.replica, (-*initial_weight) as u64);
                }
            }
            Operation::RemoveEdge { key } => {
                self.edges.remove(key);
            }
            Operation::Reinforce { key, replica, amount } => {
                self.edge_weight.entry(key.clone()).or_default().incr(*replica, *amount);
            }
            Operation::Decay { key, replica, amount } => {
                self.edge_weight.entry(key.clone()).or_default().decr(*replica, *amount);
            }
            Operation::SetEmbedding { id, embedding } => {
                let replace = match self.node_embedding_tag.get(id) {
                    Some(existing) => stamped.tag > *existing,
                    None => true,
                };
                if replace {
                    self.node_embedding_tag.insert(*id, stamped.tag);
                    if let Some(payload) = self.node_payload.get_mut(id) {
                        payload.embedding = Some(embedding.clone());
                    }
                }
            }
        }
        self.log.append(stamped);
    }

    pub fn apply_all(&mut self, ops: Vec<StampedOp>) {
        for op in ops {
            self.apply(op);
        }
    }

    // ---- queries ----

    pub fn contains_node(&self, id: &NodeId) -> bool {
        self.nodes.contains(id)
    }

    pub fn node(&self, id: &NodeId) -> Option<&NodePayload> {
        if self.nodes.contains(id) {
            self.node_payload.get(id)
        } else {
            None
        }
    }

    pub fn nodes(&self) -> impl Iterator<Item = (&NodeId, &NodePayload)> {
        self.nodes.iter_present().filter_map(move |id| self.node_payload.get(id).map(|p| (id, p)))
    }

    pub fn edge_present(&self, key: &EdgeKey) -> bool {
        self.edges.contains(key)
    }

    pub fn weight(&self, key: &EdgeKey) -> i64 {
        self.edge_weight.get(key).map(|c| c.value()).unwrap_or(0)
    }

    pub fn edges(&self) -> impl Iterator<Item = (&EdgeKey, i64)> {
        self.edges.iter_present().map(move |k| (k, self.weight(k)))
    }

    pub fn edges_from(&self, src: &NodeId) -> Vec<(EdgeKey, i64)> {
        self.edges()
            .filter(|(k, _)| &k.src == src)
            .map(|(k, w)| (k.clone(), w))
            .collect()
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len_present()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len_present()
    }

    /// A materialized, order-independent snapshot of visible state — used to
    /// assert that two replicas converged to the same knowledge after a
    /// merge, regardless of the order operations were applied in.
    pub fn state_digest(&self) -> ForestState {
        let mut nodes: Vec<(NodeId, String)> = self
            .nodes()
            .map(|(id, p)| (*id, format!("{}|{}|{}", p.tree, p.kind, p.label)))
            .collect();
        nodes.sort();
        let mut edges: Vec<(EdgeKey, i64)> = self.edges().map(|(k, w)| (k.clone(), w)).collect();
        edges.sort_by(|a, b| a.0.cmp(&b.0));
        ForestState { nodes, edges }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForestState {
    pub nodes: Vec<(NodeId, String)>,
    pub edges: Vec<(EdgeKey, i64)>,
}
