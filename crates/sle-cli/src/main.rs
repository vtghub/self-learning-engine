use std::fs;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use sle_core::{EmbeddingProvider, EventKind, Forest, Hlc, NodeId};
use sle_embeddings::{apply_proximity_links, scan_proximity, HashingEmbeddingProvider};
use sle_sync::LocalSyncProvider;

#[derive(Parser)]
#[command(name = "sle", about = "Self-Learning Semantic Engine CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ingest a workspace of mixed code/docs, scan for proximity links across
    /// trees, and simulate a round of usage feedback.
    Demo {
        path: PathBuf,
        #[arg(long, default_value_t = 0.5)]
        threshold: f32,
    },
    /// Fork a forest across two in-process replicas, diverge them with
    /// independent usage feedback, and prove CRDT merge converges.
    Distributed {
        #[arg(long, default_value = "sample-workspace/auth.rs")]
        file: PathBuf,
    },
    /// Minimal newline-delimited-JSON stdio server (a stub, not full LSP
    /// framing) so any language/tool can drive the engine as a subprocess.
    Serve,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Demo { path, threshold } => demo(&path, threshold),
        Command::Distributed { file } => distributed_demo(&file),
        Command::Serve => serve(),
    }
}

fn adapter_for(path: &Path) -> Option<Box<dyn sle_core::Adapter>> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => sle_adapters_code::CodeAdapter::for_extension("rs").map(|a| Box::new(a) as Box<dyn sle_core::Adapter>),
        Some("py") => sle_adapters_code::CodeAdapter::for_extension("py").map(|a| Box::new(a) as Box<dyn sle_core::Adapter>),
        Some("md") | Some("txt") => Some(Box::new(sle_adapters_text::TextAdapter)),
        _ => None,
    }
}

fn ingest_file(forest: &mut Forest, path: &Path) -> anyhow::Result<Option<NodeId>> {
    let Some(adapter) = adapter_for(path) else { return Ok(None) };
    let content = fs::read_to_string(path)?;
    let tree_id = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    let root = adapter.ingest(&tree_id, &content, forest)?;
    Ok(Some(root))
}

fn ingest_workspace(forest: &mut Forest, dir: &Path) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_file() {
            match ingest_file(forest, &path) {
                Ok(Some(_)) => println!("ingested {}", path.display()),
                Ok(None) => {}
                Err(e) => eprintln!("failed to ingest {}: {e}", path.display()),
            }
        }
    }
    Ok(())
}

/// Pure containers ("Module" for a whole source file, "Document" for a
/// whole prose file) don't carry meaningful content of their own — only
/// their filename, which produces noisy, misleading similarity scores
/// (e.g. two unrelated `.rs` files "matching" on the shared `rs` token).
/// Proximity is scored only between content-bearing nodes.
fn is_embeddable(kind: &str) -> bool {
    !matches!(kind, "Module" | "Document")
}

fn embed_all(forest: &mut Forest, provider: &impl EmbeddingProvider) {
    let targets: Vec<(NodeId, String)> = forest
        .nodes()
        .filter(|(_, p)| is_embeddable(&p.kind.to_string()))
        .map(|(id, p)| {
            let basis = if p.text.trim().is_empty() { p.label.clone() } else { p.text.clone() };
            (*id, basis)
        })
        .collect();
    for (id, basis) in targets {
        let embedding = provider.embed(&basis);
        forest.set_embedding(id, embedding);
    }
}

fn demo(path: &Path, threshold: f32) -> anyhow::Result<()> {
    let mut forest = Forest::new(1);
    ingest_workspace(&mut forest, path)?;
    println!(
        "\ningested {} nodes / {} edges from {}",
        forest.node_count(),
        forest.edge_count(),
        path.display()
    );

    let embedder = HashingEmbeddingProvider::new(128);
    embed_all(&mut forest, &embedder);

    let links = scan_proximity(&forest, threshold);
    println!("\nproximity scan (threshold={threshold}): {} candidate link(s)", links.len());
    for link in &links {
        let a = forest.node(&link.a);
        let b = forest.node(&link.b);
        if let (Some(a), Some(b)) = (a, b) {
            let marker = if a.tree != b.tree { "CROSS-TREE" } else { "same-tree " };
            println!(
                "  [{marker}] score={:.2}  {} ({}:{})  <->  {} ({}:{})",
                link.score, a.label, a.tree, a.kind, b.label, b.tree, b.kind
            );
        }
    }
    apply_proximity_links(&mut forest, &links);

    let calls_edge = forest.edges().find(|(k, _)| k.kind.to_string() == "Calls").map(|(k, w)| (k.clone(), w));
    if let Some((key, before)) = calls_edge {
        println!("\nsimulating usage feedback on a Calls edge (weight={before}):");
        forest.report_event(&key, EventKind::SuggestionAccepted);
        forest.report_event(&key, EventKind::TestPassed);
        println!("  after SuggestionAccepted + TestPassed: weight={}", forest.weight(&key));
        forest.report_event(&key, EventKind::SuggestionRejected);
        println!("  after a later SuggestionRejected:      weight={}", forest.weight(&key));
    }

    println!(
        "\nfinal state: {} nodes / {} edges",
        forest.node_count(),
        forest.edge_count()
    );
    Ok(())
}

