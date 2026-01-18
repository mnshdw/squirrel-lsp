use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers;
use crate::workspace::Workspace;

/// Pattern: `identifier <- inherit("path/to/parent", { body })`
#[derive(Debug, Clone)]
pub struct InheritCall<'tree> {
    pub class_name: String,
    pub parent_path: String,
    pub parent_path_node: Node<'tree>,
    pub class_body: Node<'tree>,
}

pub fn find_inherit_calls<'tree>(root: Node<'tree>, text: &str) -> Vec<InheritCall<'tree>> {
    let mut results = Vec::new();
    let mut cursor = root.walk();

    fn walk_tree<'tree>(
        node: Node<'tree>,
        text: &str,
        results: &mut Vec<InheritCall<'tree>>,
        cursor: &mut tree_sitter::TreeCursor<'tree>,
    ) {
        if node.kind() == "update_expression" {
            let mut has_new_slot_op = false;
            let mut class_name = String::new();
            let mut call_expr = None;

            for child in node.children(cursor) {
                if (child.kind() == "identifier" || child.kind() == "deref_expression")
                    && class_name.is_empty()
                {
                    if let Some(name) = helpers::extract_identifier_name(child, text) {
                        class_name = name;
                    }
                } else if child.kind() == "<-" {
                    has_new_slot_op = true;
                } else if child.kind() == "call_expression" {
                    call_expr = Some(child);
                }
            }

            if has_new_slot_op
                && !class_name.is_empty()
                && let Some(call) = call_expr
                && let Some(inherit) = parse_inherit_call(call, text)
            {
                results.push(InheritCall {
                    class_name,
                    parent_path: inherit.0,
                    parent_path_node: inherit.1,
                    class_body: inherit.2,
                });
            }
        }

        for child in node.children(cursor) {
            walk_tree(child, text, results, &mut child.walk());
        }
    }

    walk_tree(root, text, &mut results, &mut cursor);
    results
}

