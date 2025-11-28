use tower_lsp::lsp_types::Position;
use tree_sitter::{Node, Parser, Tree};

use crate::errors::AnalysisError;

/// Parse Squirrel source code and return the syntax tree
pub fn parse_squirrel(text: &str) -> Result<Tree, AnalysisError> {
    let mut parser = Parser::new();
    parser.set_language(tree_sitter_squirrel::language())?;
    parser.parse(text, None).ok_or(AnalysisError::ParseFailed)
}

/// Convert byte offset to LSP Position
pub(crate) fn position_at(text: &str, byte_offset: usize) -> Position {
    // Clamp to valid byte boundary
    let byte_offset = byte_offset.min(text.len());
    let mut line = 0u32;
    let mut col_utf16 = 0u32;
    let mut bytes_seen = 0usize;
    for ch in text.chars() {
        let ch_bytes = ch.len_utf8();
        if bytes_seen >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col_utf16 = 0;
        } else {
            col_utf16 += ch.len_utf16() as u32;
        }
        bytes_seen += ch_bytes;
    }
    Position::new(line, col_utf16)
}

/// Convert LSP Position to byte offset
pub(crate) fn byte_offset_at(text: &str, position: Position) -> Option<usize> {
    let mut current_line = 0u32;
    let mut current_col_utf16 = 0u32;
    let mut byte_offset = 0usize;

    for ch in text.chars() {
        if current_line == position.line && current_col_utf16 == position.character {
            return Some(byte_offset);
        }

        if current_line > position.line {
            // Position is beyond the end of the file
            return None;
        }

        if ch == '\n' {
            current_line += 1;
            current_col_utf16 = 0;
        } else {
            current_col_utf16 += ch.len_utf16() as u32;
        }

        byte_offset += ch.len_utf8();
    }

    // Check if we're at the end of the file
    if current_line == position.line && current_col_utf16 == position.character {
        Some(byte_offset)
    } else {
        None
    }
}

/// Get the text content of a tree-sitter node
pub fn node_text<'a>(node: Node, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

/// Extract string content from a string literal node.
///
/// Tries to find a `string_content` child first, then falls back to
/// trimming quotes from the node text.
pub fn extract_string_content(node: Node, text: &str) -> String {
    for child in node.children(&mut node.walk()) {
        if child.kind() == "string_content" {
            return node_text(child, text).to_string();
        }
    }
    // Fallback: trim quotes
    let s = node_text(node, text);
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Extract the name from an identifier or deref_expression node.
///
/// For `identifier` nodes, returns the identifier text directly.
/// For `deref_expression` nodes (like `this.foo`), returns the last identifier (`foo`).
/// Returns `None` for other node types.
pub fn extract_identifier_name(node: Node, text: &str) -> Option<String> {
    find_last_identifier(node).map(|n| node_text(n, text).to_string())
}

/// Find the last identifier node in a deref_expression chain.
///
/// For `identifier` nodes, returns the node itself.
/// For `deref_expression` nodes (like `this.foo.bar`), returns the last identifier node.
/// Recursively handles nested deref_expressions.
pub fn find_last_identifier(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" => Some(node),
        "deref_expression" => {
            let mut last = None;
            for child in node.children(&mut node.walk()) {
                if child.kind() == "identifier" {
                    last = Some(child);
                } else if child.kind() == "deref_expression"
                    && let Some(deeper) = find_last_identifier(child)
                {
                    last = Some(deeper);
                }
            }
            last
        },
        _ => None,
    }
}
