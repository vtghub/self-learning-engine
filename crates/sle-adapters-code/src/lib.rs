//! A pluggable `Adapter` for source code, backed by tree-sitter. One
//! `CodeAdapter` instance is configured per language (a small table of
//! grammar node-kind names); adding a new language is a new `LangSpec`, not
//! a core change.
//!
//! Call resolution is deliberately simple: a two-pass walk first registers
//! every Function/Class by its bare name, then a second pass matches call
//! sites to that name table. This is best-effort by-name matching (no
//! scoping, imports, or overload resolution) — a real implementation would
//! layer something like stack-graphs on top of the same tree-sitter parse
//! for scope-correct resolution; this is enough to demonstrate a real
//! `Calls` edge without that scope.

use std::collections::HashMap;

use sle_core::{Adapter, DomainTag, EdgeKey, EdgeKind, Forest, NodeId, NodeKind, NodePayload, TreeId};
use tree_sitter::{Node, Parser};

pub struct LangSpec {
    pub domain: &'static str,
    pub function_kinds: &'static [&'static str],
    pub class_kinds: &'static [&'static str],
    pub import_kinds: &'static [&'static str],
    pub call_kinds: &'static [&'static str],
}

pub struct CodeAdapter {
    language: tree_sitter::Language,
    spec: LangSpec,
}

impl CodeAdapter {
    pub fn rust() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
            spec: LangSpec {
                domain: "code:rust",
                function_kinds: &["function_item"],
                class_kinds: &["struct_item", "impl_item", "trait_item", "mod_item"],
                import_kinds: &["use_declaration"],
                call_kinds: &["call_expression"],
            },
        }
    }

    pub fn python() -> Self {
        Self {
            language: tree_sitter_python::LANGUAGE.into(),
            spec: LangSpec {
                domain: "code:python",
                function_kinds: &["function_definition"],
                class_kinds: &["class_definition"],
                import_kinds: &["import_statement", "import_from_statement"],
                call_kinds: &["call"],
            },
        }
    }

    /// Picks an adapter from a file extension (without the dot), e.g. "rs" or "py".
    pub fn for_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::rust()),
            "py" => Some(Self::python()),
            _ => None,
        }
    }

    fn domain_tag(&self) -> DomainTag {
        DomainTag::new(self.spec.domain)
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        node: Node,
        source: &str,
        tree_id: &TreeId,
        forest: &mut Forest,
        container: NodeId,
        current_fn: NodeId,
        name_index: &mut HashMap<String, NodeId>,
        last_sibling: &mut HashMap<NodeId, NodeId>,
        pass: Pass,
    ) {
        let kind = node.kind();

        if self.spec.function_kinds.contains(&kind) {
            let name = child_name(node, source).unwrap_or_else(|| "anonymous".to_string());
            let fn_node = match pass {
                Pass::Define => {
                    let text = node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
                    let n = forest.add_node(NodePayload {
                        tree: tree_id.clone(),
                        kind: NodeKind::new("Function"),
                        domain: self.domain_tag(),
                        label: name.clone(),
                        text,
                        data: serde_json::json!({}),
                        embedding: None,
                    });
                    link_sibling(forest, container, n, last_sibling);
                    name_index.insert(name, n);
                    n
                }
                Pass::Link => *name_index.get(&name).unwrap_or(&current_fn),
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk(child, source, tree_id, forest, container, fn_node, name_index, last_sibling, pass);
            }
            return;
        }

        if self.spec.class_kinds.contains(&kind) {
            let name = child_name(node, source).unwrap_or_else(|| kind.to_string());
            let class_node = match pass {
                Pass::Define => {
                    let n = forest.add_node(NodePayload {
                        tree: tree_id.clone(),
                        kind: NodeKind::new("Class"),
                        domain: self.domain_tag(),
                        label: name.clone(),
                        text: String::new(),
                        data: serde_json::json!({}),
                        embedding: None,
                    });
                    link_sibling(forest, container, n, last_sibling);
                    name_index.insert(name, n);
                    n
                }
                Pass::Link => *name_index.get(&name).unwrap_or(&container),
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk(
                    child, source, tree_id, forest, class_node, current_fn, name_index, last_sibling, pass,
                );
            }
            return;
        }

        if self.spec.import_kinds.contains(&kind) {
            if matches!(pass, Pass::Define) {
                let text = node.utf8_text(source.as_bytes()).unwrap_or("").trim().to_string();
                let import_node = forest.add_node(NodePayload {
                    tree: tree_id.clone(),
                    kind: NodeKind::new("Import"),
                    domain: self.domain_tag(),
                    label: text.clone(),
                    text,
                    data: serde_json::json!({}),
                    embedding: None,
                });
                link_sibling(forest, container, import_node, last_sibling);
            }
            return;
        }

        if self.spec.call_kinds.contains(&kind) && matches!(pass, Pass::Link) {
            if let Some(callee) = callee_name(node, source) {
                if let Some(target) = name_index.get(&callee).copied() {
                    forest.add_edge(
                        EdgeKey { src: current_fn, dst: target, kind: EdgeKind::new("Calls") },
                        1,
                    );
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, source, tree_id, forest, container, current_fn, name_index, last_sibling, pass);
        }
    }
}

