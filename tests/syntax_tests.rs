use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Parser;

#[test]
fn test_syntax_errors() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let input_dir = base.join("syntax");

    let mut files: Vec<PathBuf> = fs::read_dir(&input_dir)
        .expect("failed to read tests/syntax directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "nut"))
        .collect();
    files.sort();

    for input_path in files {
        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|_| panic!("failed to read input: {:?}", input_path));

        let file_name = input_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        // Parse expected error count from first line comment
        let expected = parse_expected_errors(&input);

        // Parse the code
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_squirrel::language())
            .expect("Failed to set language");

        let tree = parser.parse(&input, None);

        let error_count = if let Some(tree) = tree {
            count_errors(tree.root_node())
        } else {
            // If parsing completely fails, that's also an error
            1
        };

        match expected {
            ExpectedErrors::Exact(n) => {
                assert_eq!(
                    error_count, n,
                    "Mismatch for {}: expected exactly {} errors, got {}",
                    file_name, n, error_count
                );
            },
            ExpectedErrors::AtLeastOne => {
                assert!(
                    error_count >= 1,
                    "Mismatch for {}: expected at least 1 error, got {}",
                    file_name,
                    error_count
                );
            },
        }
    }
}

enum ExpectedErrors {
    Exact(usize),
    AtLeastOne,
}

/// Parse expected errors from first line comment
/// Format: // EXPECT: 0 errors
/// Or: // EXPECT: 1+ errors
/// Or: // EXPECT: 2 errors
fn parse_expected_errors(input: &str) -> ExpectedErrors {
    let first_line = input.lines().next().unwrap_or("");

    if let Some(expect_part) = first_line.strip_prefix("// EXPECT:") {
        let expect_part = expect_part.trim();

        if expect_part == "0 errors" {
            ExpectedErrors::Exact(0)
        } else if expect_part.ends_with("+ errors") {
            ExpectedErrors::AtLeastOne
        } else if let Some(num_str) = expect_part.strip_suffix(" errors") {
            if let Ok(n) = num_str.trim().parse::<usize>() {
                ExpectedErrors::Exact(n)
            } else {
                panic!("Invalid error count format: {}", expect_part);
            }
        } else {
            panic!("Invalid EXPECT format: {}", expect_part);
        }
    } else {
        panic!("Test file must start with // EXPECT: comment");
    }
}

/// Count ERROR nodes and missing nodes in the tree
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = 0;

    if node.is_error() || node.is_missing() || node.kind() == "ERROR" {
        count += 1;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_errors(child);
    }

    count
}
