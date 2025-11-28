//! Navigation support for Go to Definition, Find References, etc.

use std::path::Path;

use tower_lsp::lsp_types::{
    DocumentSymbol, Location, Position, Range, SymbolInformation, SymbolKind, Url,
};
use tree_sitter::Node;

use crate::helpers;
use crate::workspace::{MemberType, Workspace};

#[derive(Debug)]
enum SymbolAtPosition {
    InheritParentPath(String),
    MethodCall(String),
    FunctionDeclaration(),
    Identifier(String),
}

fn find_symbol_at_position(text: &str, position: Position) -> Option<SymbolAtPosition> {
    let tree = helpers::parse_squirrel(text).ok()?;
    let root = tree.root_node();

    let byte_offset = byte_offset_at(text, position)?;
    let node = find_deepest_node_at(root, byte_offset)?;

    classify_node(node, text)
}

fn find_deepest_node_at(node: Node, byte_offset: usize) -> Option<Node> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    for child in node.children(&mut node.walk()) {
        if let Some(deeper) = find_deepest_node_at(child, byte_offset) {
            return Some(deeper);
        }
    }

    Some(node)
}

fn classify_node(node: Node, text: &str) -> Option<SymbolAtPosition> {
    let node_text = node.utf8_text(text.as_bytes()).ok()?;

    match node.kind() {
        "string" | "string_content" => {
            if is_inside_inherit_call(node, text) {
                let path = node_text.trim_matches('"').to_string();
                return Some(SymbolAtPosition::InheritParentPath(path));
            }
            None
        },
        "identifier" => {
            if let Some(parent) = node.parent() {
                match parent.kind() {
                    "deref_expression" => {
                        if let Some(grandparent) = parent.parent()
                            && grandparent.kind() == "call_expression"
                        {
                            return Some(SymbolAtPosition::MethodCall(node_text.to_string()));
                        }
                        return Some(SymbolAtPosition::MethodCall(node_text.to_string()));
                    },
                    "call_expression" => {
                        return Some(SymbolAtPosition::MethodCall(node_text.to_string()));
                    },
                    "function_declaration" => {
                        return Some(SymbolAtPosition::FunctionDeclaration());
                    },
                    _ => {},
                }
            }
            Some(SymbolAtPosition::Identifier(node_text.to_string()))
        },
        _ => None,
    }
}

fn is_inside_inherit_call(node: Node, source: &str) -> bool {
    let source_bytes = source.as_bytes();
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind() == "call_expression" {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "identifier" {
                    let name = child.utf8_text(source_bytes).unwrap_or("");
                    if name == "inherit" {
                        return true;
                    }
                }
                if child.kind() == "deref_expression" {
                    for deref_child in child.children(&mut child.walk()) {
                        if deref_child.kind() == "identifier"
                            && let Ok(name) = deref_child.utf8_text(source_bytes)
                            && name == "inherit"
                        {
                            return true;
                        }
                    }
                }
            }
        }
        current = parent;
    }
    false
}

