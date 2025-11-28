use squirrel_lsp::bb_support::{analyze_hooks, analyze_inheritance};
use squirrel_lsp::workspace::Workspace;
use std::path::Path;
use tower_lsp::lsp_types::DiagnosticSeverity;

/// Create a test workspace with a class hierarchy
fn create_test_workspace() -> Workspace {
    let mut workspace = Workspace::new();

    // Index actor (base class)
    let actor_code = r#"
        this.actor <- this.inherit("scripts/entity/base", {
            function onDeath() {}
            function onDamageReceived() {}
            function setFatigue(_f) {}
        });
    "#;
    workspace
        .index_file(
            Path::new("/test/scripts/entity/tactical/actor.nut"),
            actor_code,
        )
        .unwrap();

    // Index human (inherits from actor)
    let human_code = r#"
        this.human <- this.inherit("scripts/entity/tactical/actor", {
            function create() {}
        });
    "#;
    workspace
        .index_file(
            Path::new("/test/scripts/entity/tactical/human.nut"),
            human_code,
        )
        .unwrap();

    // Index barbarian_thrall (inherits from human)
    let thrall_code = r#"
        this.barbarian_thrall <- this.inherit("scripts/entity/tactical/human", {
            function create() {}
        });
    "#;
    workspace
        .index_file(
            Path::new("/test/scripts/entity/tactical/humans/barbarian_thrall.nut"),
            thrall_code,
        )
        .unwrap();

    // Build inheritance graph
    workspace.build_inheritance_graph();

    workspace
}

#[test]
fn test_valid_hook_exact_class() {
    let workspace = create_test_workspace();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeath = o.onDeath;
            o.onDeath = function(_killer, _skill, _tile, _fatalityType) {
                onDeath(_killer, _skill, _tile, _fatalityType);
                // Custom logic
            };
        });
    "#;

    let diagnostics = analyze_hooks(code, &workspace).expect("Hook analysis should succeed");

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
    let workspace = create_test_workspace();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/aktor", function(o) {
            // typo: "aktor" instead of "actor"
        });
    "#;

    let diagnostics = analyze_hooks(code, &workspace).expect("Hook analysis should succeed");

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
    let workspace = create_test_workspace();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeth = o.onDeth;  // typo: "onDeth" instead of "onDeath"
        });
    "#;

    let diagnostics = analyze_hooks(code, &workspace).expect("Hook analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR) && d.message.contains("onDeth"))
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
    let workspace = create_test_workspace();

    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            // Using hookExactClass on actor which has children (human, etc.)
        });
    "#;

    let diagnostics = analyze_hooks(code, &workspace).expect("Hook analysis should succeed");

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        .collect();

    assert!(!warnings.is_empty(), "Should have warning about hook type");
    assert!(
        warnings[0].message.contains("hookBaseClass"),
        "Should suggest hookBaseClass"
    );
}

#[test]
fn test_method_inheritance() {
    let workspace = create_test_workspace();

    // barbarian_thrall should have access to onDeath from actor
    assert!(
        workspace.has_member("entity/tactical/humans/barbarian_thrall", "onDeath"),
        "barbarian_thrall should inherit onDeath from actor"
    );

    assert!(
        workspace.has_member("entity/tactical/human", "onDeath"),
        "human should inherit onDeath from actor"
    );

    assert!(
        !workspace.has_member("entity/tactical/actor", "nonExistentMethod"),
        "actor should not have nonExistentMethod"
    );
}

#[test]
fn test_inheritance_chain() {
    let workspace = create_test_workspace();

    // Test ancestor chain
    let ancestors = workspace.get_ancestors("entity/tactical/humans/barbarian_thrall");
    assert_eq!(
        ancestors.len(),
        2,
        "barbarian_thrall should have 2 ancestors"
    );

    let ancestor_names: Vec<_> = ancestors.iter().map(|a| a.name.as_str()).collect();
    assert!(
        ancestor_names.contains(&"human"),
        "Ancestors should include human"
    );
    assert!(
        ancestor_names.contains(&"actor"),
        "Ancestors should include actor"
    );
}

