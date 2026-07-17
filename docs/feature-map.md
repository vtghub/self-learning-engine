# Feature Map

Maps the original brainstorm requirements to what's actually implemented,
and where. Update this alongside `architecture.md` whenever a requirement
moves from "designed" to "implemented," or a new one is added.

```mermaid
mindmap
  root((Self-Learning<br/>Semantic Engine))
    Language and platform agnostic
      Adapter trait in sle-core
        tree-sitter code adapter: Rust, Python
        Markdown / plaintext adapter
        new language = new grammar + mapping fn, no core change
      Consumption-side agnostic too
        Rust core
        PyO3 Python bindings
        stdio JSON-line CLI serve stub
    Self-learning and self-updating
      report_event API
        Accessed / EditedTogether
        SuggestionAccepted / SuggestionRejected
        TestPassed / TestFailed
      Reinforcement and decay via PN-Counter edge weights
      No re-parsing needed to update weights
    Semantic tree / forest of knowledge
      Node and Edge model
        Contains, Precedes -- structural
        Calls, Imports -- code adapter
        SimilarTo -- proximity scanner
      Forest = many trees in one replica
    Connects trees when in proximity
      HashingEmbeddingProvider -- feature hashing, zero-dep
      scan_proximity -- cosine similarity over embedded nodes
      apply_proximity_links -- materializes SimilarTo edges
      Demonstrated cross-domain: code function to doc section
      Demonstrated discrimination: unrelated content stays unlinked
    Distributed and robust
      CRDT data model
        OR-Set for node/edge existence -- add-wins
        PN-Counter for edge weight -- per-replica sum
        Hybrid Logical Clock for causal ordering
      SyncProvider trait
        LocalSyncProvider -- in-process reference impl
        proptest-verified convergence -- commutative/associative/idempotent
      Idempotent operation log -- safe replay under overlapping sync
      JSON snapshot persistence -- crash recovery
    Flexible, extendable, pluggable
      Adapter trait -- swap/add ingestion domains
      EmbeddingProvider trait -- swap hashing for a real ML model
      SyncProvider trait -- swap loopback for a real network transport
      Proximity scan is a brute-force scan behind a stable signature --
      swappable for a real ANN index (HNSW) without touching callers
    Target applications
      AI coding tools -- Calls/Imports graph, usage-driven reinforcement
      Text editors -- Markdown section/paragraph tree
      Compilers -- tree-sitter structural pass is directly reusable
```

## Status legend

Everything on this map is **implemented** as of the current codebase (see
`docs/architecture.md` for where). Deliberately deferred / simplified for
v1, tracked here so they're not mistaken for gaps in the design:

| Item | v1 state | Real implementation would be |
|---|---|---|
| Proximity search | Brute-force O(n²) cosine scan | An ANN index (e.g. HNSW) behind the same `scan_proximity` signature |
| Embeddings | Deterministic feature-hashing (`HashingEmbeddingProvider`) | A real sentence-embedding model behind `EmbeddingProvider` |
| Call resolution | Best-effort by bare name, two-pass | Scope-correct resolution (e.g. stack-graphs) layered on the same tree-sitter parse |
| Distribution transport | `LocalSyncProvider` (in-process loopback) | A networked `SyncProvider` (gRPC/QUIC gossip) |