fn byte_offset_at(text: &str, position: Position) -> Option<usize> {
    let mut line = 0u32;
    let mut col = 0u32;
    let mut byte_offset = 0;

    for (i, ch) in text.char_indices() {
        if line == position.line && col == position.character {
            return Some(i);
        }

        if ch == '\n' {
            if line == position.line {
                return Some(i);
            }
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
        byte_offset = i + ch.len_utf8();
    }

    if line == position.line && col == position.character {
        return Some(byte_offset);
    }

    None
}

pub struct DefinitionResult {
    pub file_path: std::path::PathBuf,
    pub line: u32,
    pub column: u32,
}

pub fn find_definition(
    text: &str,
    position: Position,
    current_file: &Path,
    workspace: &Workspace,
) -> Option<DefinitionResult> {
    let symbol = find_symbol_at_position(text, position)?;

    match symbol {
        SymbolAtPosition::InheritParentPath(path) => {
            let normalized = path.trim_start_matches("scripts/").trim_end_matches(".nut");

            if let Some(entry) = workspace.get(normalized) {
                return Some(DefinitionResult {
                    file_path: entry.file_path.clone(),
                    line: 0,
                    column: 0,
                });
            }
        },
        SymbolAtPosition::MethodCall(method_name) | SymbolAtPosition::Identifier(method_name) => {
            let script_path = extract_script_path(current_file);

            if !script_path.is_empty()
                && let Some((file_path, line, column)) =
                    workspace.find_method_definition(&script_path, &method_name)
            {
                return Some(DefinitionResult {
                    file_path: file_path.clone(),
                    line,
                    column,
                });
            }

            let results = workspace.find_method_anywhere(&method_name);
            if let Some((file_path, line, column, _)) = results.first() {
                return Some(DefinitionResult {
                    file_path: (*file_path).clone(),
                    line: *line,
                    column: *column,
                });
            }
        },
        SymbolAtPosition::FunctionDeclaration() => {
            return None;
        },
    }

    None
}

fn extract_script_path(file_path: &Path) -> String {
    let path_str = file_path.to_string_lossy();

    if let Some(scripts_idx) = path_str.find("scripts/") {
        let after_scripts = &path_str[scripts_idx + 8..];
        return after_scripts.trim_end_matches(".nut").to_string();
    }

    String::new()
}

pub fn definition_to_location(result: DefinitionResult) -> Option<Location> {
    let uri = Url::from_file_path(&result.file_path).ok()?;
    let position = Position::new(result.line, result.column);
    Some(Location {
        uri,
        range: Range::new(position, position),
    })
}

pub fn get_document_symbols(text: &str) -> Vec<DocumentSymbol> {
    let tree = match helpers::parse_squirrel(text) {
        Ok(tree) => tree,
        Err(_) => return Vec::new(),
    };

    let root = tree.root_node();
    let mut symbols = Vec::new();

    for child in root.children(&mut root.walk()) {
        if let Some(symbol) = extract_symbol_from_node(child, text) {
            symbols.push(symbol);
        }
    }

    symbols
}

fn extract_symbol_from_node(node: Node, text: &str) -> Option<DocumentSymbol> {
    match node.kind() {
        "update_expression" => {
            let mut name = None;
            let mut table_node = None;
            let mut is_class = false;

            for child in node.children(&mut node.walk()) {
                match child.kind() {
                    "identifier" | "deref_expression" if name.is_none() => {
                        name = helpers::extract_identifier_name(child, text);
                    },
                    "call_expression" => {
                        for call_child in child.children(&mut child.walk()) {
                            if call_child.kind() == "identifier"
                                || call_child.kind() == "deref_expression"
                            {
                                let call_text = call_child.utf8_text(text.as_bytes()).unwrap_or("");
                                if call_text.contains("inherit") {
                                    is_class = true;
                                    for arg in child.children(&mut child.walk()) {
                                        if arg.kind() == "table" {
                                            table_node = Some(arg);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "table" => {
                        table_node = Some(child);
                    },
                    _ => {},
                }
            }

            let name = name?;
            let start = node.start_position();
            let end = node.end_position();

            let range = Range::new(
                Position::new(start.row as u32, start.column as u32),
                Position::new(end.row as u32, end.column as u32),
            );

            let children = table_node.map(|t| extract_table_members(t, text));

            Some(DocumentSymbol {
                name,
                detail: None,
                kind: if is_class {
                    SymbolKind::CLASS
                } else {
                    SymbolKind::VARIABLE
                },
                tags: None,
                #[allow(deprecated)]
                deprecated: None,
                range,
                selection_range: range,
                children,
            })
        },
        "function_declaration" => {
            let name = node
                .child_by_field_name("name")
                .or_else(|| {
                    node.children(&mut node.walk())
                        .find(|c| c.kind() == "identifier")
                })
                .map(|n| n.utf8_text(text.as_bytes()).unwrap_or("").to_string())?;

            let start = node.start_position();
            let end = node.end_position();
            let range = Range::new(
                Position::new(start.row as u32, start.column as u32),
                Position::new(end.row as u32, end.column as u32),
            );

            Some(DocumentSymbol {
                name,
                detail: None,
                kind: SymbolKind::FUNCTION,
                tags: None,
                #[allow(deprecated)]
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            })
        },
        "local_declaration" => {
            let name = node
                .children(&mut node.walk())
                .find(|c| c.kind() == "identifier")
                .map(|n| n.utf8_text(text.as_bytes()).unwrap_or("").to_string())?;

            let start = node.start_position();
            let end = node.end_position();
            let range = Range::new(
                Position::new(start.row as u32, start.column as u32),
                Position::new(end.row as u32, end.column as u32),
            );

            Some(DocumentSymbol {
                name,
                detail: None,
                kind: SymbolKind::VARIABLE,
                tags: None,
                #[allow(deprecated)]
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            })
        },
        _ => None,
    }
}

fn extract_table_members(node: Node, text: &str) -> Vec<DocumentSymbol> {
    let mut members = Vec::new();

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "function_declaration" => {
                let name = child
                    .child_by_field_name("name")
                    .or_else(|| {
                        child
                            .children(&mut child.walk())
                            .find(|c| c.kind() == "identifier")
                    })
                    .map(|n| n.utf8_text(text.as_bytes()).unwrap_or("").to_string());

                if let Some(name) = name {
                    let start = child.start_position();
                    let end = child.end_position();
                    let range = Range::new(
                        Position::new(start.row as u32, start.column as u32),
                        Position::new(end.row as u32, end.column as u32),
                    );

                    members.push(DocumentSymbol {
                        name,
                        detail: None,
                        kind: SymbolKind::METHOD,
                        tags: None,
                        #[allow(deprecated)]
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            },
            "table_slot" => {
                if let Some(key) = child.child_by_field_name("key") {
                    let is_function = child.child_by_field_name("value").is_some_and(|v| {
                        v.kind() == "lambda_expression" || v.kind() == "anonymous_function"
                    });

                    if is_function {
                        let name = key.utf8_text(text.as_bytes()).unwrap_or("").to_string();
                        let start = child.start_position();
                        let end = child.end_position();
                        let range = Range::new(
                            Position::new(start.row as u32, start.column as u32),
                            Position::new(end.row as u32, end.column as u32),
                        );

                        members.push(DocumentSymbol {
                            name,
                            detail: None,
                            kind: SymbolKind::METHOD,
                            tags: None,
                            #[allow(deprecated)]
                            deprecated: None,
                            range,
                            selection_range: range,
                            children: None,
                        });
                    }
                } else {
                    for slot_child in child.children(&mut child.walk()) {
                        if slot_child.kind() == "function_declaration" {
                            let name = slot_child
                                .child_by_field_name("name")
                                .or_else(|| {
                                    slot_child
                                        .children(&mut slot_child.walk())
                                        .find(|c| c.kind() == "identifier")
                                })
                                .map(|n| n.utf8_text(text.as_bytes()).unwrap_or("").to_string());

                            if let Some(name) = name {
                                let start = slot_child.start_position();
                                let end = slot_child.end_position();
                                let range = Range::new(
                                    Position::new(start.row as u32, start.column as u32),
                                    Position::new(end.row as u32, end.column as u32),
                                );

                                members.push(DocumentSymbol {
                                    name,
                                    detail: None,
                                    kind: SymbolKind::METHOD,
                                    tags: None,
                                    #[allow(deprecated)]
                                    deprecated: None,
                                    range,
                                    selection_range: range,
                                    children: None,
                                });
                            }
                        }
                    }
                }
            },
            "assignment_expression" => {
                let mut name = None;
                let mut nested_table = None;

                for c in child.children(&mut child.walk()) {
                    if c.kind() == "identifier" && name.is_none() {
                        name = Some(c.utf8_text(text.as_bytes()).unwrap_or("").to_string());
                    } else if c.kind() == "table" {
                        nested_table = Some(c);
                    }
                }

                if let (Some(name), Some(_)) = (name, nested_table) {
                    let start = child.start_position();
                    let end = child.end_position();
                    let range = Range::new(
                        Position::new(start.row as u32, start.column as u32),
                        Position::new(end.row as u32, end.column as u32),
                    );

                    members.push(DocumentSymbol {
                        name,
                        detail: None,
                        kind: SymbolKind::FIELD,
                        tags: None,
                        #[allow(deprecated)]
                        deprecated: None,
                        range,
                        selection_range: range,
                        children: None,
                    });
                }
            },
            _ => {
                members.extend(extract_table_members(child, text));
            },
        }
    }

    members
}

pub fn get_workspace_symbols(query: &str, workspace: &Workspace) -> Vec<SymbolInformation> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for (script_path, entry) in workspace.files() {
        if entry.name.to_lowercase().contains(&query_lower)
            && let Ok(uri) = Url::from_file_path(&entry.file_path)
        {
            results.push(SymbolInformation {
                name: entry.name.clone(),
                kind: SymbolKind::CLASS,
                tags: None,
                #[allow(deprecated)]
                deprecated: None,
                location: Location {
                    uri,
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                },
                container_name: Some(script_path.clone()),
            });
        }

        for member in &entry.members {
            if member.name.to_lowercase().contains(&query_lower)
                && let Ok(uri) = Url::from_file_path(&entry.file_path)
            {
                let kind = match member.member_type {
                    MemberType::Method => SymbolKind::METHOD,
                    MemberType::Field => SymbolKind::FIELD,
                };
                results.push(SymbolInformation {
                    name: member.name.clone(),
                    kind,
                    tags: None,
                    #[allow(deprecated)]
                    deprecated: None,
                    location: Location {
                        uri,
                        range: Range::new(
                            Position::new(member.line, member.column),
                            Position::new(member.line, member.column),
                        ),
                    },
                    container_name: Some(entry.name.clone()),
                });
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_symbol_in_inherit() {
        let code = r#"this.foo <- this.inherit("scripts/skills/skill", {});"#;
        let pos = Position::new(0, 30); // Inside the string

        let symbol = find_symbol_at_position(code, pos);
        assert!(matches!(
            symbol,
            Some(SymbolAtPosition::InheritParentPath(_))
        ));
    }

    #[test]
    fn test_find_method_call() {
        let code = r#"this.getContainer().getActor();"#;
        let pos = Position::new(0, 7); // On "getContainer"

        let symbol = find_symbol_at_position(code, pos);
        assert!(matches!(symbol, Some(SymbolAtPosition::MethodCall(_))));
    }
}

#[cfg(test)]
mod symbol_tests {
    use super::*;

    #[test]
    fn test_document_symbols_real_file() {
        let content =
            std::fs::read_to_string("/home/antoine/bb-ws/base_bb/scripts/skills/skill.nut")
                .expect("Should read file");

        let symbols = get_document_symbols(&content);
        eprintln!("Document symbols count: {}", symbols.len());
        for s in &symbols {
            eprintln!("  - {} ({:?})", s.name, s.kind);
            if let Some(children) = &s.children {
                eprintln!("    Children count: {}", children.len());
                for c in children.iter().take(5) {
                    eprintln!("      - {} ({:?})", c.name, c.kind);
                }
                if children.len() > 5 {
                    eprintln!("      ... and {} more", children.len() - 5);
                }
            } else {
                eprintln!("    No children!");
            }
        }

        assert!(!symbols.is_empty(), "Should have document symbols");
    }
}
