use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::errors::AnalysisError;
use crate::helpers;

pub fn compute_syntax_diagnostics(
    text: &str,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() || node.kind() == "ERROR" {
            let start = node.start_byte();
            let mut end = node.end_byte();
            if end <= start {
                end = (start + 1).min(text.len());
            }
            let range = Range::new(
                helpers::position_at(text, start),
                helpers::position_at(text, end),
            );

            let msg = if node.is_missing() {
                format!("Missing {}", node.kind())
            } else {
                let snippet = &text[start..end];
                let first = snippet.lines().next().unwrap_or("").trim();
                if first.is_empty() {
                    "Unexpected input".to_string()
                } else {
                    let display = if first.len() > 40 {
                        format!("{}…", &first[..40])
                    } else {
                        first.to_string()
                    };
                    format!("Unexpected '{}'", display)
                }
            };

            diags.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("squirrel-parser".to_string()),
                message: msg,
                ..Diagnostic::default()
            });
        }

        if !visited_children && cursor.goto_first_child() {
            visited_children = false;
            continue;
        }
        if cursor.goto_next_sibling() {
            visited_children = false;
            continue;
        }
        if !cursor.goto_parent() {
            break;
        }
        visited_children = true;
    }
    Ok(diags)
}
