use std::fs;
use std::path::Path;

use crate::forest::Forest;

/// Serialize the whole materialized forest to bytes. Used for crash-safe
/// persistence: write periodically, and on restart load the last snapshot
/// instead of replaying the full operation log from scratch.
///
/// Uses JSON rather than bincode because node payloads carry a
/// self-describing `serde_json::Value` (adapter-defined free-form data),
/// which bincode's non-self-describing format can't round-trip.
pub fn to_bytes(forest: &Forest) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(forest)?)
}

pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Forest> {
    Ok(serde_json::from_slice(bytes)?)
}

pub fn save(forest: &Forest, path: &Path) -> anyhow::Result<()> {
    fs::write(path, to_bytes(forest)?)?;
    Ok(())
}

pub fn load(path: &Path) -> anyhow::Result<Forest> {
    from_bytes(&fs::read(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DomainTag, NodeKind, NodePayload};

    #[test]
    fn roundtrip() {
        let mut f = Forest::new(1);
        f.add_node(NodePayload {
            tree: "t1".into(),
            kind: NodeKind::new("Function"),
            domain: DomainTag::new("code:rust"),
            label: "foo".into(),
            text: "fn foo() {}".into(),
            data: serde_json::json!({}),
            embedding: None,
        });
        let bytes = to_bytes(&f).unwrap();
        let restored = from_bytes(&bytes).unwrap();
        assert_eq!(restored.node_count(), f.node_count());
        assert_eq!(restored.state_digest(), f.state_digest());
    }
}
