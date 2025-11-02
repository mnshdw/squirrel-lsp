use tower_lsp::lsp_types::Position;
use tree_sitter::{Parser, Tree};

use crate::errors::AnalysisError;

/// Parse Squirrel source code and return the syntax tree
pub(crate) fn parse_squirrel(text: &str) -> Result<Tree, AnalysisError> {
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