fn parse_inherit_call<'tree>(
    call: Node<'tree>,
    text: &str,
) -> Option<(String, Node<'tree>, Node<'tree>)> {
    let mut is_inherit = false;
    let mut parent_path = String::new();
    let mut path_node = None;
    let mut body_node = None;

    for child in call.children(&mut call.walk()) {
        match child.kind() {
            "identifier" => {
                if get_node_text(child, text) == "inherit" {
                    is_inherit = true;
                }
            },
            "deref_expression" => {
                if let Some(last) = helpers::find_last_identifier(child)
                    && get_node_text(last, text) == "inherit"
                {
                    is_inherit = true;
                }
            },
            "call_args" => {
                for arg in child.children(&mut child.walk()) {
                    match arg.kind() {
                        "string" => {
                            if path_node.is_none() {
                                parent_path = helpers::extract_string_content(arg, text);
                                path_node = Some(arg);
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

    if is_inherit
        && let Some(pn) = path_node
        && let Some(bn) = body_node
    {
        Some((parent_path, pn, bn))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    Exact,
    Base,
    Descendants,
    NewObject,
    NewObjectOnce,
}

#[derive(Debug, Clone)]
pub struct HookCall<'tree> {
    pub node: Node<'tree>,
    pub hook_type: HookType,
    pub target_path: String,
    pub target_path_node: Node<'tree>,
    pub hook_function: Node<'tree>,
    pub hook_param_name: Option<String>,
}

pub fn find_hook_calls<'tree>(root: Node<'tree>, text: &str) -> Vec<HookCall<'tree>> {
    let mut results = Vec::new();
    let mut cursor = root.walk();

    fn walk_tree<'tree>(
        node: Node<'tree>,
        text: &str,
        results: &mut Vec<HookCall<'tree>>,
        cursor: &mut tree_sitter::TreeCursor<'tree>,
    ) {
        if node.kind() == "call_expression"
            && let Some(hook) = parse_hook_call(node, text)
        {
            results.push(hook);
        }

        for child in node.children(cursor) {
            walk_tree(child, text, results, &mut child.walk());
        }
    }

    walk_tree(root, text, &mut results, &mut cursor);
    results
}

fn parse_hook_call<'tree>(call: Node<'tree>, text: &str) -> Option<HookCall<'tree>> {
    let mut hook_type = None;
    let mut target_path = String::new();
    let mut path_node = None;
    let mut hook_function = None;

    for child in call.children(&mut call.walk()) {
        match child.kind() {
            "global_variable" => {
                for gchild in child.children(&mut child.walk()) {
                    if gchild.kind() == "identifier" {
                        let name = get_node_text(gchild, text);
                        hook_type = match name {
                            "mods_hookExactClass" => Some(HookType::Exact),
                            "mods_hookBaseClass" => Some(HookType::Base),
                            "mods_hookDescendants" => Some(HookType::Descendants),
                            "mods_hookNewObject" => Some(HookType::NewObject),
                            "mods_hookNewObjectOnce" => Some(HookType::NewObjectOnce),
                            _ => None,
                        };
                    }
                }
            },
            "call_args" => {
                let mut found_path = false;
                for arg in child.children(&mut child.walk()) {
                    if arg.kind() == "string" && !found_path {
                        target_path = helpers::extract_string_content(arg, text);
                        path_node = Some(arg);
                        found_path = true;
                    } else if (arg.kind() == "lambda_expression"
                        || arg.kind() == "anonymous_function")
                        && found_path
                    {
                        hook_function = Some(arg);
                    }
                }
            },
            _ => {},
        }
    }

    if let Some(ht) = hook_type
        && let Some(pn) = path_node
        && let Some(hf) = hook_function
    {
        // Extract the parameter name from the hook function
        let hook_param_name = extract_first_param_name(hf, text);

        Some(HookCall {
            node: call,
            hook_type: ht,
            target_path,
            target_path_node: pn,
            hook_function: hf,
            hook_param_name,
        })
    } else {
        None
    }
}

fn extract_first_param_name(func_node: Node, text: &str) -> Option<String> {
    for child in func_node.children(&mut func_node.walk()) {
        if child.kind() == "parameters" || child.kind() == "function_parameters" {
            for param in child.children(&mut child.walk()) {
                if param.kind() == "identifier" {
                    return Some(get_node_text(param, text).to_string());
                } else if param.kind() == "parameter" {
                    for p_child in param.children(&mut param.walk()) {
                        if p_child.kind() == "identifier" {
                            return Some(get_node_text(p_child, text).to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

pub fn analyze_hooks(text: &str, workspace: &Workspace) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut diagnostics = Vec::new();
    let hooks = find_hook_calls(root, text);

    for hook in hooks {
        diagnostics.extend(validate_hook_path(&hook, workspace, text));
        diagnostics.extend(validate_hook_methods(&hook, workspace, text));
        diagnostics.extend(validate_hook_type(&hook, workspace, text));
    }

    Ok(diagnostics)
}

fn validate_hook_path(hook: &HookCall, workspace: &Workspace, text: &str) -> Vec<Diagnostic> {
    if workspace.get(&hook.target_path).is_some() {
        return Vec::new();
    }

    let range = Range::new(
        helpers::position_at(text, hook.target_path_node.start_byte()),
        helpers::position_at(text, hook.target_path_node.end_byte()),
    );

    let mut message = format!("Script path '{}' not found", hook.target_path);
    let suggestions = workspace.find_similar_paths(&hook.target_path);
    if !suggestions.is_empty() {
        message.push_str(". Did you mean: ");
        message.push_str(&suggestions.join(", "));
        message.push('?');
    }

    vec![Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("squirrel-bb-hook".to_string()),
        message,
        code: Some(tower_lsp::lsp_types::NumberOrString::String(
            "hook-path-not-found".to_string(),
        )),
        ..Diagnostic::default()
    }]
}

fn validate_hook_methods(hook: &HookCall, workspace: &Workspace, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let target_entry = match workspace.get(&hook.target_path) {
        Some(e) => e,
        None => return diagnostics,
    };

    let param_name = match &hook.hook_param_name {
        Some(name) => name,
        None => return diagnostics,
    };

    let accesses = find_member_accesses(hook.hook_function, text);

    for access in accesses {
        if access.base == *param_name {
            if access.member_name == "SuperName" {
                continue;
            }

            if !workspace.has_member(&hook.target_path, &access.member_name) {
                let range = Range::new(
                    helpers::position_at(text, access.member_node.start_byte()),
                    helpers::position_at(text, access.member_node.end_byte()),
                );

                let mut message = format!(
                    "Method '{}' not found in '{}' or its ancestors",
                    access.member_name, target_entry.name
                );

                let suggestions =
                    workspace.find_similar_methods(&hook.target_path, &access.member_name);
                if !suggestions.is_empty() {
                    message.push_str(". Did you mean: ");
                    message.push_str(&suggestions.join(", "));
                    message.push('?');
                }

                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("squirrel-bb-hook".to_string()),
                    message,
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        "method-not-found".to_string(),
                    )),
                    ..Diagnostic::default()
                });
            }
        }
    }

    diagnostics
}

fn validate_hook_type(hook: &HookCall, workspace: &Workspace, text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let target_entry = match workspace.get(&hook.target_path) {
        Some(e) => e,
        None => return diagnostics,
    };

    let has_children = !target_entry.children.is_empty();
    let children_count = target_entry.children.len();

    match hook.hook_type {
        HookType::Exact if has_children => {
            let range = first_line_range(hook.node, text);

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("squirrel-bb-hook".to_string()),
                message: format!(
                    "Using 'hookExactClass' on '{}' which has {} descendant(s). Consider 'hookBaseClass' to affect all descendants.",
                    target_entry.name, children_count
                ),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "hook-type-suggestion".to_string(),
                )),
                ..Diagnostic::default()
            });
        },
        HookType::Descendants if !has_children => {
            let range = first_line_range(hook.node, text);

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("squirrel-bb-hook".to_string()),
                message: format!(
                    "Using 'hookDescendants' on '{}' which has no descendants. Consider 'hookExactClass'.",
                    target_entry.name
                ),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "hook-type-no-descendants".to_string(),
                )),
                ..Diagnostic::default()
            });
        },
        _ => {},
    }

    diagnostics
}

