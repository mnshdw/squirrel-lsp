use std::env;
use std::fs;
use tree_sitter::Node;

fn print_tree(node: Node, source: &str, depth: usize, show_text: bool) {
    let indent = "  ".repeat(depth);
    let kind = node.kind();

    let text = node
        .utf8_text(source.as_bytes())
        .unwrap_or("<invalid utf8>");

    let display_text = if show_text {
        if text.len() > 60 {
            format!(" | {}...", &text[..60].replace('\n', "\\n"))
        } else {
            format!(" | {}", text.replace('\n', "\\n"))
        }
    } else {
        String::new()
    };

    let position = format!(
        "{}:{}",
        node.start_position().row + 1,
        node.start_position().column + 1
    );

    println!("{}{:20} @ {}{}", indent, kind, position, display_text);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        print_tree(child, source, depth + 1, show_text);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut show_text = false;
    let mut file_path = None;

    for arg in &args[1..] {
        match arg.as_str() {
            "--text" | "-t" => show_text = true,
            "--help" | "-h" => {
                print_usage(&args[0]);
                std::process::exit(0);
            },
            other => {
                if file_path.is_some() {
                    eprintln!("Error: unexpected argument '{}'", other);
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
                file_path = Some(other);
            },
        }
    }

    let file_path = match file_path {
        Some(path) => path,
        None => {
            eprintln!("Error: missing file argument");
            print_usage(&args[0]);
            std::process::exit(1);
        },
    };

    let source = fs::read_to_string(file_path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{}': {}", file_path, e);
        std::process::exit(1);
    });

    let tree = squirrel_lsp::helpers::parse_squirrel(&source).unwrap_or_else(|e| {
        eprintln!("Parse failed: {}", e);
        std::process::exit(1);
    });

    let root = tree.root_node();

    println!("AST for '{}':\n", file_path);
    print_tree(root, &source, 0, show_text);
}

fn print_usage(program: &str) {
    eprintln!("Usage: {} [OPTIONS] <file.nut>", program);
    eprintln!();
    eprintln!("Prints the AST (Abstract Syntax Tree) of a Squirrel file.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -t, --text    Show node text content (truncated to 60 chars)");
    eprintln!("  -h, --help    Show this help message");
    eprintln!();
    eprintln!("Example:");
    eprintln!("  {} test.nut", program);
    eprintln!("  {} --text test.nut", program);
}
