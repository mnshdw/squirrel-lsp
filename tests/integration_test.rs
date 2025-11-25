use squirrel_lsp::class_registry::{ClassInfo, ClassRegistry, MemberInfo, MemberType};
use squirrel_lsp::hook_analyzer::analyze_hooks;
use squirrel_lsp::inheritance_analyzer::analyze_inheritance;
use tower_lsp::lsp_types::DiagnosticSeverity;

/// Create a test registry with Battle Brothers-like class hierarchy
fn create_bb_registry() -> ClassRegistry {
    let mut registry = ClassRegistry::new();

    // Register actor (base class)
    registry.register_class(ClassInfo {
        name: "actor".to_string(),
        parent_path: None,
        parent: None,
        children: vec![],
        members: vec![
            MemberInfo { name: "onDeath".to_string(), member_type: MemberType::Method },
            MemberInfo { name: "onDamageReceived".to_string(), member_type: MemberType::Method },
            MemberInfo { name: "setFatigue".to_string(), member_type: MemberType::Method },
        ],
    });

    // Register human (inherits from actor)
    registry.register_class(ClassInfo {
        name: "human".to_string(),
        parent_path: Some("scripts/entity/tactical/actor".to_string()),
        parent: None,
        children: vec![],
        members: vec![
            MemberInfo { name: "create".to_string(), member_type: MemberType::Method },
        ],
    });

    // Register barbarian_thrall (inherits from human)
    registry.register_class(ClassInfo {
        name: "barbarian_thrall".to_string(),
        parent_path: Some("scripts/entity/tactical/human".to_string()),
        parent: None,
        children: vec![],
        members: vec![
            MemberInfo { name: "create".to_string(), member_type: MemberType::Method },
        ],
    });

    // Add path mappings
    registry.path_to_class.insert("entity/tactical/actor".to_string(), "actor".to_string());
    registry.path_to_class.insert("entity/tactical/human".to_string(), "human".to_string());
    registry.path_to_class.insert(
        "entity/tactical/humans/barbarian_thrall".to_string(),
        "barbarian_thrall".to_string(),
    );

    // Build inheritance graph
    registry.build_inheritance_graph();

    registry
}

#[test]
fn test_valid_hook_exact_class() {
    let registry = create_bb_registry();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeath = o.onDeath;
            o.onDeath = function(_killer, _skill, _tile, _fatalityType) {
                onDeath(_killer, _skill, _tile, _fatalityType);
                // Custom logic
            };
        });
    "#;

    let diagnostics = analyze_hooks(code, &registry).expect("Hook analysis should succeed");

    // Should have a warning about using hookExactClass on base class with children
    // but no errors
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert_eq!(errors.len(), 0, "Should have no errors for valid hook");
}

#[test]
fn test_invalid_hook_path() {
    let registry = create_bb_registry();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/aktor", function(o) {
            // typo: "aktor" instead of "actor"
        });
    "#;

    let diagnostics = analyze_hooks(code, &registry).expect("Hook analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert_eq!(errors.len(), 1, "Should have one error for invalid path");
    assert!(
        errors[0].message.contains("not found"),
        "Error should mention path not found"
    );
}

#[test]
fn test_invalid_method_name_in_hook() {
    let registry = create_bb_registry();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeth = o.onDeth;  // typo: "onDeth" instead of "onDeath"
        });
    "#;

    let diagnostics = analyze_hooks(code, &registry).expect("Hook analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Some(DiagnosticSeverity::ERROR) && d.message.contains("onDeth")
        })
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "Should have one error for invalid method name"
    );
    assert!(
        errors[0].message.contains("not found"),
        "Error should mention method not found"
    );
}

#[test]
fn test_hook_type_suggestion_base_class() {
    let registry = create_bb_registry();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            // Using hookExactClass on actor which has children (human, etc.)
        });
    "#;

    let diagnostics = analyze_hooks(code, &registry).expect("Hook analysis should succeed");

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        .collect();

    assert!(
        !warnings.is_empty(),
        "Should have warning about hook type"
    );
    assert!(
        warnings[0].message.contains("hookBaseClass"),
        "Should suggest hookBaseClass"
    );
}

