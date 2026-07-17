//! Distribution-readiness layer. `sle-core`'s `Forest` stores its mutations
//! as a CRDT-shaped operation log (add-wins OR-Sets for existence,
//! PN-Counters for weights), so merging two replicas is always just
//! "apply every op, in any order, possibly more than once" — the `Forest`
//! itself guarantees that's commutative, associative, and idempotent.
//!
//! `LocalSyncProvider` here is the reference, in-process implementation of
//! `SyncProvider`: it proves the trait boundary works end-to-end without
//! standing up a real network transport. A future networked implementation
//! (gRPC/QUIC gossip) would implement the same trait and every caller above
//! this layer would be unaffected.

use sle_core::{Forest, Hlc, StampedOp, SyncProvider};

pub struct LocalSyncProvider;

impl SyncProvider for LocalSyncProvider {
    fn export_since(&self, forest: &Forest, since_hlc: Hlc) -> Vec<StampedOp> {
        forest.log().since(since_hlc)
    }

    fn import(&self, forest: &mut Forest, ops: Vec<StampedOp>) {
        forest.apply_all(ops);
    }
}

/// Convenience: ship everything `from` has produced since `since_hlc` into
/// `to`. Passing `Hlc::ZERO` ships the whole history, which is always safe
/// since replaying already-known ops is a no-op.
pub fn sync_since(from: &Forest, to: &mut Forest, since_hlc: Hlc, provider: &impl SyncProvider) {
    let ops = provider.export_since(from, since_hlc);
    provider.import(to, ops);
}

/// Merge two replicas so both end up with the union of everything either
/// one knew. Order of the two directions doesn't matter (see the
/// convergence property tests below).
pub fn merge(a: &mut Forest, b: &mut Forest, provider: &impl SyncProvider) {
    let a_ops = provider.export_since(a, Hlc::ZERO);
    let b_ops = provider.export_since(b, Hlc::ZERO);
    provider.import(a, b_ops);
    provider.import(b, a_ops);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prop_assert_eq;
    use proptest::proptest;
    use sle_core::{DomainTag, EdgeKey, EdgeKind, EventKind, NodeKind, NodePayload};

    fn node(tree: &str, label: &str) -> NodePayload {
        NodePayload {
            tree: tree.into(),
            kind: NodeKind::new("Function"),
            domain: DomainTag::new("code:rust"),
            label: label.into(),
            text: label.into(),
            data: serde_json::json!({}),
            embedding: None,
        }
    }

    #[test]
    fn merge_converges_regardless_of_direction() {
        // Two replicas fork from a shared node, then diverge: each adds its
        // own node/edge and reinforces independently, without ever
        // observing the other's ops directly.
        let mut a = Forest::new(1);
        let mut b = Forest::new(2);

        let shared_a = a.add_node(node("t1", "shared"));
        // Manually replicate the shared node's *op* into b so both replicas
        // agree on its identity (simulating "b already had this in common").
        let shared_op = a.log().all().last().unwrap().clone();
        b.apply(shared_op);

        let a_only = a.add_node(node("t1", "a_only"));
        let key_a = EdgeKey { src: shared_a, dst: a_only, kind: EdgeKind::new("Contains") };
        a.add_edge(key_a.clone(), 1);
        a.report_event(&key_a, EventKind::Accessed);

        let b_only = b.add_node(node("t1", "b_only"));
        let key_b = EdgeKey { src: shared_a, dst: b_only, kind: EdgeKind::new("Contains") };
        b.add_edge(key_b.clone(), 1);
        b.report_event(&key_b, EventKind::SuggestionAccepted);

        let provider = LocalSyncProvider;

        let mut a1 = a.clone();
        let mut b1 = b.clone();
        merge(&mut a1, &mut b1, &provider);

        let mut b2 = b.clone();
        let mut a2 = a.clone();
        merge(&mut b2, &mut a2, &provider);

        assert_eq!(a1.state_digest(), b1.state_digest(), "both replicas must converge after merging");
        assert_eq!(a1.state_digest(), a2.state_digest(), "merge order must not matter");
        assert_eq!(b1.state_digest(), b2.state_digest());

        // both nodes are visible post-merge, from either replica's view
        assert!(a1.contains_node(&a_only));
        assert!(a1.contains_node(&b_only));
        assert_eq!(a1.weight(&key_a), a.weight(&key_a));
        assert_eq!(a1.weight(&key_b), b.weight(&key_b));
    }

    #[test]
    fn merge_is_idempotent_under_repeated_exchange() {
        let mut a = Forest::new(1);
        let mut b = Forest::new(2);
        a.add_node(node("t1", "x"));
        let provider = LocalSyncProvider;

        merge(&mut a, &mut b, &provider);
        let after_first = a.state_digest();
        // Re-running merge with already-fully-synced replicas must change nothing.
        merge(&mut a, &mut b, &provider);
        assert_eq!(a.state_digest(), after_first);
        assert_eq!(a.state_digest(), b.state_digest());
    }

    // Randomized reinforce/decay traffic on a shared edge, from two
    // replicas that never see each other's ops until merge time. Whatever
    // mix of events each replica applies locally, the final weight after
    // merging must only depend on the multiset of events applied — not on
    // which replica applied them, or which merge direction ran first.
    proptest! {
        #[test]
        fn pncounter_weight_converges_under_random_traffic(
            a_events in proptest::collection::vec(0i64..=1, 0..12),
            b_events in proptest::collection::vec(0i64..=1, 0..12),
        ) {
            let mut a = Forest::new(10);
            let mut b = Forest::new(20);

            let shared = a.add_node(node("t1", "shared"));
            b.apply(a.log().all().last().unwrap().clone());
            let other_a = a.add_node(node("t1", "other_a"));
            b.apply(a.log().all().last().unwrap().clone());

            let key = EdgeKey { src: shared, dst: other_a, kind: EdgeKind::new("Contains") };
            a.add_edge(key.clone(), 0);
            b.apply(a.log().all().last().unwrap().clone());

            let mut expected: i64 = 0;
            for e in &a_events {
                if *e == 1 { a.reinforce(&key, 1); expected += 1; } else { a.decay(&key, 1); expected -= 1; }
            }
            for e in &b_events {
                if *e == 1 { b.reinforce(&key, 1); expected += 1; } else { b.decay(&key, 1); expected -= 1; }
            }

            let provider = LocalSyncProvider;
            let mut a1 = a.clone();
            let mut b1 = b.clone();
            merge(&mut a1, &mut b1, &provider);

            let mut b2 = b.clone();
            let mut a2 = a.clone();
            merge(&mut b2, &mut a2, &provider);

            prop_assert_eq!(a1.weight(&key), expected);
            prop_assert_eq!(a1.weight(&key), b1.weight(&key));
            prop_assert_eq!(a1.weight(&key), a2.weight(&key));
            prop_assert_eq!(a1.weight(&key), b2.weight(&key));
        }
    }
}
