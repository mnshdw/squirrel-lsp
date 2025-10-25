mod formatter;

use std::collections::HashMap;
use std::sync::Arc;

use formatter::{FormatError, FormatOptions, IndentStyle, format_document};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, InitializeParams, InitializeResult,
    MessageType, OneOf, Position, Range, SemanticToken, SemanticTokenModifier, SemanticTokenType,
    SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensResult, SemanticTokensServerCapabilities,
    ServerCapabilities, ServerInfo, TextDocumentContentChangeEvent, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, async_trait};
use tree_sitter::Parser;

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, String>>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn get_document(&self, uri: &Url) -> Option<String> {
        let store = self.documents.read().await;
        store.get(uri).cloned()
    }

    fn map_formatting_options(options: &tower_lsp::lsp_types::FormattingOptions) -> FormatOptions {
        let tab_width = std::cmp::max(1, options.tab_size as usize);
        let indent_style = if options.insert_spaces {
            IndentStyle::Spaces(tab_width)
        } else {
            IndentStyle::Tabs
        };

        let mut format_options = FormatOptions::with_indent(indent_style);
        format_options.insert_final_newline = options.insert_final_newline.unwrap_or(true);
        format_options.trim_trailing_whitespace = options.trim_trailing_whitespace.unwrap_or(true);
        format_options
    }

    async fn handle_format_request(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let original = match self.get_document(&uri).await {
            Some(text) => text,
            None => return Ok(None),
        };

        let options = Self::map_formatting_options(&params.options);
        match format_document(&original, &options) {
            Ok(formatted) => {
                if formatted == original {
                    return Ok(Some(Vec::new()));
                }
                let edit = TextEdit::new(full_range(&original), formatted);
                Ok(Some(vec![edit]))
            },
            Err(err) => {
                self.report_format_error(&err).await;
                self.publish_syntax_diagnostics(uri, &original).await;
                Ok(None)
            },
        }
    }

    async fn report_format_error(&self, err: &FormatError) {
        self.client
            .log_message(MessageType::ERROR, format!("Formatting failed: {err}"))
            .await;
    }
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        let token_types = vec![
            SemanticTokenType::NAMESPACE,
            SemanticTokenType::TYPE,
            SemanticTokenType::CLASS,
            SemanticTokenType::ENUM,
            SemanticTokenType::INTERFACE,
            SemanticTokenType::STRUCT,
            SemanticTokenType::TYPE_PARAMETER,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::EVENT,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::METHOD,
            SemanticTokenType::MACRO,
            SemanticTokenType::KEYWORD,
            SemanticTokenType::MODIFIER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::REGEXP,
            SemanticTokenType::OPERATOR,
        ];

        let token_modifiers = vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
            SemanticTokenModifier::STATIC,
            SemanticTokenModifier::DEPRECATED,
            SemanticTokenModifier::ABSTRACT,
            SemanticTokenModifier::ASYNC,
            SemanticTokenModifier::MODIFICATION,
            SemanticTokenModifier::DOCUMENTATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ];

        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                },
            )),
            document_formatting_provider: Some(OneOf::Left(true)),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                    legend: SemanticTokensLegend {
                        token_types,
                        token_modifiers,
                    },
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    range: Some(false),
                    ..Default::default()
                }),
            ),
            ..ServerCapabilities::default()
        };

        Ok(InitializeResult {
            capabilities,
            server_info: Some(ServerInfo {
                name: "squirrel-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: tower_lsp::lsp_types::InitializedParams) {
        self.client
            .log_message(
                MessageType::INFO,
                "Squirrel formatter ready to format documents.",
            )
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut store = self.documents.write().await;
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        store.insert(uri.clone(), text.clone());
        self.publish_syntax_diagnostics(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut store = self.documents.write().await;
        let uri = params.text_document.uri;
        if let Some(entry) = store.get_mut(&uri)
            && let Some(TextDocumentContentChangeEvent { text, .. }) =
                params.content_changes.into_iter().next_back()
        {
            *entry = text;
            // Re-run diagnostics after change
            let current = entry.clone();
            drop(store);
            self.publish_syntax_diagnostics(uri, &current).await;
            return;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let mut store = self.documents.write().await;
        store.remove(&params.text_document.uri);
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        self.handle_format_request(params).await
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let text = match self.get_document(&uri).await {
            Some(text) => text,
            None => return Ok(None),
        };

        match compute_semantic_tokens(&text) {
            Ok(data) => Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data,
            }))),
            Err(err) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Semantic tokens failed: {err}"))
                    .await;
                Ok(None)
            },
        }
    }
}

fn full_range(text: &str) -> Range {
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    Range::new(Position::new(0, 0), Position::new(line, character))
}

