//! A pluggable `Adapter` for prose (Markdown or plain text). Builds a
//! Document → Section → Paragraph tree using heading nesting for
//! `Contains` edges and reading order for `Precedes` edges — the same
//! adapter contract that `sle-adapters-code` implements for source code, so
//! both feed the same core engine.

use std::collections::HashMap;

use sle_core::{Adapter, DomainTag, EdgeKey, EdgeKind, Forest, NodeId, NodeKind, NodePayload, TreeId};

pub struct TextAdapter;

impl TextAdapter {
    fn domain_tag() -> DomainTag {
        DomainTag::new("text:markdown")
    }
}

impl Adapter for TextAdapter {
    fn domain(&self) -> DomainTag {
        Self::domain_tag()
    }

    fn ingest(&self, tree_id: &TreeId, source: &str, forest: &mut Forest) -> anyhow::Result<NodeId> {
        let root = forest.add_node(NodePayload {
            tree: tree_id.clone(),
            kind: NodeKind::new("Document"),
            domain: Self::domain_tag(),
            label: tree_id.clone(),
            text: source.to_string(),
            data: serde_json::json!({}),
            embedding: None,
        });

        let mut stack: Vec<(u32, NodeId)> = vec![(0, root)];
        let mut last_sibling: HashMap<NodeId, NodeId> = HashMap::new();
        let mut para_buf: Vec<String> = Vec::new();

        for line in source.lines() {
            if let Some((level, title)) = heading_level(line) {
                flush_paragraph(&mut para_buf, forest, &stack, &mut last_sibling, tree_id);

                while stack.last().expect("root always present").0 >= level {
                    stack.pop();
                }
                let parent = stack.last().expect("root always present").1;

                let node = forest.add_node(NodePayload {
                    tree: tree_id.clone(),
                    kind: NodeKind::new("Section"),
                    domain: Self::domain_tag(),
                    label: title.clone(),
                    text: title,
                    data: serde_json::json!({ "level": level }),
                    embedding: None,
                });
                link_sibling(forest, parent, node, &mut last_sibling);
                stack.push((level, node));
            } else if line.trim().is_empty() {
                flush_paragraph(&mut para_buf, forest, &stack, &mut last_sibling, tree_id);
            } else {
                para_buf.push(line.to_string());
            }
        }
        flush_paragraph(&mut para_buf, forest, &stack, &mut last_sibling, tree_id);

        Ok(root)
    }
}

fn heading_level(line: &str) -> Option<(u32, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let bytes = trimmed.as_bytes();
    if hashes == bytes.len() {
        return Some((hashes as u32, String::new()));
    }
    if bytes[hashes] == b' ' {
        return Some((hashes as u32, trimmed[hashes..].trim().to_string()));
    }
    None
}

fn link_sibling(
    forest: &mut Forest,
    parent: NodeId,
    node: NodeId,
    last_sibling: &mut HashMap<NodeId, NodeId>,
) {
    forest.add_edge(EdgeKey { src: parent, dst: node, kind: EdgeKind::new("Contains") }, 1);
    if let Some(prev) = last_sibling.get(&parent).copied() {
        forest.add_edge(EdgeKey { src: prev, dst: node, kind: EdgeKind::new("Precedes") }, 1);
    }
    last_sibling.insert(parent, node);
}

fn flush_paragraph(
    buf: &mut Vec<String>,
    forest: &mut Forest,
    stack: &[(u32, NodeId)],
    last_sibling: &mut HashMap<NodeId, NodeId>,
    tree_id: &TreeId,
) {
    if buf.is_empty() {
        return;
    }
    let text = buf.join("\n");
    let parent = stack.last().expect("root always present").1;
    let label: String = text.chars().take(48).collect();
    let node = forest.add_node(NodePayload {
        tree: tree_id.clone(),
        kind: NodeKind::new("Paragraph"),
        domain: TextAdapter::domain_tag(),
        label,
        text,
        data: serde_json::json!({}),
        embedding: None,
    });
    link_sibling(forest, parent, node, last_sibling);
    buf.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_document_section_paragraph_tree() {
        let mut forest = Forest::new(1);
        let adapter = TextAdapter;
        let source = "# Title\n\nIntro paragraph.\n\n## Sub\n\nSub paragraph one.\nstill same paragraph.\n\nSub paragraph two.\n";
        let root = adapter.ingest(&"doc1".to_string(), source, &mut forest).unwrap();

        let kinds: Vec<String> =
            forest.nodes().map(|(_, p)| p.kind.to_string()).collect();
        assert_eq!(kinds.iter().filter(|k| *k == "Document").count(), 1);
        assert_eq!(kinds.iter().filter(|k| *k == "Section").count(), 2);
        assert_eq!(kinds.iter().filter(|k| *k == "Paragraph").count(), 3);

        let root_out_edges = forest.edges_from(&root);
        assert!(root_out_edges.iter().any(|(k, _)| k.kind.to_string() == "Contains"));
    }
}
