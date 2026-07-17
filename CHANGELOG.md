# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project uses [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-07-17

Initial release: the first working implementation of the Self-Learning
Semantic Engine brainstorm — a single-node engine with a CRDT-shaped data
model designed to be distribution-ready.

### Added

- **`sle-core`** — CRDT semantic graph model: add-wins OR-Set for node/edge
  existence, PN-Counter for edge weights, hybrid logical clock, idempotent
  append-only operation log, JSON snapshot persistence, and the `Adapter` /
  `EmbeddingProvider` / `SyncProvider` plugin traits.
- **`sle-core` learning loop** — `report_event` API (Accessed,
  EditedTogether, SuggestionAccepted/Rejected, TestPassed/Failed) driving
  reinforcement/decay of edge weights without re-parsing.
- **`sle-adapters-code`** — tree-sitter-based `Adapter` for Rust and Python:
  Module/Class/Function/Import nodes, Contains/Precedes/Calls/Imports
  edges, with best-effort by-name call resolution.
- **`sle-adapters-text`** — Markdown/plaintext `Adapter` building a
  Document → Section → Paragraph tree.
- **`sle-embeddings`** — zero-dependency hashing-trick `EmbeddingProvider`
  and a brute-force cosine `scan_proximity` that materializes `SimilarTo`
  edges across trees and domains.
- **`sle-sync`** — `LocalSyncProvider` reference `SyncProvider`
  implementation, with proptest-verified merge convergence (commutative,
  associative, idempotent).
- **`sle-cli`** — `demo`, `distributed`, and `serve` subcommands.
- **`bindings/sle-py`** — PyO3/maturin Python bindings (`Engine` class)
  exposing ingest, proximity scanning, event reporting, and delta
  export/import; smoke-tested end to end.
- **`sample-workspace/`** — demo content (Rust/Python auth code + Markdown
  design doc + unrelated math/baking content) used to demonstrate both
  cross-domain proximity linking and correct discrimination against
  unrelated content.
- **Docs** — `README.md`, `docs/architecture.md`, `docs/feature-map.md`,
  `docs/request-workflow.md` (Mermaid diagrams), and the original design
  brainstorm doc.

### Known v1 simplifications

Tracked in detail in `docs/feature-map.md`: proximity search is brute-force
(not an ANN index), embeddings are hashing-based (not a real ML model),
call resolution is best-effort by name (not scope-correct), and
distribution ships only an in-process loopback transport.
