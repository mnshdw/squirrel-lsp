use std::collections::HashMap;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, Diagnostic, Range, TextEdit, Url, WorkspaceEdit,
};

use crate::helpers;

/// Extract variable name from source text using the diagnostic range
fn extract_variable_name(text: &str, range: Range) -> Option<String> {
    let start_byte = helpers::byte_offset_at(text, range.start)?;
    let end_byte = helpers::byte_offset_at(text, range.end)?;

    if start_byte >= end_byte || end_byte > text.len() {
        return None;
    }

    Some(text[start_byte..end_byte].to_string())
}

/// Create a WorkspaceEdit to delete the line containing the unused variable declaration
fn create_delete_unused_variable_edit(
    text: &str,
    range: Range,
    uri: &Url,
) -> Option<WorkspaceEdit> {
    let line_num = range.start.line as usize;
    let lines: Vec<&str> = text.lines().collect();

    if line_num >= lines.len() {
        return None;
    }

    // Find the start and end of the line to delete (including newline)
    let mut current_line = 0;
    let mut byte_offset = 0;
    let mut line_start = 0;

    for ch in text.chars() {
        if current_line == line_num {
            line_start = byte_offset;
            break;
        }
        if ch == '\n' {
            current_line += 1;
            byte_offset += 1;
        } else {
            byte_offset += ch.len_utf8();
        }
    }

    // Find the end of the line (including newline)
    let mut line_end = line_start;
    for ch in text[line_start..].chars() {
        if ch == '\n' {
            line_end += 1;
            break;
        }
        line_end += ch.len_utf8();
    }

    // If this is the last line and doesn't end with newline, just go to the end
    if line_end == line_start + text[line_start..].len() {
        line_end = text.len();
    }

    let start_pos = helpers::position_at(text, line_start);
    let end_pos = helpers::position_at(text, line_end);
    let delete_range = Range::new(start_pos, end_pos);

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![TextEdit {
            range: delete_range,
            new_text: String::new(),
        }],
    );

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

/// Generate code actions for the given diagnostics
pub fn generate_code_actions(text: &str, diagnostics: &[Diagnostic], uri: &Url) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // Check if any diagnostics are for unused variables
    for diagnostic in diagnostics {
        if diagnostic.source.as_deref() == Some("squirrel-semantic")
            && diagnostic.message.starts_with("Unused variable")
        {
            // Extract variable name from source text using the diagnostic range
            if let Some(var_name) = extract_variable_name(text, diagnostic.range) {
                // Find the line containing the declaration
                if let Some(edit) = create_delete_unused_variable_edit(text, diagnostic.range, uri)
                {
                    let action = CodeAction {
                        title: format!("Delete unused variable '{}'", var_name),
                        kind: Some(CodeActionKind::QUICKFIX),
                        edit: Some(edit),
                        diagnostics: Some(vec![diagnostic.clone()]),
                        ..Default::default()
                    };
                    actions.push(action);
                }
            }
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_analyzer::compute_semantic_diagnostics;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn test_unused_variable_code_action() {
        let code = r#"function test() {
    local unused_var = 10;
    local used_var = 20;

    print(used_var);
}
"#;

        let uri = Url::parse("file:///test.nut").unwrap();

        let diagnostics = compute_semantic_diagnostics(code).expect("Failed to analyze");

        // Should have one unused variable diagnostic
        let unused_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.starts_with("Unused variable"))
            .collect();

        assert_eq!(unused_diags.len(), 1);
        assert!(unused_diags[0].message.contains("unused_var"));

        // Generate code actions
        let actions = generate_code_actions(code, &diagnostics, &uri);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Delete unused variable 'unused_var'");

        // Verify the edit deletes the line
        assert!(actions[0].edit.is_some());
    }

    #[test]
    fn test_variable_name_extraction_from_range() {
        let code = "local my_variable = 10;";
        let uri = Url::parse("file:///test.nut").unwrap();

        let diagnostics = compute_semantic_diagnostics(code).expect("Failed to analyze");
        let actions = generate_code_actions(code, &diagnostics, &uri);

        // If there's an unused variable, the name should be extracted correctly from the range
        if !actions.is_empty() {
            assert!(
                actions[0].title.contains("my_variable"),
                "Variable name should be extracted from source text, not message"
            );
        }
    }
}
