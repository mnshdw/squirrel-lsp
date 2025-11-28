use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

use squirrel_lsp::symbol_resolver::compute_symbol_diagnostics_with_globals;
use squirrel_lsp::syntax_analyzer::compute_syntax_diagnostics;
use tower_lsp::lsp_types::DiagnosticSeverity;

fn check_file(path: &Path, globals: &HashSet<String>) -> (usize, usize) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path.display(), e);
            return (0, 0);
        },
    };

    let file_path = path.to_string_lossy();
    let mut errors = 0;
    let mut warnings = 0;

    // Syntax diagnostics
    if let Ok(diags) = compute_syntax_diagnostics(&source) {
        for diag in diags {
            let severity = diag.severity.unwrap_or(DiagnosticSeverity::ERROR);
            let line = diag.range.start.line + 1;
            let col = diag.range.start.character + 1;
            if severity == DiagnosticSeverity::ERROR {
                println!("{}:{}:{}: error: {}", file_path, line, col, diag.message);
                errors += 1;
            } else {
                println!("{}:{}:{}: warning: {}", file_path, line, col, diag.message);
                warnings += 1;
            }
        }
    }

    // Semantic diagnostics
    if let Ok(diags) = compute_symbol_diagnostics_with_globals(&file_path, &source, globals) {
        for diag in diags {
            let severity = diag.severity.unwrap_or(DiagnosticSeverity::WARNING);
            let line = diag.range.start.line + 1;
            let col = diag.range.start.character + 1;
            match severity {
                DiagnosticSeverity::ERROR => {
                    println!("{}:{}:{}: error: {}", file_path, line, col, diag.message);
                    errors += 1;
                },
                DiagnosticSeverity::WARNING => {
                    println!("{}:{}:{}: warning: {}", file_path, line, col, diag.message);
                    warnings += 1;
                },
                _ => {
                    // Skip hints
                },
            }
        }
    }

    (errors, warnings)
}

fn collect_files(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    if path.is_file() && path.extension().is_some_and(|e| e == "nut") {
        files.push(path.to_path_buf());
    } else if path.is_dir()
        && let Ok(entries) = fs::read_dir(path)
    {
        for entry in entries.flatten() {
            collect_files(&entry.path(), files);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.nut|directory> [...]", args[0]);
        eprintln!("\nRuns syntax and semantic checks on Squirrel files.");
        eprintln!("If a directory is given, recursively checks all .nut files.");
        std::process::exit(1);
    }

    let mut files = Vec::new();
    for arg in &args[1..] {
        collect_files(Path::new(arg), &mut files);
    }

    if files.is_empty() {
        eprintln!("No .nut files found");
        std::process::exit(1);
    }

    let globals = HashSet::new(); // Could be populated from workspace
    let mut total_errors = 0;
    let mut total_warnings = 0;

    for file in &files {
        let (e, w) = check_file(file, &globals);
        total_errors += e;
        total_warnings += w;
    }

    println!(
        "\nChecked {} files: {} errors, {} warnings",
        files.len(),
        total_errors,
        total_warnings
    );

    if total_errors > 0 {
        std::process::exit(1);
    }
}
