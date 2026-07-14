use std::path::{Path, PathBuf};

use squirrel_lsp::navigation::find_definitions;
use squirrel_lsp::workspace::Workspace;
use tower_lsp::lsp_types::Position;

const ACTOR: &str = r#"this.actor <- this.inherit("scripts/entity/entity", {
	m = {},
	function onDeath(_killer) {}
	function checkMorale() {}
});"#;

const AURA_ABSTRACT: &str = r#"this.aura_abstract <- ::inherit("scripts/skills/skill", {
	m = {},
	function create() {}
	function onUpdate() {}
});"#;

const AMOK_AURA: &str = r#"this.amok_aura <- ::inherit("scripts/skills/aura/aura_abstract", {
	m = {},
	function create()
	{
		aura_abstract.create();
	}
});"#;

const ACTOR_HOOK: &str = r#"::mods_hookExactClass("entity/tactical/actor", function(o) {
	local checkMorale = o.checkMorale;
	o.checkMorale = function() {
		checkMorale();
		this.onDeath(null);
	}
});"#;

fn workspace() -> Workspace {
    let mut ws = Workspace::with_folders(vec![PathBuf::from("/ws")]);

    for (path, src) in [
        ("/ws/base/scripts/entity/tactical/actor.nut", ACTOR),
        (
            "/ws/mod/scripts/skills/aura/aura_abstract.nut",
            AURA_ABSTRACT,
        ),
        ("/ws/mod/scripts/skills/aura/amok_aura.nut", AMOK_AURA),
        ("/ws/mod/hooks/entity/tactical/actor.nut", ACTOR_HOOK),
    ] {
        ws.index_file(Path::new(path), src).unwrap();
    }
    ws.build_inheritance_graph();
    ws
}

/// Cursor just inside `needle`, on the line holding `anchor`.
fn at(text: &str, anchor: &str, needle: &str) -> Position {
    let (line, col) = text
        .lines()
        .enumerate()
        .find_map(|(i, l)| {
            if l.contains(anchor) {
                l.find(needle).map(|c| (i, c))
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("{anchor:?} / {needle:?} not found"));

    Position::new(line as u32, col as u32 + 1)
}

fn goto(ws: &Workspace, file: &str, text: &str, anchor: &str, needle: &str) -> Vec<String> {
    find_definitions(text, at(text, anchor, needle), Path::new(file), ws)
        .iter()
        .map(|d| format!("{}:{}", d.file_path.display(), d.line))
        .collect()
}

#[test]
fn test_class_declared_with_global_inherit_is_indexed() {
    let ws = workspace();

    assert!(
        ws.contains("skills/aura/aura_abstract"),
        "a class declared with ::inherit() must be indexed"
    );
    assert_eq!(ws.find_by_name("aura_abstract").len(), 1);
    assert!(ws.has_member("skills/aura/aura_abstract", "create"));
}

#[test]
fn test_goto_inherit_path_string() {
    let ws = workspace();
    let file = "/ws/mod/scripts/skills/aura/amok_aura.nut";

    let hits = goto(&ws, file, AMOK_AURA, "::inherit(", "scripts/skills/aura");
    assert_eq!(
        hits,
        vec!["/ws/mod/scripts/skills/aura/aura_abstract.nut:0"]
    );
}

#[test]
fn test_goto_parent_class_by_name() {
    let ws = workspace();
    let file = "/ws/mod/scripts/skills/aura/amok_aura.nut";

    let hits = goto(
        &ws,
        file,
        AMOK_AURA,
        "aura_abstract.create()",
        "aura_abstract",
    );
    assert_eq!(
        hits,
        vec!["/ws/mod/scripts/skills/aura/aura_abstract.nut:0"]
    );
}

#[test]
fn test_goto_method_on_named_class() {
    let ws = workspace();
    let file = "/ws/mod/scripts/skills/aura/amok_aura.nut";

    let hits = goto(&ws, file, AMOK_AURA, "aura_abstract.create()", "create()");
    assert_eq!(
        hits,
        vec!["/ws/mod/scripts/skills/aura/aura_abstract.nut:2"]
    );
}

#[test]
fn test_goto_hook_target_path_string() {
    let ws = workspace();
    let file = "/ws/mod/hooks/entity/tactical/actor.nut";

    let hits = goto(
        &ws,
        file,
        ACTOR_HOOK,
        "::mods_hookExactClass(",
        "entity/tactical/actor",
    );
    assert_eq!(hits, vec!["/ws/base/scripts/entity/tactical/actor.nut:0"]);
}

#[test]
fn test_goto_hook_param_member_resolves_to_hooked_class() {
    let ws = workspace();
    let file = "/ws/mod/hooks/entity/tactical/actor.nut";

    let hits = goto(
        &ws,
        file,
        ACTOR_HOOK,
        "local checkMorale = o.checkMorale",
        "checkMorale;",
    );
    assert_eq!(hits, vec!["/ws/base/scripts/entity/tactical/actor.nut:3"]);
}

#[test]
fn test_goto_this_member_inside_hook_resolves_to_hooked_class() {
    let ws = workspace();
    let file = "/ws/mod/hooks/entity/tactical/actor.nut";

    let hits = goto(&ws, file, ACTOR_HOOK, "this.onDeath(null)", "onDeath");
    assert_eq!(hits, vec!["/ws/base/scripts/entity/tactical/actor.nut:2"]);
}

#[test]
fn test_known_class_without_the_method_yields_nothing() {
    let ws = workspace();
    let file = "/ws/mod/hooks/entity/tactical/actor.nut";

    let text = ACTOR_HOOK.replace("o.checkMorale;", "o.noSuchMethod;");
    let hits = goto(
        &ws,
        file,
        &text,
        "local checkMorale = o.noSuchMethod",
        "noSuchMethod;",
    );

    assert!(hits.is_empty(), "got: {hits:?}");
}

#[test]
fn test_ambiguous_fallback_is_deterministic_and_complete() {
    let ws = workspace();
    let file = "/ws/mod/scripts/skills/aura/amok_aura.nut";

    let text = "function f() { local thing = getThing(); thing.create(); }";

    let first = goto(&ws, file, text, "thing.create()", "create()");
    for _ in 0..5 {
        assert_eq!(goto(&ws, file, text, "thing.create()", "create()"), first);
    }

    assert_eq!(first.len(), 2, "got: {first:?}");
}
