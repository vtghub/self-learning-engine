"""Smoke test for the sle_py PyO3 bindings: ingest the sample workspace,
scan for proximity links, query neighbors, report a usage event, and prove
two engines converge via export/import_delta. Run with:

    .venv/Scripts/python.exe python_tests/smoke_test.py
"""

import pathlib

from sle_py import Engine

WORKSPACE = pathlib.Path(__file__).resolve().parents[3] / "sample-workspace"


def main():
    engine = Engine(1)
    for name in ["auth.rs", "login.py", "design.md", "math_utils.rs", "baking.md"]:
        root = engine.ingest(str(WORKSPACE / name))
        assert root, f"ingest returned no root for {name}"

    nodes_before, edges_before = engine.stats()
    print(f"ingested: {nodes_before} nodes / {edges_before} edges")
    assert nodes_before > 0 and edges_before > 0

    link_count = engine.scan_proximity(threshold=0.5, dims=128)
    print(f"proximity scan found {link_count} link(s)")
    assert link_count > 0, "expected at least one cross-tree/semantic link"

    nodes_after, edges_after = engine.stats()
    assert edges_after > edges_before, "proximity links should add edges"

    # Find some edge to exercise report_event on.
    sample_neighbors = []
    for _tree_root_guess in range(50):
        node_id = f"1.{_tree_root_guess}"
        neighbors = engine.query_neighbors(node_id)
        if neighbors:
            sample_neighbors = [(node_id, dst, kind) for dst, kind, _w in neighbors]
            break
    assert sample_neighbors, "expected to find at least one node with outgoing edges"

    src, dst, kind = sample_neighbors[0]
    weight_after = engine.report_event(src, dst, kind, "SuggestionAccepted")
    print(f"reinforced edge {src} -[{kind}]-> {dst}: weight={weight_after}")

    # export/import_delta: a second engine should converge to the same edge weight.
    delta = engine.export_delta()
    replica2 = Engine(2)
    replica2.import_delta(delta)
    n2, e2 = replica2.stats()
    assert (n2, e2) == (nodes_after, edges_after), "replica2 should match after import_delta"
    print(f"replica2 converged: {n2} nodes / {e2} edges")

    print("SMOKE TEST OK")


if __name__ == "__main__":
    main()
