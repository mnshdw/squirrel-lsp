//! Extracts symbols from a Squirrel AST.
//!
//! This module walks the AST and builds a SymbolMap representing
//! all the definitions in a file.

use tower_lsp::lsp_types::Position;
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers;
use crate::symbols::{FileSymbols, Symbol, SymbolKind, SymbolMap, Table, extract_script_path};

/// Extract symbols from a Squirrel file
pub fn extract_file_symbols(file_path: &str, text: &str) -> Result<FileSymbols, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let script_path = extract_script_path(file_path);
    let mut file_symbols = FileSymbols {
        path: script_path.clone(),
        symbols: SymbolMap::new(),
        main_table: None,
    };

    // Process top-level statements
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        process_top_level_node(child, text, &mut file_symbols, &script_path);
    }

    Ok(file_symbols)
}

fn process_top_level_node(
    node: Node,
    text: &str,
    file_symbols: &mut FileSymbols,
    script_path: &str,
) {
    if node.kind() == "update_expression" {
        process_update_expression(node, text, file_symbols, script_path);
    }
}

fn process_update_expression(
    node: Node,
    text: &str,
    file_symbols: &mut FileSymbols,
    script_path: &str,
) {
    let mut has_new_slot = false;
    let mut lhs_node: Option<Node> = None;
    let mut rhs_node: Option<Node> = None;

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "<-" => has_new_slot = true,
            "identifier" if lhs_node.is_none() => lhs_node = Some(child),
            "deref_expression" if lhs_node.is_none() => lhs_node = Some(child),
            _ if has_new_slot && rhs_node.is_none() => rhs_node = Some(child),
            _ => {},
        }
    }

    if !has_new_slot {
        return;
    }

    let Some(lhs) = lhs_node else { return };
    let Some(rhs) = rhs_node else { return };

    let name = extract_lhs_name(lhs, text);
    let Some(name) = name else { return };

    let symbol_kind = extract_symbol_from_value(rhs, text);
    let pos = Position {
        line: lhs.start_position().row as u32,
        character: lhs.start_position().column as u32,
    };

    if file_symbols.main_table.is_none()
        && let SymbolKind::Table {
            ref parent,
            ref slots,
        } = symbol_kind
    {
        file_symbols.main_table = Some(Table {
            name: name.clone(),
            path: script_path.to_string(),
            parent: parent.clone(),
            slots: slots.clone(),
            defined_at: pos,
        });
    }

    let symbol = Symbol {
        kind: symbol_kind,
        defined_at: pos,
    };

    file_symbols.symbols.insert(name, symbol);
}

fn extract_lhs_name(node: Node, text: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(helpers::node_text(node, text).to_string()),
        "deref_expression" => {
            let mut last_ident = None;
            for child in node.children(&mut node.walk()) {
                if child.kind() == "identifier" {
                    last_ident = Some(child);
                } else if child.kind() == "deref_expression"
                    && let Some(name) = extract_lhs_name(child, text)
                {
                    return Some(name);
                }
            }
            last_ident.map(|n| helpers::node_text(n, text).to_string())
        },
        _ => None,
    }
}

fn extract_symbol_from_value(node: Node, text: &str) -> SymbolKind {
    match node.kind() {
        "table" => {
            let slots = extract_table_slots(node, text);
            SymbolKind::Table {
                parent: None,
                slots,
            }
        },
        "call_expression" => {
            if let Some((parent_path, body)) = extract_inherit_call(node, text) {
                let slots = if let Some(body_node) = body {
                    extract_table_slots(body_node, text)
                } else {
                    SymbolMap::new()
                };
                SymbolKind::Table {
                    parent: Some(parent_path),
                    slots,
                }
            } else {
                SymbolKind::Variable
            }
        },
        "function_declaration" | "lambda_expression" | "anonymous_function" => {
            let params = extract_function_params(node, text);
            SymbolKind::Function { params }
        },
        _ => SymbolKind::Variable,
    }
}

fn extract_table_slots(table_node: Node, text: &str) -> SymbolMap {
    let mut slots = SymbolMap::new();

    for child in table_node.children(&mut table_node.walk()) {
        match child.kind() {
            "table_slots" => {
                // Recurse into table_slots wrapper node
                for slot_child in child.children(&mut child.walk()) {
                    extract_slot_into(&mut slots, slot_child, text);
                }
            },
            _ => extract_slot_into(&mut slots, child, text),
        }
    }

    slots
}

fn extract_slot_into(slots: &mut SymbolMap, node: Node, text: &str) {
    match node.kind() {
        "assignment_expression" => {
            if let Some((name, symbol)) = extract_assignment_slot(node, text) {
                slots.insert(name, symbol);
            }
        },
        "table_slot" => {
            if let Some((name, symbol)) = extract_table_slot(node, text) {
                slots.insert(name, symbol);
            }
        },
        _ => {},
    }
}

fn extract_assignment_slot(node: Node, text: &str) -> Option<(String, Symbol)> {
    let left = node.child_by_field_name("left")?;
    let right = node.child_by_field_name("right")?;

    if left.kind() != "identifier" {
        return None;
    }

    let name = helpers::node_text(left, text).to_string();
    let kind = extract_symbol_from_value(right, text);
    let pos = Position {
        line: left.start_position().row as u32,
        character: left.start_position().column as u32,
    };

    Some((
        name,
        Symbol {
            kind,
            defined_at: pos,
        },
    ))
}

