use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::class_registry::ClassRegistry;
use crate::errors::AnalysisError;
use crate::helpers;
use crate::tree_sitter_helpers::{HookType, find_hook_calls, find_member_accesses};

/// Analyze hook calls and generate diagnostics
pub fn analyze_hooks(
    text: &str,
    registry: &ClassRegistry,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut diagnostics = Vec::new();

    // Find all hook calls
    let hooks = find_hook_calls(root, text);

    for hook in hooks {
        // Validator 1: Check if target path exists
        diagnostics.extend(validate_hook_path(&hook, registry, text));

        // Validator 2: Check if accessed methods exist
        diagnostics.extend(validate_hook_methods(&hook, registry, text));

        // Validator 3: Suggest better hook type if appropriate
        diagnostics.extend(validate_hook_type(&hook, registry, text));
    }

    Ok(diagnostics)
}

/// Validate that the hook target path exists
fn validate_hook_path(
    hook: &crate::tree_sitter_helpers::HookCall,
    registry: &ClassRegistry,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Try to find the target class
    let target_class = registry.get_class_by_path(&hook.target_path);

    if target_class.is_none() {
        // Class not found - create error diagnostic
        let range = Range::new(
            helpers::position_at(text, hook.target_path_node.start_byte()),
            helpers::position_at(text, hook.target_path_node.end_byte()),
        );

        let mut message = format!("Class path '{}' not found", hook.target_path);

        // Add "did you mean?" suggestions
        let suggestions = registry.find_similar_paths(&hook.target_path);
        if !suggestions.is_empty() {
            message.push_str(". Did you mean: ");
            message.push_str(&suggestions.join(", "));
            message.push('?');
        }

        diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("squirrel-hook".to_string()),
            message,
            code: Some(tower_lsp::lsp_types::NumberOrString::String(
                "hook-path-not-found".to_string(),
            )),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

/// Validate that methods accessed in hooks exist
fn validate_hook_methods(
    hook: &crate::tree_sitter_helpers::HookCall,
    registry: &ClassRegistry,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get the target class
    let target_class = match registry.get_class_by_path(&hook.target_path) {
        Some(c) => c,
        None => return diagnostics, // Already handled by validate_hook_path
    };

    // Find all member accesses in the hook function
    let accesses = find_member_accesses(hook.hook_function, text);

    for access in accesses {
        // Check if the base is the hook parameter (typically 'o' or 'q')
        // Common pattern: local method = o.methodName
        // We're looking for accesses on the hook parameter

        // Simple heuristic: if base is a single letter (o, q, etc.), it's likely the hook param
        if access.base.len() == 1 {
            // Skip special Squirrel language features
            if access.member_name == "SuperName" {
                continue;
            }

            // Check if this method exists in the target class
            if !registry.has_method(&target_class.name, &access.member_name) {
                let range = Range::new(
                    helpers::position_at(text, access.member_node.start_byte()),
                    helpers::position_at(text, access.member_node.end_byte()),
                );

                let mut message = format!(
                    "Method '{}' not found in class '{}' or its ancestors",
                    access.member_name, target_class.name
                );

                // Add "did you mean?" suggestions
                let suggestions =
                    registry.find_similar_methods(&target_class.name, &access.member_name);
                if !suggestions.is_empty() {
                    message.push_str(". Did you mean: ");
                    message.push_str(&suggestions.join(", "));
                    message.push('?');
                }

                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("squirrel-hook".to_string()),
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

/// Suggest better hook type if appropriate
fn validate_hook_type(
    hook: &crate::tree_sitter_helpers::HookCall,
    registry: &ClassRegistry,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get the target class
    let target_class = match registry.get_class_by_path(&hook.target_path) {
        Some(c) => c,
        None => return diagnostics,
    };

    let has_children = !target_class.children.is_empty();
    let children_count = target_class.children.len();

    // Check for suboptimal hook types
    match hook.hook_type {
        HookType::Exact if has_children => {
            // Using hookExactClass on a base class with descendants
            let range = Range::new(
                helpers::position_at(text, hook.node.start_byte()),
                helpers::position_at(text, hook.node.end_byte()),
            );

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("squirrel-hook".to_string()),
                message: format!(
                    "Using 'hookExactClass' on '{}' which has {} descendant(s). Consider 'hookBaseClass' to affect all descendants.",
                    target_class.name, children_count
                ),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "hook-type-suggestion".to_string(),
                )),
                ..Diagnostic::default()
            });
        },
        HookType::Descendants if !has_children => {
            // Using hookDescendants on a leaf class
            let range = Range::new(
                helpers::position_at(text, hook.node.start_byte()),
                helpers::position_at(text, hook.node.end_byte()),
            );

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("squirrel-hook".to_string()),
                message: format!(
                    "Using 'hookDescendants' on '{}' which has no descendants. Consider 'hookExactClass'.",
                    target_class.name
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_registry::{ClassInfo, MemberInfo, MemberType};

    fn create_test_registry() -> ClassRegistry {
        let mut registry = ClassRegistry::new();

        // Register actor class with onDeath method
        registry.register_class(ClassInfo {
            name: "actor".to_string(),
            parent_path: None,
            parent: None,
            children: vec!["human".to_string()],
            members: vec![
                MemberInfo {
                    name: "onDeath".to_string(),
                    member_type: MemberType::Method,
                },
                MemberInfo {
                    name: "setFatigue".to_string(),
                    member_type: MemberType::Method,
                },
            ],
        });

        // Add path mapping
        registry
            .path_to_class
            .insert("entity/tactical/actor".to_string(), "actor".to_string());

        registry
    }

    #[test]
    fn test_valid_hook() {
        let registry = create_test_registry();

        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeath = o.onDeath;
            });
        "#;

        let diagnostics = analyze_hooks(code, &registry).unwrap();

        // Should have a warning about using hookExactClass on base class with children
        // but no errors about missing methods
        assert!(
            diagnostics
                .iter()
                .all(|d| d.severity != Some(DiagnosticSeverity::ERROR))
        );
    }

    #[test]
    fn test_invalid_hook_path() {
        let registry = create_test_registry();

        let code = r#"
            ::mods_hookExactClass("entity/tactical/aktor", function(o) {
                // typo in path
            });
        "#;

        let diagnostics = analyze_hooks(code, &registry).unwrap();

        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0].message.contains("not found"));
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn test_invalid_method_name() {
        let registry = create_test_registry();

        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeth = o.onDeth;  // typo in method name
            });
        "#;

        let diagnostics = analyze_hooks(code, &registry).unwrap();

        // Should have error about missing method
        let method_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == Some(DiagnosticSeverity::ERROR) && d.message.contains("onDeth")
            })
            .collect();

        assert!(!method_errors.is_empty());
        assert!(method_errors[0].message.contains("not found"));
    }

    #[test]
    fn test_hook_type_suggestion() {
        let registry = create_test_registry();

        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                // Using hookExactClass on actor which has children
            });
        "#;

        let diagnostics = analyze_hooks(code, &registry).unwrap();

        // Should have warning about hook type
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
            .collect();

        assert!(!warnings.is_empty());
        assert!(warnings[0].message.contains("hookBaseClass"));
    }
}
