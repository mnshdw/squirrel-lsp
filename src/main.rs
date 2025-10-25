mod formatter;

use std::collections::HashMap;
use std::sync::Arc;

use formatter::{FormatError, FormatOptions, IndentStyle, format_document};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, InitializeParams, InitializeResult,
    MessageType, OneOf, Position, Range, ServerCapabilities, ServerInfo,
    TextDocumentContentChangeEvent, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Url,
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
        let capabilities = ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    open_close: Some(true),
                    change: Some(TextDocumentSyncKind::FULL),
                    ..Default::default()
                },
            )),
            document_formatting_provider: Some(OneOf::Left(true)),
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

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