fn extract_table_slot(slot_node: Node, text: &str) -> Option<(String, Symbol)> {
    let mut name: Option<String> = None;
    let mut value_node: Option<Node> = None;
    let mut name_node: Option<Node> = None;

    for child in slot_node.children(&mut slot_node.walk()) {
        match child.kind() {
            "identifier" if name.is_none() => {
                name = Some(helpers::node_text(child, text).to_string());
                name_node = Some(child);
            },
            "function_declaration" => {
                if let Some(fn_name_node) = find_first_identifier(child) {
                    name = Some(helpers::node_text(fn_name_node, text).to_string());
                    name_node = Some(fn_name_node);
                }
                value_node = Some(child);
            },
            "=" | "," => {},
            _ if value_node.is_none() && name.is_some() => {
                value_node = Some(child);
            },
            _ => {},
        }
    }

    let name = name?;
    let pos_node = name_node.unwrap_or(slot_node);
    let pos = Position {
        line: pos_node.start_position().row as u32,
        character: pos_node.start_position().column as u32,
    };

    let kind = if let Some(value) = value_node {
        extract_symbol_from_value(value, text)
    } else {
        SymbolKind::Variable
    };

    Some((
        name,
        Symbol {
            kind,
            defined_at: pos,
        },
    ))
}

fn extract_inherit_call<'a>(node: Node<'a>, text: &str) -> Option<(String, Option<Node<'a>>)> {
    let mut is_inherit = false;
    let mut parent_path: Option<String> = None;
    let mut body_node: Option<Node> = None;

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "identifier" => {
                if helpers::node_text(child, text) == "inherit" {
                    is_inherit = true;
                }
            },
            "deref_expression" => {
                let mut last_ident = "";
                for subchild in child.children(&mut child.walk()) {
                    if subchild.kind() == "identifier" {
                        last_ident = helpers::node_text(subchild, text);
                    }
                }
                if last_ident == "inherit" {
                    is_inherit = true;
                }
            },
            "call_args" => {
                for arg in child.children(&mut child.walk()) {
                    match arg.kind() {
                        "string" => {
                            if parent_path.is_none() {
                                parent_path = Some(helpers::extract_string_content(arg, text));
                            }
                        },
                        "table" => {
                            body_node = Some(arg);
                        },
                        _ => {},
                    }
                }
            },
            _ => {},
        }
    }

    if is_inherit {
        Some((parent_path.unwrap_or_default(), body_node))
    } else {
        None
    }
}

fn extract_function_params(node: Node, text: &str) -> Vec<String> {
    let mut params = Vec::new();

    for child in node.children(&mut node.walk()) {
        if child.kind() == "parameters" {
            for param in child.children(&mut child.walk()) {
                if param.kind() == "parameter"
                    && let Some(ident) = find_first_identifier(param)
                {
                    params.push(helpers::node_text(ident, text).to_string());
                }
            }
        }
    }

    params
}

fn find_first_identifier(node: Node) -> Option<Node> {
    node.children(&mut node.walk())
        .find(|&child| child.kind() == "identifier")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_table() {
        let code = r#"
            skill <- {
                m = { Container = null },
                function getContainer() {
                    return m.Container;
                }
            };
        "#;
        let file_symbols = extract_file_symbols("scripts/skills/skill.nut", code).unwrap();

        assert!(file_symbols.main_table.is_some());
        assert_eq!(file_symbols.main_table.as_ref().unwrap().name, "skill");
        assert!(file_symbols.symbols.contains_key("skill"));

        let skill = &file_symbols.symbols["skill"];
        if let SymbolKind::Table { parent, slots } = &skill.kind {
            assert!(parent.is_none());
            assert!(slots.contains_key("m"));
            assert!(slots.contains_key("getContainer"));
        } else {
            panic!("Expected Table, got {:?}", skill.kind);
        }
    }

    #[test]
    fn test_inherit_call() {
        let code = r#"
            this.perk_legend_ambidextrous <- this.inherit("scripts/skills/skill", {
                function onAdded() {
                    local x = 1;
                }
            });
        "#;
        let file_symbols =
            extract_file_symbols("scripts/skills/perks/perk_legend_ambidextrous.nut", code)
                .unwrap();

        assert!(file_symbols.main_table.is_some());
        let main_table = file_symbols.main_table.as_ref().unwrap();
        assert_eq!(main_table.name, "perk_legend_ambidextrous");
        assert_eq!(main_table.path, "skills/perks/perk_legend_ambidextrous");
        assert_eq!(main_table.parent.as_deref(), Some("scripts/skills/skill"));

        let perk = &file_symbols.symbols["perk_legend_ambidextrous"];
        if let SymbolKind::Table { parent, slots } = &perk.kind {
            assert_eq!(parent.as_deref(), Some("scripts/skills/skill"));
            assert!(slots.contains_key("onAdded"));
        } else {
            panic!("Expected Table, got {:?}", perk.kind);
        }
    }

    #[test]
    fn test_nested_tables() {
        let code = r#"
            my_class <- {
                m = {
                    offHandSkill = null,
                    HandToHand = null
                },
                function setOffhandSkill(_a) {
                    this.m.offHandSkill = _a;
                }
            };
        "#;
        let file_symbols = extract_file_symbols("test.nut", code).unwrap();

        let my_class = &file_symbols.symbols["my_class"];
        if let SymbolKind::Table { parent: _, slots } = &my_class.kind {
            assert!(slots.contains_key("m"));
            assert!(slots.contains_key("setOffhandSkill"));

            let m = &slots["m"];
            if let SymbolKind::Table {
                parent: _,
                slots: m_slots,
            } = &m.kind
            {
                assert!(m_slots.contains_key("offHandSkill"));
                assert!(m_slots.contains_key("HandToHand"));
            } else {
                panic!("Expected nested Table for 'm'");
            }
        } else {
            panic!("Expected Table");
        }
    }
}