#[derive(Clone, Copy)]
enum Pass {
    Define,
    Link,
}

fn child_name(node: Node, source: &str) -> Option<String> {
    for field in ["name", "type"] {
        if let Some(child) = node.child_by_field_name(field) {
            if let Ok(text) = child.utf8_text(source.as_bytes()) {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn callee_name(node: Node, source: &str) -> Option<String> {
    let func_field = node.child_by_field_name("function")?;
    let text = func_field.utf8_text(source.as_bytes()).ok()?;
    Some(text.rsplit(['.', ':']).next().unwrap_or(text).trim().to_string())
}

fn link_sibling(forest: &mut Forest, parent: NodeId, node: NodeId, last_sibling: &mut HashMap<NodeId, NodeId>) {
    forest.add_edge(EdgeKey { src: parent, dst: node, kind: EdgeKind::new("Contains") }, 1);
    if let Some(prev) = last_sibling.get(&parent).copied() {
        forest.add_edge(EdgeKey { src: prev, dst: node, kind: EdgeKind::new("Precedes") }, 1);
    }
    last_sibling.insert(parent, node);
}

impl Adapter for CodeAdapter {
    fn domain(&self) -> DomainTag {
        self.domain_tag()
    }

    fn ingest(&self, tree_id: &TreeId, source: &str, forest: &mut Forest) -> anyhow::Result<NodeId> {
        let mut parser = Parser::new();
        parser.set_language(&self.language)?;
        let tree = parser
            .parse(source, None)
            .ok_or_else(|| anyhow::anyhow!("tree-sitter failed to parse {tree_id}"))?;

        let module = forest.add_node(NodePayload {
            tree: tree_id.clone(),
            kind: NodeKind::new("Module"),
            domain: self.domain_tag(),
            label: tree_id.clone(),
            text: String::new(),
            data: serde_json::json!({}),
            embedding: None,
        });

        let mut name_index: HashMap<String, NodeId> = HashMap::new();
        let mut last_sibling: HashMap<NodeId, NodeId> = HashMap::new();
        let root = tree.root_node();

        self.walk(root, source, tree_id, forest, module, module, &mut name_index, &mut last_sibling, Pass::Define);
        self.walk(root, source, tree_id, forest, module, module, &mut name_index, &mut last_sibling, Pass::Link);

        Ok(module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_functions_and_calls() {
        let mut forest = Forest::new(1);
        let adapter = CodeAdapter::rust();
        let source = r#"
            fn helper() -> i32 { 42 }

            fn main() {
                let x = helper();
                println!("{}", x);
            }
        "#;
        let module = adapter.ingest(&"main.rs".to_string(), source, &mut forest).unwrap();

        let labels: Vec<String> = forest
            .nodes()
            .filter(|(_, p)| p.kind.to_string() == "Function")
            .map(|(_, p)| p.label.clone())
            .collect();
        assert!(labels.contains(&"helper".to_string()));
        assert!(labels.contains(&"main".to_string()));

        let calls: Vec<_> = forest.edges().filter(|(k, _)| k.kind.to_string() == "Calls").collect();
        assert_eq!(calls.len(), 1, "main should call helper exactly once: {calls:?}");

        let module_edges = forest.edges_from(&module);
        assert!(module_edges.iter().any(|(k, _)| k.kind.to_string() == "Contains"));
    }

    #[test]
    fn python_functions_and_classes() {
        let mut forest = Forest::new(1);
        let adapter = CodeAdapter::python();
        let source = "class Greeter:\n    def greet(self):\n        return hello()\n\ndef hello():\n    return 'hi'\n";
        adapter.ingest(&"main.py".to_string(), source, &mut forest).unwrap();

        let class_labels: Vec<String> = forest
            .nodes()
            .filter(|(_, p)| p.kind.to_string() == "Class")
            .map(|(_, p)| p.label.clone())
            .collect();
        assert!(class_labels.contains(&"Greeter".to_string()));

        let calls: Vec<_> = forest.edges().filter(|(k, _)| k.kind.to_string() == "Calls").collect();
        assert_eq!(calls.len(), 1, "greet should call hello: {calls:?}");
    }
}
