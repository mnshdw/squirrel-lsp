//! Semantic token highlighting for Squirrel code.
//!
//! This module provides semantic tokens for syntax highlighting in editors.

use tower_lsp::lsp_types::SemanticToken;

use crate::errors::AnalysisError;
use crate::helpers;

const TOKEN_TYPE_CLASS: u32 = 2;
const TOKEN_TYPE_ENUM: u32 = 3;
const TOKEN_TYPE_PARAMETER: u32 = 7;
const TOKEN_TYPE_VARIABLE: u32 = 8;
const TOKEN_TYPE_PROPERTY: u32 = 9;
const TOKEN_TYPE_FUNCTION: u32 = 12;
const TOKEN_TYPE_METHOD: u32 = 13;
const TOKEN_TYPE_KEYWORD: u32 = 15;
const TOKEN_TYPE_COMMENT: u32 = 17;
const TOKEN_TYPE_STRING: u32 = 18;
const TOKEN_TYPE_NUMBER: u32 = 19;
const TOKEN_TYPE_OPERATOR: u32 = 21;

pub fn compute_semantic_tokens(text: &str) -> Result<Vec<SemanticToken>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut tokens: Vec<(usize, usize, u32, u32)> = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;

    const MODIFIER_DECLARATION: u32 = 1 << 0;
    const MODIFIER_READONLY: u32 = 1 << 2;

    loop {
        let node = cursor.node();
        let kind = node.kind();

        if node.child_count() == 0 {
            let (token_type, modifiers) = match kind {
                "identifier" => {
                    let parent = node.parent();
                    match parent.map(|p| p.kind()) {
                        Some("function_declaration") => {
                            (Some(TOKEN_TYPE_FUNCTION), MODIFIER_DECLARATION)
                        },
                        Some("class_declaration") => (Some(TOKEN_TYPE_CLASS), MODIFIER_DECLARATION),
                        Some("enum_declaration") => (Some(TOKEN_TYPE_ENUM), MODIFIER_DECLARATION),
                        Some("const_declaration") => (
                            Some(TOKEN_TYPE_VARIABLE),
                            MODIFIER_DECLARATION | MODIFIER_READONLY,
                        ),
                        Some("local_declaration") => {
                            (Some(TOKEN_TYPE_VARIABLE), MODIFIER_DECLARATION)
                        },
                        Some("var_statement") => (Some(TOKEN_TYPE_VARIABLE), MODIFIER_DECLARATION),
                        Some("parameter") => (Some(TOKEN_TYPE_PARAMETER), MODIFIER_DECLARATION),
                        Some("member_declaration") => {
                            (Some(TOKEN_TYPE_PROPERTY), MODIFIER_DECLARATION)
                        },
                        Some("deref_expression") => {
                            if let Some(grandparent) = parent.and_then(|p| p.parent()) {
                                if grandparent.kind() == "call_expression" {
                                    if let Some(p) = parent {
                                        if grandparent.child_by_field_name("function") == Some(p) {
                                            (Some(TOKEN_TYPE_METHOD), 0)
                                        } else {
                                            (Some(TOKEN_TYPE_PROPERTY), 0)
                                        }
                                    } else {
                                        (Some(TOKEN_TYPE_PROPERTY), 0)
                                    }
                                } else {
                                    (Some(TOKEN_TYPE_PROPERTY), 0)
                                }
                            } else {
                                (Some(TOKEN_TYPE_PROPERTY), 0)
                            }
                        },
                        Some("call_expression") => (Some(TOKEN_TYPE_FUNCTION), 0),
                        _ => (Some(TOKEN_TYPE_VARIABLE), 0),
                    }
                },

                "integer" | "float" => (Some(TOKEN_TYPE_NUMBER), 0),
                "string" | "string_content" | "verbatim_string" | "char" | "\"" => {
                    (Some(TOKEN_TYPE_STRING), 0)
                },
                "true" | "false" => (Some(TOKEN_TYPE_NUMBER), 0),
                "null" => (Some(TOKEN_TYPE_KEYWORD), 0),

                "comment" => (Some(TOKEN_TYPE_COMMENT), 0),

                "=" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "+" | "-" | "*" | "/"
                | "%" | "++" | "--" | "&&" | "||" | "!" | "&" | "|" | "^" | "~" | "<<" | ">>"
                | ">>>" | "+=" | "-=" | "*=" | "/=" | "%=" | "<-" => (Some(TOKEN_TYPE_OPERATOR), 0),

                "const" | "local" | "var" | "static" | "if" | "else" | "for" | "foreach"
                | "while" | "do" | "switch" | "case" | "default" | "break" | "continue"
                | "return" | "yield" | "try" | "catch" | "throw" | "in" | "instanceof"
                | "typeof" | "delete" | "clone" | "resume" | "extends" | "constructor"
                | "rawcall" | "function" | "class" | "enum" => (Some(TOKEN_TYPE_KEYWORD), 0),

                _ => (None, 0),
            };

            if let Some(token_type) = token_type {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                tokens.push((start_byte, end_byte, token_type, modifiers));
            }
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

    tokens.sort_by_key(|(start, _, _, _)| *start);

    let mut semantic_tokens = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for (start_byte, end_byte, token_type, modifiers) in tokens {
        let start_pos = helpers::position_at(text, start_byte);
        let end_pos = helpers::position_at(text, end_byte);

        if start_pos.line == end_pos.line {
            let length = end_pos.character - start_pos.character;

            let delta_line = start_pos.line - prev_line;
            let delta_start = if delta_line == 0 {
                start_pos.character - prev_col
            } else {
                start_pos.character
            };

            semantic_tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type,
                token_modifiers_bitset: modifiers,
            });

            prev_line = start_pos.line;
            prev_col = start_pos.character;
        } else {
            let token_text = &text[start_byte..end_byte];
            let mut current_line = start_pos.line;

            for (i, line_text) in token_text.split('\n').enumerate() {
                let line_start_col = if i == 0 { start_pos.character } else { 0 };
                let line_length = line_text.encode_utf16().count() as u32;

                let delta_line = current_line - prev_line;
                let delta_start = if delta_line == 0 {
                    line_start_col - prev_col
                } else {
                    line_start_col
                };

                semantic_tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length: line_length,
                    token_type,
                    token_modifiers_bitset: modifiers,
                });

                prev_line = current_line;
                prev_col = line_start_col + line_length;
                current_line += 1;
            }
        }
    }

    Ok(semantic_tokens)
}
