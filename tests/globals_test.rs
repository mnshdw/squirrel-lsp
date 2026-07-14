use std::path::{Path, PathBuf};

use squirrel_lsp::symbol_resolver::{
    collect_unresolved_identifiers, compute_symbol_diagnostics_with_globals,
};
use squirrel_lsp::workspace::Workspace;

fn index(root: &str, files: &[(&str, &str)]) -> Workspace {
    let mut workspace = Workspace::with_folders(vec![PathBuf::from(root)]);

    for (path, source) in files {
        workspace.index_file(Path::new(path), source).unwrap();
    }
    workspace.build_inheritance_graph();

    for (path, source) in files {
        let unresolved = collect_unresolved_identifiers(path, source, workspace.globals()).unwrap();
        workspace.set_unresolved(Path::new(path), &unresolved);
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