/// Returns a range covering only the first line of the node
fn first_line_range(node: Node, text: &str) -> Range {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte();
    let first_line_end = text[start_byte..end_byte]
        .find('\n')
        .map(|i| start_byte + i)
        .unwrap_or(end_byte);
    Range::new(
        helpers::position_at(text, start_byte),
        helpers::position_at(text, first_line_end),
    )
}

pub fn analyze_inheritance(
    text: &str,
    workspace: &Workspace,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut diagnostics = Vec::new();
    let inherits = find_inherit_calls(root, text);

    for inherit_call in inherits {
        diagnostics.extend(validate_parent_path(&inherit_call, workspace, text));
        diagnostics.extend(check_circular_inheritance(&inherit_call, workspace, text));
    }

    Ok(diagnostics)
}

fn validate_parent_path(
    inherit: &InheritCall,
    workspace: &Workspace,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let lookup_path = inherit
        .parent_path
        .strip_prefix("scripts/")
        .unwrap_or(&inherit.parent_path);

    if workspace.get(lookup_path).is_none() {
        let range = Range::new(
            helpers::position_at(text, inherit.parent_path_node.start_byte()),
            helpers::position_at(text, inherit.parent_path_node.end_byte()),
        );

        let mut message = format!("Parent path '{}' not found", inherit.parent_path);
        let suggestions = workspace.find_similar_paths(&inherit.parent_path);
        if !suggestions.is_empty() {
            message.push_str(". Did you mean: ");
            message.push_str(&suggestions.join(", "));
            message.push('?');
        }

        diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("squirrel-inherit".to_string()),
            message,
            code: Some(tower_lsp::lsp_types::NumberOrString::String(
                "parent-path-not-found".to_string(),
            )),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

fn check_circular_inheritance(
    inherit: &InheritCall,
    workspace: &Workspace,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let class_name = &inherit.class_name;

    let lookup_path = inherit
        .parent_path
        .strip_prefix("scripts/")
        .unwrap_or(&inherit.parent_path);

    if let Some(parent_entry) = workspace.get(lookup_path) {
        let ancestors = workspace.get_ancestors(lookup_path);

        if parent_entry.name == *class_name {
            let range = Range::new(
                helpers::position_at(text, inherit.parent_path_node.start_byte()),
                helpers::position_at(text, inherit.parent_path_node.end_byte()),
            );

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("squirrel-inherit".to_string()),
                message: format!("'{}' cannot inherit from itself", class_name),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "circular-inheritance".to_string(),
                )),
                ..Diagnostic::default()
            });
        } else if ancestors.iter().any(|a| a.name == *class_name) {
            let range = Range::new(
                helpers::position_at(text, inherit.parent_path_node.start_byte()),
                helpers::position_at(text, inherit.parent_path_node.end_byte()),
            );

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("squirrel-inherit".to_string()),
                message: format!(
                    "Circular inheritance detected: '{}' appears in its own ancestor chain",
                    class_name
                ),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "circular-inheritance".to_string(),
                )),
                ..Diagnostic::default()
            });
        }
    }

    diagnostics
}

#[derive(Debug, Clone)]
pub struct MemberAccess<'tree> {
    pub base: String,
    pub member_name: String,
    pub member_node: Node<'tree>,
}

pub fn find_member_accesses<'tree>(root: Node<'tree>, text: &str) -> Vec<MemberAccess<'tree>> {
    let mut results = Vec::new();
    let mut cursor = root.walk();

    fn walk<'tree>(
        node: Node<'tree>,
        text: &str,
        results: &mut Vec<MemberAccess<'tree>>,
        cursor: &mut tree_sitter::TreeCursor<'tree>,
    ) {
        if node.kind() == "deref_expression" {
            let mut base = String::new();
            let mut member_name = String::new();
            let mut member_node = None;

            for child in node.children(cursor) {
                if child.kind() == "identifier" {
                    if base.is_empty() {
                        base = get_node_text(child, text).to_string();
                    } else {
                        member_name = get_node_text(child, text).to_string();
                        member_node = Some(child);
                    }
                }
            }

            if !base.is_empty()
                && !member_name.is_empty()
                && let Some(member_node) = member_node
            {
                results.push(MemberAccess {
                    base,
                    member_name,
                    member_node,
                });
            }
        }

        for child in node.children(cursor) {
            walk(child, text, results, &mut child.walk());
        }
    }

    walk(root, text, &mut results, &mut cursor);
    results
}

