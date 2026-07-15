use std::path::{Path, PathBuf};

use squirrel_lsp::symbol_resolver::{compute_symbol_diagnostics_with_globals, scan_file_bindings};
use squirrel_lsp::workspace::Workspace;

fn index(root: &str, files: &[(&str, &str)]) -> Workspace {
    let mut workspace = Workspace::with_folders(vec![PathBuf::from(root)]);

    for (path, source) in files {
        workspace.index_file(Path::new(path), source).unwrap();
    }
    workspace.build_inheritance_graph();

    for (path, source) in files {
        let bindings = scan_file_bindings(path, source, workspace.globals()).unwrap();
        workspace.set_unresolved(Path::new(path), &bindings.unresolved);
        workspace.set_declared_locals(Path::new(path), &bindings.declared_locals);
    }
    workspace.infer_host_globals();

    workspace
}

fn undeclared(workspace: &Workspace, path: &str, source: &str) -> Vec<String> {
    compute_symbol_diagnostics_with_globals(path, source, workspace.known_globals())
        .unwrap()
        .iter()
        .filter(|d| d.message.starts_with("Undeclared variable"))
        .map(|d| d.message.clone())
        .collect()
}

/// A project with no `scripts/` directory was never indexed at all, so every identifier was
/// reported as undeclared.
#[test]
fn test_cross_file_globals_resolve_without_any_directory_convention() {
    let title = r#"
        function drawTitle() {
            local f = Font();
            f.render("hello");
            return DEFAULT_FONT;
        }
    "#;

    let workspace = index(
        "/home/me/game",
        &[
            (
                "/home/me/game/ui/font.nut",
                r#"
                    class Font {
                        function render(_text) {}
                    }
                    DEFAULT_FONT <- Font();
                "#,
            ),
            ("/home/me/game/ui/title.nut", title),
        ],
    );

    assert!(
        undeclared(&workspace, "/home/me/game/ui/title.nut", title).is_empty(),
        "got: {:?}",
        undeclared(&workspace, "/home/me/game/ui/title.nut", title)
    );
}

/// `require` and `Font` are bound by the env and appear in no `.nut` file.
#[test]
fn test_host_bindings_are_inferred_from_repeated_use() {
    let main = r#"
        require("ui/font.nut");
        local f = Font();
    "#;
    let other = r#"
        require("ui/sprite.nut");
        local g = Font();
    "#;

    let workspace = index(
        "/home/me/game",
        &[
            ("/home/me/game/main.nut", main),
            ("/home/me/game/other.nut", other),
        ],
    );

    assert!(
        undeclared(&workspace, "/home/me/game/main.nut", main).is_empty(),
        "'require' and 'Font' are used throughout the project and defined nowhere; got: {:?}",
        undeclared(&workspace, "/home/me/game/main.nut", main)
    );
}

/// A name used exactly once and defined nowhere is still reported, so typos do not become invisible.
#[test]
fn test_a_single_use_of_an_unknown_name_is_still_reported() {
    let main = "local x = someTypoOnlyWrittenOnce;";

    let workspace = index("/home/me/game", &[("/home/me/game/main.nut", main)]);
    let found = undeclared(&workspace, "/home/me/game/main.nut", main);

    assert_eq!(found.len(), 1, "got: {found:?}");
    assert!(found[0].contains("someTypoOnlyWrittenOnce"));
}

#[test]
fn test_a_leaked_local_is_not_mistaken_for_a_host_global() {
    let declares_a = r#"
        function pickA() {
            local targets = [1, 2, 3];
            return targets[0];
        }
    "#;
    let declares_b = r#"
        function pickB() {
            local targets = getEnemies();
            return targets.len();
        }
    "#;
    let leak = r#"
        function pickC() {
            return targets[0];
        }
    "#;

    let workspace = index(
        "/ws",
        &[
            ("/ws/a.nut", declares_a),
            ("/ws/b.nut", declares_b),
            ("/ws/c.nut", leak),
        ],
    );

    assert!(
        !workspace.known_globals().contains("targets"),
        "'targets' is a local of the codebase, so it must not be inferred as a host global"
    );
    assert_eq!(
        undeclared(&workspace, "/ws/c.nut", leak),
        vec!["Undeclared variable 'targets'".to_string()],
        "the leaked use must still be reported"
    );
}

