# Self-Learning Semantic Engine — Design & Implementation Plan

## Context

The goal is a language-agnostic, platform-agnostic engine that ingests artifacts (source
code, prose, or any other structured/unstructured input), builds a **semantic tree** of
knowledge for each artifact, and continuously **re-weights** that tree from observed usage.
Separately, trees are linked to each other when they come into **semantic proximity**
(shared meaning, not just shared location), so the system behaves like a growing forest of
knowledge rather than isolated per-file trees. It must be distributed-ready, pluggable
(new input languages/domains, new embedding backends, new learning rules), and robust
(crash-safe, convergent under concurrent updates).

Decisions made with the user before designing:
- **Target domains for v1**: both an AI-coding-tool use case (source code) and a
  text-editor/writing-assistant use case (prose/docs) — built on the *same* core engine,
  so cross-domain proximity linking (e.g. a code comment relating to a design doc
  paragraph) is demonstrable, not hypothetical.
- **Learning signal**: hybrid — a deterministic structural/symbolic pass builds the initial
  tree, and embeddings drive proximity detection and similarity-based edges. Usage
  feedback (edits, accept/reject, references) reinforces or decays edge weights. No neural
  net training in v1.
- **Distribution**: v1 runs single-node, but the data model and mutation log are shaped as
  CRDTs from day one, with a `SyncProvider` interface. A real network transport is future
  work; v1 proves convergence with an in-process `LocalSyncProvider`.
- **Core language**: Rust, with Python bindings (via PyO3/maturin) as the primary
  higher-level wrapper.

