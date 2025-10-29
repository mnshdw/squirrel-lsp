use std::fs;
use std::path::{Path, PathBuf};

use pretty_assertions::assert_eq;
use squirrel_lsp::formatter::{FormatOptions, IndentStyle, format_document};

#[test]
fn test_formatter() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("formatter");
    let input_dir = base.join("input");
    let expected_dir = base.join("expected");

    let mut files: Vec<PathBuf> = fs::read_dir(&input_dir)
        .expect("failed to read tests/input directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "nut"))
        .collect();
    files.sort();

    for input_path in files {
        let input: String = fs::read_to_string(&input_path)
            .unwrap_or_else(|_| panic!("failed to read input: {:?}", input_path));

        let file_name = input_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let expected_path = expected_dir.join(&file_name);
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("failed to read expected: {:?}", expected_path));

        let options = if input.lines().any(|l| l.starts_with('\t')) {
            FormatOptions::with_indent(IndentStyle::Tabs)
        } else {
            FormatOptions::default()
        };

        let output = format_document(&input, &options)
            .unwrap_or_else(|e| panic!("formatting failed for {}: {}", file_name, e));
        assert_eq!(output, expected, "mismatch for case: {}", file_name);
    }
}
