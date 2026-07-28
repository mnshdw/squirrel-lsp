//! Navigation support for Go to Definition, Find References, etc.

use std::path::Path;

use tower_lsp::lsp_types::{
    DocumentSymbol, Location, Position, Range, SymbolInformation, SymbolKind, Url,
};
use tree_sitter::Node;

use crate::helpers;
use crate::workspace::{MemberType, Workspace};

#[derive(Debug, PartialEq, Eq)]
enum SymbolAtPosition {
    /// A string naming another file: `inherit("scripts/…")`, `::mods_hookExactClass("…")`.
    ScriptPath(String),
    /// A member access, `base.name`. The base is carried along because it is what says
    /// *which* class the member belongs to.
    Member {
        base: Option<String>,
        name: String,
    },
    /// A bare identifier: a class name, a global, a local.
    Name(String),
    FunctionDeclaration,
}

fn find_symbol_at_position(text: &str, position: Position) -> Option<SymbolAtPosition> {
    let tree = helpers::parse_squirrel(text).ok()?;
    let root = tree.root_node();

    let byte_offset = byte_offset_at(text, position)?;
    let node = find_deepest_node_at(root, byte_offset)?;

    classify_node(node, text)
}

/// The functions whose first string argument names another script.
fn names_a_script_path(callee: &str) -> bool {
    matches!(
        callee,
        "inherit"
            | "mods_hookExactClass"
            | "mods_hookBaseClass"
            | "mods_hookDescendants"
            | "mods_hookNewObject"
            | "mods_hookNewObjectOnce"
    )
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
            if is_inside_script_path_call(node, text) {
                let path = node_text.trim_matches('"').to_string();
                return Some(SymbolAtPosition::ScriptPath(path));
            }
            None
        },
        "identifier" => {
            let Some(parent) = node.parent() else {
                return Some(SymbolAtPosition::Name(node_text.to_string()));
            };

            match parent.kind() {
                "function_declaration" => Some(SymbolAtPosition::FunctionDeclaration),

                // Either side of a `.` lands here, and they mean opposite things: in
                // `rotu_mod_aura_abstract.create()`, the left is the class being referred
                // to and the right is a method of it. Treating both as a method name (as
                // this used to) makes Go to Definition on a class name find nothing.
                "deref_expression" => {
                    if is_member_of_deref(node) {
                        Some(SymbolAtPosition::Member {
                            base: deref_base_name(node, text),
                            name: node_text.to_string(),
                        })
                    } else {
                        Some(SymbolAtPosition::Name(node_text.to_string()))
                    }
                },

                // A bare call, `create()`: a method on whatever class we are inside.
                "call_expression" => Some(SymbolAtPosition::Member {
                    base: None,
                    name: node_text.to_string(),
                }),

                _ => Some(SymbolAtPosition::Name(node_text.to_string())),
            }
        },
        _ => None,
    }
}

/// True when this identifier is on the right of the `.` (or `::`), i.e. it is the member
/// being accessed rather than the thing being accessed.
fn is_member_of_deref(node: Node) -> bool {
    node.prev_sibling()
        .is_some_and(|prev| matches!(prev.kind(), "." | "::"))
}

/// Name of the thing a member is being read from: the identifier just left of the `.`.
fn deref_base_name(member: Node, text: &str) -> Option<String> {
    let dot = member.prev_sibling()?;
    let base = dot.prev_sibling()?;

    // `foo.bar`      -> "foo"
    // `this.m.Rage`  -> "m"   (the immediate base; not resolvable, and that is fine)
    let ident = helpers::find_last_identifier(base)?;
    Some(ident.utf8_text(text.as_bytes()).ok()?.to_string())
}