### Prior art grounding this design
- **Tree-sitter** — incremental, error-tolerant, grammar-per-language parsing; the
  standard way to get a language-agnostic concrete syntax tree to build a semantic tree on
  top of ([tree-sitter](https://github.com/tree-sitter), [incremental parsing](https://tomassetti.me/incremental-parsing-using-tree-sitter/)).
- **Stack graphs** — scalable name-resolution technique layered on tree-sitter output,
  useful model for how "calls/references" edges can be derived per-language without a full
  compiler front end ([arXiv:2211.01224](https://arxiv.org/pdf/2211.01224)).
- **GraphRAG / code knowledge graphs** — combining a structural graph (nodes = code
  entities, edges = calls/imports/contains) with a vector index over the same entities,
  bridged so queries can walk structure *and* rank by meaning — exactly the hybrid
  symbolic+embedding model chosen here ([GraphRAG for code](https://cerebrolabs.tech/blog/graphrag-code-knowledge-graph/), [CodeGraph](https://github.com/Abhishek-Aditya-bs/CodeGraph)).
- **CRDTs for graphs** — add-wins OR-Sets for node/edge existence and PN-Counters for
  numeric fields (like edge weight) are the standard way to make concurrent, uncoordinated
  merges converge automatically — the mechanism that lets independently-evolving trees
  merge later without a central authority ([crdt.tech](https://crdt.tech/), [graph-crdt](https://github.com/vndee/graph-crdt)).
- **PyO3/maturin** — the standard toolchain for exposing a Rust core as a native Python
  module while keeping the core itself embeddable/FFI-able for other languages later
  ([PyO3 guide](https://pyo3.rs/)).

## Architecture Overview

```
                     ┌─────────────────────────────────────────┐
                     │              sle-core (Rust)             │
                     │  Node/Edge model · Operation Log (CRDT)  │
                     │  In-memory graph index · Snapshot store  │
                     │  Reinforcement/decay engine               │
                     │  Proximity scanner (ANN over embeddings)  │
                     └───────────┬───────────────┬─────────────┘
             ┌───────────────────┤               ├───────────────────┐
             │                   │               │                   │
   ┌─────────▼────────┐ ┌────────▼───────┐ ┌─────▼──────┐  ┌─────────▼────────┐
   │ sle-adapters-code │ │ sle-adapters-  │ │sle-embeddings│ │   sle-sync       │
   │ (tree-sitter:      │ │ text (markdown/│ │ trait +      │ │ SyncProvider     │
   │  Rust, Python)      │ │ plaintext →   │ │ hashing impl │ │ trait +          │
   │  → nodes/edges      │ │ section tree) │ │ + optional   │ │ LocalSyncProvider│
   └────────────────────┘ └───────────────┘ │ ONNX impl    │ │ (convergence     │
                                              └──────────────┘ │  proof)          │
                                                                └──────────────────┘
                     ┌─────────────────────────────────────────┐
                     │  sle-py (PyO3 bindings)  │  sle-cli (demo/JSON-RPC host) │
                     └─────────────────────────────────────────┘
```

Everything outside `sle-core` is a **plugin** implementing a small trait. `sle-core` never
imports a specific language grammar or embedding backend directly — it depends only on the
`Adapter`, `EmbeddingProvider`, and `SyncProvider` traits. This is what makes the engine
language/platform-agnostic and extensible.

## Core Data Model (`sle-core`)

- **Node**: `{ id: NodeId, kind: NodeKind, domain: DomainTag, payload: serde_json::Value,
  embedding: Option<Vec<f32>>, created_hlc, touched_hlc }`. `NodeKind` is open-ended
  (`Module`, `Class`, `Function`, `Statement`, `Document`, `Section`, `Paragraph`, …) —
  adapters define their own kinds.
- **Edge**: `{ src: NodeId, dst: NodeId, kind: EdgeKind, weight: PnCounter }`. `EdgeKind`
  includes structural kinds (`Contains`, `Calls`, `Imports`, `Precedes`) from adapters, and
  engine-generated kinds (`SimilarTo`, `ReferencedBy`) from the proximity scanner.
- **SemanticTree**: a named/rooted subgraph (one per ingested artifact — one file, one
  document). Trees live together in a **Forest** (one process/workspace's worth of trees);
  cross-tree edges are just edges whose src/dst belong to different trees.
- **Operation Log**: every mutation (`AddNode`, `AddEdge`, `Reinforce(edge, delta)`,
  `Decay(edge, delta)`) is appended with a hybrid-logical-clock timestamp and a replica id.
  This log *is* the source of truth (event-sourced); the in-memory graph (built on
  `petgraph`) is a materialized view rebuilt from the log or a periodic snapshot.
- **CRDT semantics**: node/edge existence = add-wins OR-Set (tombstone on remove); edge
  weight = PN-Counter (each replica's increments/decrements are tracked separately and
  summed on merge, so reinforcement from independent replicas converges without
  coordination). This is what `sle-sync` merges.

## Learning Loop

1. **Structural pass** (deterministic): an `Adapter` parses raw input → emits `AddNode`/
   `AddEdge` ops for the tree's backbone (containment, calls/imports, document sections).
2. **Embedding pass**: `EmbeddingProvider::embed(node) -> Vec<f32>` fills in `node.embedding`
   for leaf-ish nodes (functions, paragraphs). Default impl (`HashingEmbeddingProvider`) is
   a zero-dependency, deterministic feature-hashing + random-projection embedding — good
   enough to cluster related content and to unit-test proximity logic without any model
   download. An optional `real-embeddings` cargo feature adds an `OnnxEmbeddingProvider`
   for a real sentence-embedding model, swappable with no core changes.
3. **Proximity scan**: a background/on-demand job builds an ANN index (HNSW) over all node
   embeddings in the forest and, for pairs above a similarity threshold that aren't already
   linked, emits a `SimilarTo` edge (initial weight = similarity score). This is the
   mechanism that "connects trees when in proximity."
4. **Usage feedback**: the engine exposes `report_event(node_or_edge, EventKind)` (accessed,
   edited-together, suggestion-accepted/rejected, test-passed/failed). Each event maps to a
   `Reinforce`/`Decay` op on the relevant edges (Hebbian-style: co-activated edges
   strengthen; untouched edges decay on a schedule). This is what makes the tree
   self-updating over time, independent of re-parsing.

## Distribution Readiness (`sle-sync`)

- `SyncProvider` trait: `export_since(hlc) -> OpBatch`, `import(OpBatch)`, both operating on
  the CRDT op log — merge is just "apply all ops, let OR-Set/PN-Counter rules resolve
  conflicts," so it's commutative/associative/idempotent by construction.
- v1 ships `LocalSyncProvider` (in-process, loopback) with property tests (via `proptest`)
  proving convergence: fork a forest, apply divergent op sequences on each fork, merge in
  both orders, assert identical resulting state.
- A real network transport (gRPC/QUIC gossip) is explicitly deferred — the trait boundary
  is the deliverable for v1, not a live cluster.

## Adapters (v1 scope)

- `sle-adapters-code`: tree-sitter grammars for **Rust and Python** to start (adding a
  language later = new grammar crate + a small mapping function, not a core change).
  Produces `Module/Class/Function/Statement` nodes and `Contains/Calls/Imports` edges.
- `sle-adapters-text`: heading/paragraph/sentence segmentation for Markdown and plaintext.
  Produces `Document/Section/Paragraph` nodes and `Contains/Precedes` edges.

## Interfaces

- `sle-py`: PyO3 module (built with `maturin`) exposing `ingest(path)`, `query_neighbors`,
  `report_event`, `export_delta`/`import_delta` to Python.
- `sle-cli`: a demo binary that (a) ingests a small mixed workspace (a couple of Rust/Python
  files + a couple of Markdown docs), (b) prints detected cross-domain `SimilarTo` links,
  (c) simulates a run of usage events and shows weight changes, (d) spins up two in-process
  forests, diverges them, and merges via `LocalSyncProvider` to prove convergence. This is
  also a stdio JSON-RPC server mode (LSP-style) so any language/tool can talk to the engine
  as a subprocess later — the language-agnostic access point on the *consumption* side.

## Crate Layout

```
SelfLearningEngine/
  Cargo.toml (workspace)
  crates/
    sle-core/
    sle-adapters-code/
    sle-adapters-text/
    sle-embeddings/
    sle-sync/
    sle-cli/
  bindings/
    sle-py/         (PyO3 + maturin project)
  sample-workspace/  (demo input: a couple of .rs/.py files + .md docs)
```

## Verification

- `cargo test` across the workspace: unit tests per crate; `proptest`-based property tests
  in `sle-sync` proving CRDT merge is commutative/associative/idempotent.
- `cargo run -p sle-cli -- demo ./sample-workspace`: manually inspect that cross-domain
  `SimilarTo` edges appear between related code and doc nodes, and that a simulated
  usage-feedback run visibly changes edge weights.
- `cargo run -p sle-cli -- demo --distributed`: run the fork/diverge/merge scenario and
  confirm both replicas converge to an identical forest state.
- `maturin develop` in `bindings/sle-py`, then a short Python smoke script that imports the
  module, ingests the sample workspace, and queries neighbors — confirms the FFI boundary
  works end-to-end.