fn position_at(text: &str, byte_offset: usize) -> Position {
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

impl Backend {
    async fn publish_syntax_diagnostics(&self, uri: Url, text: &str) {
        let diags = match compute_syntax_diagnostics(text) {
            Ok(d) => d,
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Failed to parse: {e}"))
                    .await;
                Vec::new()
            },
        };
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

fn compute_syntax_diagnostics(text: &str) -> std::result::Result<Vec<Diagnostic>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_squirrel::language())
        .map_err(|e| e.to_string())?;
    let Some(tree) = parser.parse(text, None) else {
        return Ok(Vec::new());
    };
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
            let range = Range::new(position_at(text, start), position_at(text, end));

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

fn compute_semantic_tokens(text: &str) -> std::result::Result<Vec<SemanticToken>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_squirrel::language())
        .map_err(|e| e.to_string())?;
    let Some(tree) = parser.parse(text, None) else {
        return Ok(Vec::new());
    };
    let root = tree.root_node();

    let mut tokens: Vec<(usize, usize, u32, u32)> = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;

    // Token modifier bit flags
    const MODIFIER_DECLARATION: u32 = 1 << 0;  // 1
    const MODIFIER_READONLY: u32 = 1 << 2;     // 4

    loop {
        let node = cursor.node();
        let kind = node.kind();

        // Process leaf nodes (including comments which are marked as extra)
        if node.child_count() == 0 {
            let (token_type, modifiers) = match kind {
                // Variables and identifiers
                "identifier" => {
                    // Check parent context for better classification
                    let parent = node.parent();
                    match parent.map(|p| p.kind()) {
                        Some("function_declaration") => {
                            // Function name declaration
                            (Some(12), MODIFIER_DECLARATION) // FUNCTION with declaration
                        },
                        Some("class_declaration") => {
                            (Some(2), MODIFIER_DECLARATION) // CLASS with declaration
                        },
                        Some("enum_declaration") => {
                            (Some(3), MODIFIER_DECLARATION) // ENUM with declaration
                        },
                        Some("const_declaration") => {
                            // Constants are readonly
                            (Some(8), MODIFIER_DECLARATION | MODIFIER_READONLY) // VARIABLE with readonly
                        },
                        Some("local_declaration") => {
                            // Local variable declaration
                            (Some(8), MODIFIER_DECLARATION) // VARIABLE with declaration
                        },
                        Some("var_statement") => {
                            // Var statement declaration
                            (Some(8), MODIFIER_DECLARATION) // VARIABLE with declaration
                        },
                        Some("parameter") => {
                            // Function parameters - use parameter type
                            (Some(7), MODIFIER_DECLARATION) // PARAMETER with declaration
                        },
                        Some("member_declaration") => {
                            // Class member/property
                            (Some(9), MODIFIER_DECLARATION) // PROPERTY with declaration
                        },
                        Some("deref_expression") => {
                            // Check if this deref is the function in a call_expression
                            if let Some(grandparent) = parent.and_then(|p| p.parent()) {
                                if grandparent.kind() == "call_expression" {
                                    // Check if parent deref is the 'function' field
                                    if let Some(p) = parent {
                                        if grandparent.child_by_field_name("function") == Some(p) {
                                            (Some(13), 0) // METHOD
                                        } else {
                                            (Some(9), 0) // PROPERTY
                                        }
                                    } else {
                                        (Some(9), 0) // PROPERTY
                                    }
                                } else {
                                    (Some(9), 0) // PROPERTY
                                }
                            } else {
                                (Some(9), 0) // PROPERTY
                            }
                        },
                        Some("call_expression") => {
                            // Function call
                            (Some(12), 0) // FUNCTION
                        },
                        _ => {
                            // Regular variable usage
                            (Some(8), 0) // VARIABLE
                        },
                    }
                },

                // Literals
                "integer" | "float" => (Some(19), 0), // NUMBER
                "string" | "string_content" | "verbatim_string" | "char" => (Some(18), 0), // STRING
                "true" | "false" => (Some(19), 0),    // NUMBER (boolean)
                "null" => (Some(15), 0),              // KEYWORD

                // Comments
                "comment" => (Some(17), 0), // COMMENT

                // Operators
                "=" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "+" | "-" | "*" | "/"
                | "%" | "++" | "--" | "&&" | "||" | "!" | "&" | "|" | "^" | "~" | "<<" | ">>"
                | ">>>" | "+=" | "-=" | "*=" | "/=" | "%=" | "<-" => (Some(21), 0), // OPERATOR

                // Keywords
                "const" | "local" | "var" | "static" | "if" | "else" | "for" | "foreach"
                | "while" | "do" | "switch" | "case" | "default" | "break" | "continue"
                | "return" | "yield" | "try" | "catch" | "throw" | "in" | "instanceof"
                | "typeof" | "delete" | "clone" | "resume" | "extends" | "constructor"
                | "rawcall" | "function" | "class" | "enum" => (Some(15), 0), // KEYWORD

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

    // Sort tokens by position
    tokens.sort_by_key(|(start, _, _, _)| *start);

    // Convert to LSP semantic tokens (delta-encoded)
    let mut semantic_tokens = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for (start_byte, end_byte, token_type, modifiers) in tokens {
        let start_pos = position_at(text, start_byte);
        let length = end_byte.saturating_sub(start_byte) as u32;

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
    }

    Ok(semantic_tokens)
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
