# Architecture

The engine is a Rust workspace of small, trait-bounded crates. `sle-core`
depends on nothing domain-specific — only on the `Adapter`,
`EmbeddingProvider`, and `SyncProvider` traits it defines — which is what
keeps the engine language-agnostic, platform-agnostic, and pluggable:
every language, embedding backend, or transport is a new implementation of
one of those traits, not a change to the core.

## System diagram

```mermaid
graph TB
    subgraph Core["sle-core (Rust)"]
        Forest["Forest<br/>Node/Edge model, OR-Set + PN-Counter CRDT,<br/>Operation Log, Hybrid Logical Clock"]
        Traits["Adapter / EmbeddingProvider /<br/>SyncProvider traits"]
        Learning["report_event()<br/>reinforcement / decay"]
        Snapshot["snapshot.rs<br/>JSON persistence"]
    end

    subgraph Adapters["Ingestion adapters (pluggable)"]
        CodeAdapter["sle-adapters-code<br/>tree-sitter: Rust, Python"]
        TextAdapter["sle-adapters-text<br/>Markdown / plaintext"]
    end

    subgraph EmbedProx["sle-embeddings"]
        Hashing["HashingEmbeddingProvider<br/>feature hashing, zero-dep"]
        Scanner["scan_proximity() / apply_proximity_links()<br/>brute-force cosine to SimilarTo edges"]
    end

    subgraph Sync["sle-sync (distribution-ready)"]
        Local["LocalSyncProvider<br/>in-process reference impl"]
        Merge["merge() / sync_since()"]
    end

    subgraph Interfaces["Interfaces"]
        CLI["sle-cli<br/>demo / distributed / serve"]
        PyBindings["bindings/sle-py<br/>PyO3 + maturin"]
    end

    CodeAdapter -->|implements Adapter| Forest
    TextAdapter -->|implements Adapter| Forest
    Hashing -->|implements EmbeddingProvider| Scanner
    Scanner --> Forest
    Local -->|implements SyncProvider| Merge
    Merge --> Forest
    Forest --> Learning
    Forest --> Snapshot

    CLI --> Forest
    CLI --> CodeAdapter
    CLI --> TextAdapter
    CLI --> Hashing
    CLI --> Scanner
    CLI --> Local

    PyBindings --> Forest
    PyBindings --> CodeAdapter
    PyBindings --> TextAdapter
    PyBindings --> Hashing
    PyBindings --> Scanner
```

## Core data model

```mermaid
classDiagram
    class NodeId {
        +u64 replica
        +u64 local
    }
    class Tag {
        +ReplicaId replica
        +Hlc hlc
    }
    class NodePayload {
        +TreeId tree
        +NodeKind kind
        +DomainTag domain
        +String label
        +String text
        +Value data
        +Option~Vec~f32~~ embedding
    }
    class EdgeKey {
        +NodeId src
        +NodeId dst
        +EdgeKind kind
    }
    class PnCounter {
        +BTreeMap~ReplicaId,u64~ pos
        +BTreeMap~ReplicaId,u64~ neg
        +value() i64
        +merge(other)
    }
    class OrSet~E~ {
        +BTreeMap~E,Set~Tag~~ adds
        +BTreeMap~E,Set~Tag~~ removes
        +add(elem, tag)
        +remove(elem)
        +contains(elem) bool
        +merge(other)
    }
    class Forest {
        +OrSet~NodeId~ nodes
        +OrSet~EdgeKey~ edges
        +BTreeMap~NodeId,NodePayload~ node_payload
        +BTreeMap~EdgeKey,PnCounter~ edge_weight
        +OpLog log
        +Set~Tag~ applied_tags
        +add_node(payload) NodeId
        +add_edge(key, weight)
        +reinforce(key, amount)
        +decay(key, amount)
        +apply(stamped_op)
        +report_event(key, event)
    }

    Forest --> OrSet : nodes / edges
    Forest --> PnCounter : edge_weight
    Forest --> NodePayload : node_payload
    Forest --> Tag : applied_tags (idempotency)
    EdgeKey --> NodeId : src / dst
```

**Why this shape:** every mutation (`AddNode`, `AddEdge`, `Reinforce`,
`Decay`) is appended to `Forest`'s operation log with a `Tag` (replica +
hybrid-logical-clock timestamp). Node/edge *existence* is an add-wins
OR-Set; edge *weight* is a PN-Counter (each replica's increments/decrements
tracked separately, merged by per-replica max/sum). Both are commutative,
associative, and idempotent by construction, so two replicas can always
merge their operation logs — in any order, even with overlap — and
converge to the same state. That CRDT shape is what "distributed" means
for this engine in v1: a single process today, provably mergeable with
another process's history whenever a real network transport is added
behind `SyncProvider`.

## Crate/layer reference

| Layer | Crate | Role |
|---|---|---|
| Core | `sle-core` | Node/Edge/Forest CRDT model, operation log, HLC, reinforcement/decay, plugin traits, JSON snapshot persistence |
| Ingestion | `sle-adapters-code` | tree-sitter-based `Adapter` for Rust & Python: Module/Class/Function/Import nodes, Contains/Precedes/Calls/Imports edges |
| Ingestion | `sle-adapters-text` | Markdown/plaintext `Adapter`: Document/Section/Paragraph nodes, Contains/Precedes edges |
| Learning | `sle-embeddings` | `EmbeddingProvider` (hashing-trick), `scan_proximity`/`apply_proximity_links` (cross-tree `SimilarTo` edges) |
| Distribution | `sle-sync` | `SyncProvider`, `LocalSyncProvider`, `merge`/`sync_since`, proptest convergence tests |
| Interface | `sle-cli` | `demo`, `distributed`, `serve` (stdio JSON-line stub) |
| Interface | `bindings/sle-py` | PyO3/maturin `Engine` class — same traits/Forest, exposed to Python |

See [feature-map.md](feature-map.md) for how these map back to the original
requirements, and [request-workflow.md](request-workflow.md) for
sequence-level flows through each entry point.