/// Is this string the path argument of a call that names another script?
fn is_inside_script_path_call(node: Node, source: &str) -> bool {
    let source_bytes = source.as_bytes();
    let mut current = node;

    while let Some(parent) = current.parent() {
        if parent.kind() == "call_expression" {
            for child in parent.children(&mut parent.walk()) {
                let callee = match child.kind() {
                    "identifier" => child.utf8_text(source_bytes).ok().map(str::to_string),
                    "deref_expression" | "global_variable" => helpers::find_last_identifier(child)
                        .and_then(|n| n.utf8_text(source_bytes).ok())
                        .map(str::to_string),
                    _ => None,
                };

                if callee.is_some_and(|name| names_a_script_path(&name)) {
                    return true;
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

/// The class a file is editing, when the cursor sits inside a `::mods_hook*` callback.
struct HookContext {
    target_path: String,
    param_name: Option<String>,
}

fn enclosing_hook(text: &str, byte_offset: usize) -> Option<HookContext> {
    let tree = helpers::parse_squirrel(text).ok()?;

    crate::bb_support::find_hook_calls(tree.root_node(), text)
        .into_iter()
        .find(|hook| {
            byte_offset >= hook.hook_function.start_byte()
                && byte_offset <= hook.hook_function.end_byte()
        })
        .map(|hook| HookContext {
            target_path: hook.target_path,
            param_name: hook.hook_param_name,
        })
}

/// All the places a symbol could be defined, best first.
pub fn find_definitions(
    text: &str,
    position: Position,
    current_file: &Path,
    workspace: &Workspace,
) -> Vec<DefinitionResult> {
    let Some(symbol) = find_symbol_at_position(text, position) else {
        return Vec::new();
    };

    let hook = byte_offset_at(text, position).and_then(|offset| enclosing_hook(text, offset));

    match symbol {
        SymbolAtPosition::ScriptPath(path) => workspace
            .resolve_all(&path)
            .iter()
            .map(|entry| DefinitionResult {
                file_path: entry.file_path.clone(),
                line: entry.line,
                column: entry.column,
            })
            .collect(),

        SymbolAtPosition::Name(name) => {
            // A class or table by that name: `rotu_mod_aura_abstract`, `orc_warlord`.
            let by_name: Vec<DefinitionResult> = workspace
                .find_by_name(&name)
                .iter()
                .map(|entry| DefinitionResult {
                    file_path: entry.file_path.clone(),
                    line: entry.line,
                    column: entry.column,
                })
                .collect();

            if !by_name.is_empty() {
                return by_name;
            }

            // Otherwise it may still be a method used without a receiver.
            find_member(workspace, current_file, hook.as_ref(), None, &name)
        },

        SymbolAtPosition::Member { base, name } => find_member(
            workspace,
            current_file,
            hook.as_ref(),
            base.as_deref(),
            &name,
        ),

        SymbolAtPosition::FunctionDeclaration => Vec::new(),
    }
}

/// Resolve `base.name` by working out which class `base` refers to.
fn find_member(
    workspace: &Workspace,
    current_file: &Path,
    hook: Option<&HookContext>,
    base: Option<&str>,
    name: &str,
) -> Vec<DefinitionResult> {
    let enclosing_class = || match hook {
        Some(h) => h.target_path.clone(),
        None => workspace.script_path_for(current_file),
    };

    let owner: Option<String> = match base {
        Some(b) if hook.is_some_and(|h| h.param_name.as_deref() == Some(b)) => {
            Some(enclosing_class())
        },
        Some("this") => Some(enclosing_class()),
        Some(b) => workspace
            .find_by_name(b)
            .first()
            .map(|entry| entry.script_path.clone()),
        None => Some(enclosing_class()),
    };

    if let Some(owner) = owner.filter(|o| !o.is_empty()) {
        if workspace.contains(&owner) {
            return workspace
                .find_method_definition(&owner, name)
                .map(|(file_path, line, column)| DefinitionResult {
                    file_path: file_path.clone(),
                    line,
                    column,
                })
                .into_iter()
                .collect();
        }

        if hook.is_some() {
            return Vec::new();
        }
    }

    workspace
        .find_method_anywhere(name)
        .into_iter()
        .map(|(file_path, line, column, _)| DefinitionResult {
            file_path: file_path.clone(),
            line,
            column,
        })
        .collect()
}

/// The single best definition. The server uses [`find_definitions`], which can offer
/// several when a name is genuinely defined in more than one place.
#[allow(
    dead_code,
    reason = "used by tests and by callers wanting a single answer"
)]
pub fn find_definition(
    text: &str,
    position: Position,
    current_file: &Path,
    workspace: &Workspace,
) -> Option<DefinitionResult> {
    find_definitions(text, position, current_file, workspace)
        .into_iter()
        .next()
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
        assert!(matches!(symbol, Some(SymbolAtPosition::ScriptPath(_))));
    }

    #[test]
    fn test_find_symbol_in_global_inherit() {
        // `::inherit(…)` is a global_variable, not a deref_expression.
        let code = r#"this.foo <- ::inherit("scripts/skills/skill", {});"#;
        let pos = Position::new(0, 27);

        let symbol = find_symbol_at_position(code, pos);
        assert_eq!(
            symbol,
            Some(SymbolAtPosition::ScriptPath(
                "scripts/skills/skill".to_string()
            ))
        );
    }

    #[test]
    fn test_find_symbol_in_hook_path() {
        let code = r#"::mods_hookExactClass("entity/tactical/actor", function(o) {});"#;
        let pos = Position::new(0, 25);

        let symbol = find_symbol_at_position(code, pos);
        assert_eq!(
            symbol,
            Some(SymbolAtPosition::ScriptPath(
                "entity/tactical/actor".to_string()
            ))
        );
    }

    #[test]
    fn test_find_method_call() {
        let code = r#"this.getContainer().getActor();"#;
        let pos = Position::new(0, 7); // On "getContainer"

        let symbol = find_symbol_at_position(code, pos);
        assert!(matches!(symbol, Some(SymbolAtPosition::Member { .. })));
    }

    #[test]
    fn test_deref_base_is_a_name_not_a_method() {
        let code = r#"function f() { aura_abstract.create(); }"#;
        let pos = Position::new(0, 16);

        let symbol = find_symbol_at_position(code, pos);
        assert_eq!(
            symbol,
            Some(SymbolAtPosition::Name("aura_abstract".to_string()))
        );
    }
}

#[cfg(test)]
mod symbol_tests {
    use super::*;

    #[test]
    #[ignore]
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
