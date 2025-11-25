use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::class_registry::ClassRegistry;
use crate::errors::AnalysisError;
use crate::helpers;
use crate::tree_sitter_helpers::find_inherit_calls;

/// Analyze inherit() calls and generate diagnostics
pub fn analyze_inheritance(
    text: &str,
    registry: &ClassRegistry,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut diagnostics = Vec::new();

    // Find all inherit() calls
    let inherits = find_inherit_calls(root, text);

    for inherit_call in inherits {
        // Validate that parent path exists
        diagnostics.extend(validate_parent_path(&inherit_call, registry, text));

        // Check for circular inheritance (requires the current class to be registered)
        diagnostics.extend(check_circular_inheritance(&inherit_call, registry, text));
    }

    Ok(diagnostics)
}

/// Validate that the parent class path exists
fn validate_parent_path(
    inherit_call: &crate::tree_sitter_helpers::InheritCall,
    registry: &ClassRegistry,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Try to find the parent class
    let parent_class = registry.get_class_by_path(&inherit_call.parent_path);

    if parent_class.is_none() {
        // Parent class not found - create error diagnostic
        let range = Range::new(
            helpers::position_at(text, inherit_call.parent_path_node.start_byte()),
            helpers::position_at(text, inherit_call.parent_path_node.end_byte()),
        );

        let mut message = format!("Parent class '{}' not found", inherit_call.parent_path);

        // Add "did you mean?" suggestions
        let suggestions = registry.find_similar_paths(&inherit_call.parent_path);
        if !suggestions.is_empty() {
            message.push_str(". Did you mean: ");
            message.push_str(&suggestions.join(", "));
            message.push('?');
        }

        diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("squirrel-inheritance".to_string()),
            message,
            code: Some(tower_lsp::lsp_types::NumberOrString::String(
                "parent-class-not-found".to_string(),
            )),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

/// Check for circular inheritance
/// Note: This is a simplified check. Full circular inheritance detection requires
/// the current class to already be registered in the registry.
fn check_circular_inheritance(
    inherit_call: &crate::tree_sitter_helpers::InheritCall,
    registry: &ClassRegistry,
    text: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get the parent class
    let parent_class = match registry.get_class_by_path(&inherit_call.parent_path) {
        Some(c) => c,
        None => return diagnostics, // Already handled by validate_parent_path
    };

    // Check if the current class name appears in the parent's ancestor chain
    let ancestors = registry.get_ancestors(&parent_class.name);

    for ancestor in ancestors {
        if ancestor.name == inherit_call.class_name {
            // Circular inheritance detected!
            let range = Range::new(
                helpers::position_at(text, inherit_call.node.start_byte()),
                helpers::position_at(text, inherit_call.node.end_byte()),
            );

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("squirrel-inheritance".to_string()),
                message: format!(
                    "Circular inheritance detected: '{}' → '{}' → ... → '{}'",
                    inherit_call.class_name, parent_class.name, inherit_call.class_name
                ),
                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                    "circular-inheritance".to_string(),
                )),
                ..Diagnostic::default()
            });
            break;
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::class_registry::ClassInfo;

    fn create_test_registry() -> ClassRegistry {
        let mut registry = ClassRegistry::new();

        // Register actor class
        registry.register_class(ClassInfo {
            name: "actor".to_string(),
            parent_path: None,
            parent: None,
            children: vec![],
            members: vec![],
        });

        // Register human class (inherits from actor)
        registry.register_class(ClassInfo {
            name: "human".to_string(),
            parent_path: Some("scripts/entity/tactical/actor".to_string()),
            parent: Some("actor".to_string()),
            children: vec![],
            members: vec![],
        });

        // Add path mappings
        registry
            .path_to_class
            .insert("entity/tactical/actor".to_string(), "actor".to_string());
        registry
            .path_to_class
            .insert("entity/tactical/human".to_string(), "human".to_string());

        registry.build_inheritance_graph();

        registry
    }

    #[test]
    fn test_valid_inheritance() {
        let registry = create_test_registry();

        let code = r#"
            barbarian <- inherit("scripts/entity/tactical/human", {
                function create() {
                    human.create();
                }
            });
        "#;

        let diagnostics = analyze_inheritance(code, &registry).unwrap();

        // Should have no errors
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_invalid_parent_path() {
        let registry = create_test_registry();

        let code = r#"
            barbarian <- inherit("scripts/entity/tactical/humam", {
                // typo in parent path
            });
        "#;

        let diagnostics = analyze_inheritance(code, &registry).unwrap();

        assert!(!diagnostics.is_empty());
        assert!(diagnostics[0].message.contains("not found"));
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    }

    #[test]
    fn test_circular_inheritance() {
        let mut registry = create_test_registry();

        // Create a circular dependency: actor -> human -> actor
        // Update actor to inherit from human (creating the circle)
        registry.register_class(ClassInfo {
            name: "actor".to_string(),
            parent_path: Some("scripts/entity/tactical/human".to_string()),
            parent: Some("human".to_string()),
            children: vec![],
            members: vec![],
        });

        registry.build_inheritance_graph();

        let code = r#"
            human <- inherit("scripts/entity/tactical/actor", {
                // This creates a circular dependency
            });
        "#;

        let diagnostics = analyze_inheritance(code, &registry).unwrap();

        // Should detect circular inheritance
        let circular_errors: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Circular"))
            .collect();

        assert!(!circular_errors.is_empty());
    }
}
