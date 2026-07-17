use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::hlc::Hlc;

/// Identifies a replica (one running instance of the engine). Kept as a plain
/// u64 so a new replica just needs any value it can plausibly claim as unique
/// (random, config-assigned, hash of hostname+pid, ...).
pub type ReplicaId = u64;

/// A node is uniquely identified by which replica minted it and a local
/// per-replica counter — no coordination needed to generate one, which is
/// what makes concurrent, offline node creation on different replicas safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub ReplicaId, pub u64);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.0, self.1)
    }
}

impl std::str::FromStr for NodeId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, '.');
        let replica = parts.next().ok_or_else(|| anyhow::anyhow!("NodeId missing replica"))?;
        let local = parts.next().ok_or_else(|| anyhow::anyhow!("NodeId missing local id"))?;
        Ok(NodeId(replica.parse()?, local.parse()?))
    }
}

// Serialized via Display/FromStr (rather than derived) so NodeId can be
// used directly as a BTreeMap key and still round-trip through JSON, whose
// object keys must be strings; it's also the format exposed to bindings
// (e.g. `sle-py`) so foreign callers can pass node ids around as plain
// strings.
impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// A tree groups the nodes emitted by one adapter run over one artifact
/// (one file, one document). Trees are just a grouping label on nodes, not a
/// separate storage structure — cross-tree edges are ordinary edges whose
/// endpoints happen to carry different `TreeId`s.
pub type TreeId = String;

/// Every CRDT-relevant mutation (add/remove) is stamped with a `Tag`: the
/// replica that produced it plus the HLC timestamp it happened at. Tags are
/// unique because a given replica's clock never repeats a timestamp, and
/// they are what OR-Set add/remove tracking and PN-Counter merges key on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tag {
    pub replica: ReplicaId,
    pub hlc: Hlc,
}
