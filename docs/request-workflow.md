# Request / Data Flow

Sequence diagrams for the three entry points into the engine. Add a new
diagram here whenever a new entry point or user-facing flow is added (a new
CLI subcommand, a new bound Python method, a new adapter's ingest path
worth calling out separately).

## `sle-cli demo <path>` — ingest, embed, link, learn

```mermaid
sequenceDiagram
    actor User
    participant CLI as sle-cli (demo)
    participant Adapter as CodeAdapter / TextAdapter
    participant Forest
    participant Embed as HashingEmbeddingProvider
    participant Scan as scan_proximity

    User->>CLI: sle demo ./sample-workspace
    loop each file in workspace
        CLI->>Adapter: ingest(tree_id, source, &mut forest)
        Adapter->>Forest: add_node / add_edge (Contains, Precedes, Calls, Imports)
    end
    CLI->>Embed: embed(node text or label) for each content-bearing node
    Embed-->>CLI: Vec<f32>
    CLI->>Forest: set_embedding(id, embedding)
    CLI->>Scan: scan_proximity(&forest, threshold)
    Scan-->>CLI: Vec<ProximityLink>
    CLI->>Forest: apply_proximity_links (adds SimilarTo edges)
    CLI->>Forest: report_event(edge, SuggestionAccepted / TestPassed / ...)
    Forest-->>CLI: updated edge weight
    CLI-->>User: printed proximity links + before/after weights
```

## `sle-cli distributed --file <path>` — CRDT merge convergence

```mermaid
sequenceDiagram
    actor User
    participant CLI as sle-cli (distributed)
    participant A as Forest (replica A)
    participant B as Forest (replica B)
    participant Sync as LocalSyncProvider

    User->>CLI: sle distributed --file auth.rs
    CLI->>A: ingest(auth.rs)
    CLI->>Sync: sync_since(A, &mut B, Hlc::ZERO)
    Sync->>A: export_since(0)
    A-->>Sync: ops
    Sync->>B: import(ops)
    Note over A,B: A and B now share identical nodes/edges

    CLI->>A: report_event(edge, SuggestionAccepted + TestPassed)
    CLI->>B: report_event(edge, SuggestionRejected + Accessed)
    Note over A,B: replicas diverge independently, never observing each other

    CLI->>Sync: merge(A, B)
    Sync->>A: import(B's ops since 0)
    Sync->>B: import(A's ops since 0)
    CLI->>CLI: assert state_digest(A) == state_digest(B), both merge orders
    CLI-->>User: "convergence verified"
```

## Python bindings (`bindings/sle-py`) — same core, foreign-language caller

```mermaid
sequenceDiagram
    actor Py as Python caller
    participant Engine as sle_py.Engine (PyO3)
    participant Forest
    participant Adapters
    participant Embed as sle-embeddings

    Py->>Engine: Engine(replica_id)
    Py->>Engine: engine.ingest(path)
    Engine->>Adapters: adapter_for(extension)
    Adapters->>Forest: add_node / add_edge
    Engine-->>Py: root node id (string "replica.local")

    Py->>Engine: engine.scan_proximity(threshold, dims)
    Engine->>Embed: embed() per content-bearing node
    Engine->>Forest: set_embedding / apply_proximity_links
    Engine-->>Py: link count

    Py->>Engine: engine.report_event(src, dst, kind, event)
    Engine->>Forest: report_event(edge_key, event)
    Engine-->>Py: new edge weight

    Py->>Engine: engine.export_delta()
    Engine-->>Py: JSON bytes (operation log)
    Py->>Engine: engine2.import_delta(bytes)
    Engine->>Forest: apply_all(ops)
```
