use std::fs;
use std::path::{Path, PathBuf};

use squirrel_lsp::semantic_analyzer::compute_semantic_diagnostics;

#[test]
fn test_semantic_analyzer() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let input_dir = base.join("semantic");

    let mut files: Vec<PathBuf> = fs::read_dir(&input_dir)
        .expect("failed to read tests/semantic directory")
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

        // Parse expected errors from first line comment
        let expected_errors = parse_expected_errors(&input);

        let diagnostics = compute_semantic_diagnostics(&input)
            .unwrap_or_else(|e| panic!("semantic analysis failed for {}: {}", file_name, e));

        // Extract undeclared variable names from diagnostics
        let actual_errors: Vec<String> = diagnostics
            .iter()
            .filter_map(|d| {
                if d.message.starts_with("Undeclared variable '") {
                    // Extract variable name from "Undeclared variable 'xyz'"
                    d.message
                        .strip_prefix("Undeclared variable '")
                        .and_then(|s| s.strip_suffix("'"))
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();

        // Sort for comparison
        let mut sorted_actual = actual_errors.clone();
        let mut sorted_expected = expected_errors.clone();
        sorted_actual.sort();
        sorted_expected.sort();

        assert_eq!(
            sorted_actual, sorted_expected,
            "Mismatch for {}: expected errors {:?}, got {:?}",
            file_name, expected_errors, actual_errors
        );
    }
}

/// Parse expected errors from first line comment
/// Format: // EXPECT: var1, var2, var3
/// Or: // EXPECT: no errors
fn parse_expected_errors(input: &str) -> Vec<String> {
    let first_line = input.lines().next().unwrap_or("");

    if let Some(expect_part) = first_line.strip_prefix("// EXPECT:") {
        let expect_part = expect_part.trim();
        if expect_part == "no errors" {
            Vec::new()
        } else {
            expect_part
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    } else {
        panic!("Test file must start with // EXPECT: comment");
    }
}
