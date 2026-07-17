# Self-Learning Semantic Engine

A language-agnostic, platform-agnostic engine that ingests artifacts
(source code, prose, or anything else with an `Adapter`), builds a
**semantic tree** per artifact, and continuously **re-weights** it from
observed usage. Trees are linked to each other when they come into
**semantic proximity** — shared meaning, not just shared location — so the
system behaves like a growing forest of knowledge, not isolated per-file
trees. The data model is CRDT-shaped from the ground up, so it's
distributed-ready even though v1 runs single-node.

See [docs/architecture.md](docs/architecture.md) for the system diagram and
core data model, [docs/feature-map.md](docs/feature-map.md) for how each
original requirement maps to what's implemented, and
[docs/request-workflow.md](docs/request-workflow.md) for sequence-level
flows through the CLI and Python bindings. The original design brainstorm
and rationale live in
[docs/brainstorm-design-and-implement-enchanted-brooks.md](docs/brainstorm-design-and-implement-enchanted-brooks.md).
See [CHANGELOG.md](CHANGELOG.md) for release history.

## Architecture

| Layer | Crate | Role |
|---|---|---|
| Core | `sle-core` | Node/Edge/Forest CRDT model, operation log, hybrid logical clock, reinforcement/decay, plugin traits (`Adapter`, `EmbeddingProvider`, `SyncProvider`), JSON snapshot persistence |
| Ingestion | `sle-adapters-code` | tree-sitter-based `Adapter` for Rust & Python |
| Ingestion | `sle-adapters-text` | Markdown/plaintext `Adapter` |
| Learning | `sle-embeddings` | Hashing-trick `EmbeddingProvider` + brute-force proximity scanner (`SimilarTo` edges) |
| Distribution | `sle-sync` | `SyncProvider`, `LocalSyncProvider`, CRDT merge, proptest convergence tests |
| Interface | `sle-cli` | `demo`, `distributed`, `serve` (stdio JSON-line stub) |
| Interface | `bindings/sle-py` | PyO3/maturin Python bindings over the same core |

## Project structure

```
SelfLearningEngine/
  Cargo.toml                    workspace manifest
  crates/
    sle-core/                   CRDT graph model, traits, learning loop
    sle-adapters-code/          tree-sitter adapter (Rust, Python)
    sle-adapters-text/          Markdown/plaintext adapter
    sle-embeddings/             embedding provider + proximity scanner
    sle-sync/                   SyncProvider + LocalSyncProvider
    sle-cli/                    demo / distributed / serve CLI
  bindings/
    sle-py/                     PyO3 + maturin Python bindings
      python_tests/smoke_test.py
  sample-workspace/              demo input: auth.rs, login.py, design.md,
                                  math_utils.rs, baking.md
  docs/
    architecture.md
    feature-map.md
    request-workflow.md
    brainstorm-design-and-implement-enchanted-brooks.md
```

## Building and running

Requires a Rust toolchain (stable, MSVC on Windows) and, for the Python
bindings, Python 3.8+ with `maturin`.

```bash
# build + test the whole Rust workspace
cargo build --workspace
cargo test --workspace

# ingest the sample workspace, scan for proximity links, simulate feedback
cargo run -p sle-cli -- demo ./sample-workspace

# prove CRDT merge converges across two diverging replicas
cargo run -p sle-cli -- distributed --file sample-workspace/auth.rs

# minimal newline-delimited-JSON stdio server (stub, not full LSP framing)
cargo run -p sle-cli -- serve
```

Python bindings:

```bash
cd bindings/sle-py
python -m venv .venv
./.venv/Scripts/activate   # or `source .venv/bin/activate` on macOS/Linux
pip install maturin
maturin develop
python python_tests/smoke_test.py
```

## Design notes / known v1 simplifications

- **Proximity search** is a brute-force O(n²) cosine scan, fine at demo
  scale; swappable for a real ANN index (HNSW) behind the same
  `scan_proximity` signature.
- **Embeddings** default to a deterministic, zero-dependency feature-hashing
  provider (no model download); a real sentence-embedding model is a
  drop-in `EmbeddingProvider` implementation.
- **Call resolution** in `sle-adapters-code` is best-effort by bare name
  (two-pass define-then-link), not scope-correct name resolution.
- **Distribution** ships an in-process `LocalSyncProvider` reference
  implementation; a networked transport is a new `SyncProvider` impl, not a
  core change.

See [docs/feature-map.md](docs/feature-map.md) for the full list mapped
against the original requirements.

## Contributing / workflow

This repo follows a three-tier branching workflow (`main` /
`develop` / `feature/*`) — see `.claude/commands/git-workflow.md`. After
every feature/fix change, architecture and workflow docs are updated per
`.claude/commands/update-docs.md`.