fn distributed_demo(file: &Path) -> anyhow::Result<()> {
    let mut a = Forest::new(100);
    ingest_file(&mut a, file)?.ok_or_else(|| anyhow::anyhow!("no adapter for {}", file.display()))?;

    let provider = LocalSyncProvider;
    let mut b = Forest::new(200);
    sle_sync::sync_since(&a, &mut b, Hlc::ZERO, &provider);
    println!(
        "replica B synced from replica A once: {} nodes / {} edges shared",
        b.node_count(),
        b.edge_count()
    );

    let key = match a.edges().find(|(k, _)| k.kind.to_string() == "Calls").map(|(k, _)| k.clone()) {
        Some(k) => k,
        None => anyhow::bail!("{} has no Calls edge to demonstrate with", file.display()),
    };
    println!("shared edge before divergence: weight={}", a.weight(&key));

    // Replica A: one developer's session.
    a.report_event(&key, EventKind::SuggestionAccepted);
    a.report_event(&key, EventKind::TestPassed);
    // Replica B: a different developer's session, never seeing A's updates.
    b.report_event(&key, EventKind::SuggestionRejected);
    b.report_event(&key, EventKind::Accessed);

    println!("replica A before merge: weight={}", a.weight(&key));
    println!("replica B before merge: weight={}", b.weight(&key));

    let mut a1 = a.clone();
    let mut b1 = b.clone();
    sle_sync::merge(&mut a1, &mut b1, &provider);

    let mut b2 = b.clone();
    let mut a2 = a.clone();
    sle_sync::merge(&mut b2, &mut a2, &provider);

    println!("merged weight (A-then-B order): {}", a1.weight(&key));
    println!("merged weight (B-then-A order): {}", b2.weight(&key));

    if a1.state_digest() != b1.state_digest() || a1.state_digest() != a2.state_digest() || a1.state_digest() != b2.state_digest() {
        anyhow::bail!("replicas did not converge — this is a bug");
    }
    println!("convergence verified: both merge orders produce identical forest state.");
    Ok(())
}

fn serve() -> anyhow::Result<()> {
    use std::io::{BufRead, Write};
    eprintln!(
        "sle-cli serve: newline-delimited JSON stdio stub (NOT full LSP framing). \
         One JSON request per line in, one JSON response per line out. \
         Methods: ingest {{path}}, stats {{}}."
    );
    let mut forest = Forest::new(1);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = handle_request(&mut forest, &line);
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn handle_request(forest: &mut Forest, line: &str) -> String {
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return serde_json::json!({ "error": format!("invalid json: {e}") }).to_string(),
    };
    let method = value.get("method").and_then(|m| m.as_str()).unwrap_or("");
    match method {
        "ingest" => {
            let path = value.get("params").and_then(|p| p.get("path")).and_then(|p| p.as_str());
            let Some(path) = path else {
                return serde_json::json!({ "error": "missing params.path" }).to_string();
            };
            match ingest_file(forest, Path::new(path)) {
                Ok(Some(_)) => serde_json::json!({
                    "result": { "node_count": forest.node_count(), "edge_count": forest.edge_count() }
                })
                .to_string(),
                Ok(None) => serde_json::json!({ "error": format!("no adapter for {path}") }).to_string(),
                Err(e) => serde_json::json!({ "error": e.to_string() }).to_string(),
            }
        }
        "stats" => serde_json::json!({
            "result": { "node_count": forest.node_count(), "edge_count": forest.edge_count() }
        })
        .to_string(),
        other => serde_json::json!({ "error": format!("method not implemented: {other}") }).to_string(),
    }
}
