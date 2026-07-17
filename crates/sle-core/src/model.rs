use std::collections::BTreeMap;

use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::id::{NodeId, ReplicaId, TreeId};

/// Open-ended node kind, e.g. "Module", "Function", "Paragraph". Adapters
/// mint their own kinds; the core never enumerates a closed set so new
/// domains never require a core change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeKind(pub String);

impl NodeKind {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Open-ended edge kind, e.g. "Contains", "Calls", "Imports", "Precedes",
/// plus engine-generated kinds like "SimilarTo".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeKind(pub String);

impl EdgeKind {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Tags which domain/language an adapter produced a node from, e.g.
/// "code:rust", "code:python", "text:markdown".
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DomainTag(pub String);

impl DomainTag {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// The last-writer-wins payload carried by a node. `data` is adapter-defined
/// free-form JSON (e.g. source span, docstring) so the core never needs to
/// know a domain's internal shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePayload {
    pub tree: TreeId,
    pub kind: NodeKind,
    pub domain: DomainTag,
    pub label: String,
    pub text: String,
    pub data: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
}

/// Identifies an edge by its endpoints and kind. There is at most one edge
/// per (src, dst, kind) triple; reinforcing/decaying the "same" edge from
/// multiple replicas is what the PN-Counter weight is designed to merge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeKey {
    pub src: NodeId,
    pub dst: NodeId,
    pub kind: EdgeKind,
}

// Serialized as a single string (rather than derived) for the same reason as
// `NodeId`: it's used as a `BTreeMap` key and JSON object keys must be
// strings. The kind is written last via `splitn`, so it may itself contain
// the separator character without breaking parsing.
impl Serialize for EdgeKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!(
            "{}.{}~{}.{}~{}",
            self.src.0, self.src.1, self.dst.0, self.dst.1, self.kind.0
        ))
    }
}

impl<'de> Deserialize<'de> for EdgeKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let mut parts = s.splitn(3, '~');
        let src = parts.next().ok_or_else(|| de::Error::custom("EdgeKey missing src"))?;
        let dst = parts.next().ok_or_else(|| de::Error::custom("EdgeKey missing dst"))?;
        let kind = parts.next().ok_or_else(|| de::Error::custom("EdgeKey missing kind"))?;
        let parse_node = |s: &str| -> Result<NodeId, D::Error> {
            let mut p = s.splitn(2, '.');
            let a = p.next().ok_or_else(|| de::Error::custom("bad node id in EdgeKey"))?;
            let b = p.next().ok_or_else(|| de::Error::custom("bad node id in EdgeKey"))?;
            Ok(NodeId(a.parse().map_err(de::Error::custom)?, b.parse().map_err(de::Error::custom)?))
        };
        Ok(EdgeKey { src: parse_node(src)?, dst: parse_node(dst)?, kind: EdgeKind::new(kind) })
    }
}

/// A PN-Counter: each replica tracks its own positive/negative increments
/// separately, so merging two replicas' counters is just a per-replica max
/// (each replica's own count only ever grows), which makes merge
/// commutative, associative, and idempotent without coordination.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PnCounter {
    pos: BTreeMap<ReplicaId, u64>,
    neg: BTreeMap<ReplicaId, u64>,
}

impl PnCounter {
    pub fn value(&self) -> i64 {
        let p: u64 = self.pos.values().sum();
        let n: u64 = self.neg.values().sum();
        p as i64 - n as i64
    }

    pub fn incr(&mut self, replica: ReplicaId, amount: u64) {
        *self.pos.entry(replica).or_insert(0) += amount;
    }

    pub fn decr(&mut self, replica: ReplicaId, amount: u64) {
        *self.neg.entry(replica).or_insert(0) += amount;
    }

    pub fn merge(&mut self, other: &PnCounter) {
        for (replica, count) in &other.pos {
            let entry = self.pos.entry(*replica).or_insert(0);
            *entry = (*entry).max(*count);
        }
        for (replica, count) in &other.neg {
            let entry = self.neg.entry(*replica).or_insert(0);
            *entry = (*entry).max(*count);
        }
    }
}
