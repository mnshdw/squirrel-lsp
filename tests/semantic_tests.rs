use std::fs;
use std::path::{Path, PathBuf};

use squirrel_lsp::symbol_resolver::compute_symbol_diagnostics;

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

        let file_path = input_path.to_string_lossy().to_string();

        // Parse expected errors from first line comment
        let expected_errors = parse_expected_errors(&input);

        let diagnostics = compute_symbol_diagnostics(&file_path, &input)
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

/// Test that nested table slots accessed via this.m.x are not flagged as unused
#[test]
fn test_nested_table_slot_not_unused() {
    let code = r#"
this.skill <- {
    m = {
        offHandSkill = null,
        HandToHand = null
    },

    function setOffhandSkill(_a) {
        this.m.offHandSkill = _a;
    },

    function getHandToHand() {
        return this.m.HandToHand;
    }
}
"#;

    let diagnostics =
        compute_symbol_diagnostics("test.nut", code).expect("analysis should succeed");

    // Check for unused variable warnings about offHandSkill or HandToHand
    let unused_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused"))
        .collect();

    // Print all diagnostics for debugging
    for d in &diagnostics {
        eprintln!("Diagnostic: {} at {:?}", d.message, d.range);
    }

    assert!(
        unused_warnings.is_empty(),
        "Should not have unused warnings for accessed nested table slots, got: {:?}",
        unused_warnings
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_variable_used_in_table_literal_value() {
    let code = r#"
        function testFunc(_targetTile) {
            local skillToUse = 5;
            ::Time.scheduleEvent(1, 2, 3, {
                TargetTile = _targetTile,
                Skill = skillToUse
            });
        }
    "#;
    let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
    assert!(!diagnostics.iter().any(|d| d.message.contains("skillToUse")));
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
