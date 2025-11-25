use squirrel_lsp::helpers::parse_squirrel;

#[test]
fn debug_inherit_ast() {
    let code = r#"
        barbarian_thrall <- inherit("scripts/entity/tactical/human", {
            m = {},
            function create() {
                human.create();
            }
        });
    "#;

    let tree = parse_squirrel(code).unwrap();
    let root = tree.root_node();

    println!("\n=== INHERIT CALL AST ===");
    print_node(root, code, 0);
}

#[test]
fn debug_hook_ast() {
    let code = r#"
        ::mods_hookExactClass("entity/tactical/actor", function(o) {
            local onDeath = o.onDeath;
        });
    "#;

    let tree = parse_squirrel(code).unwrap();
    let root = tree.root_node();

    println!("\n=== HOOK CALL AST ===");
    print_node(root, code, 0);
}

fn print_node(node: tree_sitter::Node, text: &str, depth: usize) {
    let indent = "  ".repeat(depth);
    let node_text = node.utf8_text(text.as_bytes()).unwrap_or("");
    let preview = if node_text.len() > 50 {
        format!("{}...", &node_text[..50])
    } else {
        node_text.to_string()
    };

    println!(
        "{}{} [{}..{}] \"{}\"",
        indent,
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        preview.replace('\n', "\\n")
    );

    // Print named children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_node(child, text, depth + 1);
    }
}