#[test]
fn test_is_descendant_of() {
    let workspace = create_test_workspace();

    // Check via ancestors
    let human_ancestors = workspace.get_ancestors("entity/tactical/human");
    assert!(
        human_ancestors.iter().any(|a| a.name == "actor"),
        "human is a descendant of actor"
    );

    let thrall_ancestors = workspace.get_ancestors("entity/tactical/humans/barbarian_thrall");
    assert!(
        thrall_ancestors.iter().any(|a| a.name == "actor"),
        "barbarian_thrall is a descendant of actor"
    );

    assert!(
        thrall_ancestors.iter().any(|a| a.name == "human"),
        "barbarian_thrall is a descendant of human"
    );

    // actor should not have ancestors that include human
    let actor_ancestors = workspace.get_ancestors("entity/tactical/actor");
    assert!(
        !actor_ancestors.iter().any(|a| a.name == "human"),
        "actor is NOT a descendant of human"
    );
}

#[test]
fn test_multiple_hooks_different_types() {
    let workspace = create_test_workspace();

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

    let diagnostics = analyze_hooks(code, &workspace).expect("Hook analysis should succeed");

    // Should have multiple warnings but no errors
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        .collect();

    assert_eq!(errors.len(), 0, "Should have no errors");
    assert!(warnings.len() >= 2, "Should have multiple warnings");
}

/// Test case for semantic/029_function_declared_in_parent.nut
/// This verifies that inheriting from a known parent class doesn't produce "not found" errors
#[test]
fn test_inherit_from_skill_class() {
    let mut workspace = Workspace::new();

    // First, index the parent skill class
    let skill_code = r#"
        skill <- {
            m = {
                Container = null
            },

            function getContainer() {
                return m.Container;
            }
        };
    "#;
    workspace
        .index_file(Path::new("/test/scripts/skills/skill.nut"), skill_code)
        .unwrap();

    workspace.build_inheritance_graph();

    // Now test the child class that inherits from skill
    let child_code = r#"
        this.perk_legend_ambidextrous <- this.inherit("scripts/skills/skill", {
            function onAdded() {
                local off = getContainer().getActor().getOffhandItem();
            }
        });
    "#;

    let diagnostics = analyze_inheritance(child_code, &workspace).expect("Analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert!(
        errors.is_empty(),
        "Should have no errors when parent exists. Got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Test that missing parent produces an error
#[test]
fn test_missing_parent_class() {
    let workspace = Workspace::new(); // Empty workspace

    let code = r#"
        this.perk <- this.inherit("scripts/skills/skill", {
            function onAdded() {}
        });
    "#;

    let diagnostics = analyze_inheritance(code, &workspace).expect("Analysis should succeed");

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();

    assert!(
        !errors.is_empty(),
        "Should have error when parent doesn't exist"
    );
    assert!(
        errors[0].message.contains("not found"),
        "Error should mention parent not found"
    );
}

#[test]
fn test_inherit_hook() {
    let mut workspace = Workspace::new();
    let vanilla = r#"
        this.skill <- {
        };
    "#;
    workspace
        .index_file(Path::new("scripts/skills/skill.nut"), vanilla)
        .unwrap();
    let legends = r#"
        this.legend_parrying_effect <- this.inherit("scripts/skills/skill", {
            m = {
            },
        });
    "#;
    workspace
        .index_file(
            Path::new("scripts/skills/effects/legend_parrying_effect.nut"),
            legends,
        )
        .unwrap();
    let fotn = r#"
        ::mods_hookExactClass("skills/effects/legend_parrying_effect", function ( o )
        {
            o.m.ParrySounds <- [
                "sounds/combat/legend_parried_01.wav",
            ];
        });
    "#;
    // workspace
    //     .index_file(
    //         Path::new("hooks/skills/effects/legend_parrying_effect.nut"),
    //         fotn,
    //     )
    //     .unwrap();
    let diagnostics = analyze_inheritance(fotn, &workspace).unwrap();
    eprintln!("Diagnostics: {:?}", diagnostics);
    assert!(diagnostics.is_empty());
}