pub fn get_node_text<'a>(node: Node, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn create_test_workspace() -> Workspace {
        let mut workspace = Workspace::new();

        // Index actor with children
        let actor_code = r#"
            this.actor <- this.inherit("scripts/entity/base", {
                function onDeath() {}
                function setFatigue(_f) {}
            });
        "#;
        workspace
            .index_file(
                Path::new("/test/scripts/entity/tactical/actor.nut"),
                actor_code,
            )
            .unwrap();

        // Index human that inherits from actor
        let human_code = r#"
            this.human <- this.inherit("scripts/entity/tactical/actor", {
                function onTurnStart() {}
            });
        "#;
        workspace
            .index_file(
                Path::new("/test/scripts/entity/tactical/human.nut"),
                human_code,
            )
            .unwrap();

        workspace.build_inheritance_graph();
        workspace
    }

    #[test]
    fn test_find_inherit_calls() {
        let code = r#"
            this.knight <- this.inherit("scripts/entity/tactical/human", {
                function create() {}
            });
        "#;

        let tree = helpers::parse_squirrel(code).unwrap();
        let calls = find_inherit_calls(tree.root_node(), code);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].class_name, "knight");
        assert_eq!(calls[0].parent_path, "scripts/entity/tactical/human");
    }

    #[test]
    fn test_find_hook_calls() {
        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeath = o.onDeath;
            });
        "#;

        let tree = helpers::parse_squirrel(code).unwrap();
        let calls = find_hook_calls(tree.root_node(), code);

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].hook_type, HookType::Exact);
        assert_eq!(calls[0].target_path, "entity/tactical/actor");
    }

    #[test]
    fn test_valid_hook() {
        let workspace = create_test_workspace();
        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeath = o.onDeath;
            });
        "#;

        let diagnostics = analyze_hooks(code, &workspace).unwrap();
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != Some(DiagnosticSeverity::ERROR))
        );
    }

    #[test]
    fn test_invalid_hook_path() {
        let workspace = create_test_workspace();
        let code = r#"
            ::mods_hookExactClass("entity/tactical/aktor", function(o) {});
        "#;

        let diagnostics = analyze_hooks(code, &workspace).unwrap();
        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0].message.contains("not found"));
    }

    #[test]
    fn test_invalid_method_name() {
        let workspace = create_test_workspace();
        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeth = o.onDeth;
            });
        "#;

        let diagnostics = analyze_hooks(code, &workspace).unwrap();
        let method_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR) && d.message.contains("onDeth")
            })
            .collect();

        assert!(!method_errors.is_empty());
    }

    #[test]
    fn test_hook_type_suggestion() {
        let workspace = create_test_workspace();
        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {});
        "#;

        let diagnostics = analyze_hooks(code, &workspace).unwrap();
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
            .collect();

        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("hookBaseClass"));
    }

    #[test]
    fn test_valid_inheritance() {
        let workspace = create_test_workspace();
        let code = r#"
            this.knight <- this.inherit("scripts/entity/tactical/actor", {
                function create() {}
            });
        "#;

        let diagnostics = analyze_inheritance(code, &workspace).unwrap();
        // No errors for valid parent path
        let errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn test_invalid_parent_path() {
        let workspace = create_test_workspace();
        let code = r#"
            this.knight <- this.inherit("scripts/entity/tactical/aktor", {
                function create() {}
            });
        "#;

        let diagnostics = analyze_inheritance(code, &workspace).unwrap();
        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0].message.contains("not found"));
    }

    #[test]
    fn test_inherit_from_self() {
        let mut workspace = Workspace::new();

        // Index a class that tries to inherit from itself
        let knight_code = r#"
            this.knight <- this.inherit("scripts/entity/tactical/knight", {
                function create() {}
            });
        "#;
        workspace
            .index_file(
                Path::new("/test/scripts/entity/tactical/knight.nut"),
                knight_code,
            )
            .unwrap();
        workspace.build_inheritance_graph();

        let diagnostics = analyze_inheritance(knight_code, &workspace).unwrap();
        assert!(!diagnostics.is_empty());
        assert!(
            diagnostics[0]
                .message
                .contains("cannot inherit from itself")
        );
    }
}