#[test]
fn test_valid_inheritance() {
    let registry = create_bb_registry();

    let code = r#"
        knight <- inherit("scripts/entity/tactical/human", {
            m = {},
            function create() {
                human.create();
            }
        });
    "#;

    let diagnostics = analyze_inheritance(code, &registry).expect("Inheritance analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert_eq!(errors.len(), 0, "Should have no errors for valid inheritance");
}

#[test]
fn test_invalid_parent_path() {
    let registry = create_bb_registry();

    let code = r#"
        knight <- inherit("scripts/entity/tactical/humam", {
            // typo: "humam" instead of "human"
        });
    "#;

    let diagnostics = analyze_inheritance(code, &registry).expect("Inheritance analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "Should have one error for invalid parent path"
    );
    assert!(
        errors[0].message.contains("not found"),
        "Error should mention parent not found"
    );
}

#[test]
fn test_method_inheritance() {
    let registry = create_bb_registry();

    // barbarian_thrall should have access to onDeath from actor
    assert!(
        registry.has_method("barbarian_thrall", "onDeath"),
        "barbarian_thrall should inherit onDeath from actor"
    );

    assert!(
        registry.has_method("human", "onDeath"),
        "human should inherit onDeath from actor"
    );

    assert!(
        !registry.has_method("actor", "nonExistentMethod"),
        "actor should not have nonExistentMethod"
    );
}

#[test]
fn test_inheritance_chain() {
    let registry = create_bb_registry();

    // Test ancestor chain
    let ancestors = registry.get_ancestors("barbarian_thrall");
    assert_eq!(ancestors.len(), 2, "barbarian_thrall should have 2 ancestors");

    let ancestor_names: Vec<_> = ancestors.iter().map(|a| a.name.as_str()).collect();
    assert!(
        ancestor_names.contains(&"human"),
        "Ancestors should include human"
    );
    assert!(
        ancestor_names.contains(&"actor"),
        "Ancestors should include actor"
    );

    // Test descendant chain
    let descendants = registry.get_descendants("actor");
    assert!(
        descendants.len() >= 2,
        "actor should have at least 2 descendants"
    );

    let descendant_names: Vec<_> = descendants.iter().map(|d| d.name.as_str()).collect();
    assert!(
        descendant_names.contains(&"human"),
        "Descendants should include human"
    );
    assert!(
        descendant_names.contains(&"barbarian_thrall"),
        "Descendants should include barbarian_thrall"
    );
}

#[test]
fn test_is_descendant_of() {
    let registry = create_bb_registry();

    assert!(
        registry.is_descendant_of("human", "actor"),
        "human is a descendant of actor"
    );

    assert!(
        registry.is_descendant_of("barbarian_thrall", "actor"),
        "barbarian_thrall is a descendant of actor"
    );

    assert!(
        registry.is_descendant_of("barbarian_thrall", "human"),
        "barbarian_thrall is a descendant of human"
    );

    assert!(
        !registry.is_descendant_of("actor", "human"),
        "actor is NOT a descendant of human"
    );
}

#[test]
fn test_multiple_hooks_different_types() {
    let registry = create_bb_registry();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeath = o.onDeath;
        });

        ::mods_hookBaseClass("entity/tactical/actor", function(o) {
            while(!("m" in o)) o=o[o.SuperName];
        });

        ::mods_hookDescendants("entity/tactical/humans/barbarian_thrall", function(o) {
            // This will give a warning since barbarian_thrall has no descendants
        });
    "#;

    let diagnostics = analyze_hooks(code, &registry).expect("Hook analysis should succeed");

    // Should have multiple warnings but no errors
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        .collect();

    // Debug: Print all diagnostics
    for (i, d) in diagnostics.iter().enumerate() {
        println!("Diagnostic {}: {:?} - {}", i, d.severity, d.message);
    }

    assert_eq!(errors.len(), 0, "Should have no errors");
    assert!(warnings.len() >= 2, "Should have multiple warnings");
}