#[test]
fn test_a_pervasive_bare_name_stays_a_host_global() {
    let uses: Vec<(String, String)> = (0..8)
        .map(|i| {
            (
                format!("/ws/u{i}.nut"),
                "class C { function f() { return m.value; } }".to_string(),
            )
        })
        .collect();
    let mut files: Vec<(&str, &str)> = uses.iter().map(|(p, s)| (p.as_str(), s.as_str())).collect();
    files.push(("/ws/odd.nut", "function g() { local m = 1; return m; }"));

    let workspace = index("/ws", &files);

    assert!(
        workspace.known_globals().contains("m"),
        "'m' is referenced bare far more than it is declared, so it stays an inferred binding"
    );
}

/// Battle Brothers refers to one file by two different partial paths, with and without the `scripts/`
/// prefix, and multiple mods provide the same script.
#[test]
fn test_battle_brothers_partial_paths_and_mod_overlay() {
    let workspace = index(
        "/ws",
        &[
            (
                "/ws/vanilla/scripts/entity/tactical/actor.nut",
                r#"this.actor <- this.inherit("scripts/entity/base", {
                    function onDeath() {}
                });"#,
            ),
            (
                "/ws/legends/scripts/entity/tactical/actor.nut",
                r#"this.actor <- this.inherit("scripts/entity/base", {
                    function onLegendsDeath() {}
                });"#,
            ),
        ],
    );

    // The form an inherit() uses
    assert!(
        workspace.contains("scripts/entity/tactical/actor"),
        "inherit() path must resolve"
    );
    // The form a hook uses, for the same file.
    assert!(
        workspace.contains("entity/tactical/actor"),
        "hook path must resolve"
    );

    // Both mods copies are found, and their members are merged: at runtime they are
    // one class, so a method added by either exists.
    assert_eq!(workspace.resolve_all("entity/tactical/actor").len(), 2);
    assert!(workspace.has_member("entity/tactical/actor", "onDeath"));
    assert!(workspace.has_member("entity/tactical/actor", "onLegendsDeath"));

    assert!(workspace.globals().contains("actor"));
}

/// The environment binds its own names into the root table, and in a class-based codebase
/// most of them are used from inside class bodies. Those references are not reported (a
/// name in a derived class may be an inherited slot), but they still have to count towards
/// the inference: a native used once outside a class and once inside one would otherwise
/// stay below the threshold and be reported at the reference outside.
#[test]
fn test_native_used_inside_a_class_counts_towards_inference() {
    let config = r#"
        function setup() {
            local img = Image("res://images/player.png");
            return img.width;
        }
    "#;
    let tent = r#"
        class Tent extends Entity {
            constructor() {
                down1 = Image("res://images/objects/tent.png");
            }
        }
    "#;

    let workspace = index("/ws", &[("/ws/config.nut", config), ("/ws/tent.nut", tent)]);

    assert!(
        workspace.known_globals().contains("Image"),
        "Image is used twice, so it should be inferred as a name from the environment"
    );
    assert!(
        undeclared(&workspace, "/ws/config.nut", config).is_empty(),
        "got: {:?}",
        undeclared(&workspace, "/ws/config.nut", config)
    );
}

/// A name used only once is still assumed to be a typo, including inside a class.
#[test]
fn test_single_use_outside_a_class_is_still_reported() {
    let config = r#"
        function setup() {
            return Imag("res://images/player.png");
        }
    "#;

    let workspace = index("/ws", &[("/ws/config.nut", config)]);

    assert_eq!(
        undeclared(&workspace, "/ws/config.nut", config),
        vec!["Undeclared variable 'Imag'".to_string()]
    );
}
