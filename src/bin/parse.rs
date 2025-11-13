use std::env;
use std::fs;
use tree_sitter::Node;

fn print_errors(node: Node, source: &str, depth: usize) {
    let indent = "  ".repeat(depth);

    if node.is_error() || node.is_missing() {
        println!(
            "{}Error at {}:{} - {} (missing: {})",
            indent,
            node.start_position().row + 1,
            node.start_position().column + 1,
            node.kind(),
            node.is_missing()
        );

        let text = node
            .utf8_text(source.as_bytes())
            .unwrap_or("<invalid utf8>");
        println!("{}  Text: {:?}", indent, text);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_errors(child, source, depth + 1);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.nut>", args[0]);
        eprintln!("\nParses a Squirrel file and reports any syntax errors.");
        std::process::exit(1);
    }

    let source = fs::read_to_string(&args[1]).unwrap_or_else(|e| {
        eprintln!("Error reading file '{}': {}", args[1], e);
        std::process::exit(1);
    });

    let tree = squirrel_lsp::helpers::parse_squirrel(&source).unwrap_or_else(|e| {
        eprintln!("Parse failed: {}", e);
        std::process::exit(1);
    });

    let root = tree.root_node();

    if root.has_error() {
        println!("Parse errors found:\n");
        print_errors(root, &source, 0);
        std::process::exit(1);
    } else {
        println!("No parse errors");
        std::process::exit(0);
    }
}
