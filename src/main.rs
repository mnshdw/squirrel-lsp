mod errors;
mod formatter;
mod helpers;
mod semantic_analyzer;
mod syntax_analyzer;

use std::collections::HashMap;
use std::sync::Arc;

use formatter::{FormatError, FormatOptions, IndentStyle, format_document};
use semantic_analyzer::compute_semantic_diagnostics;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, InitializeParams, InitializeResult, MessageType, OneOf, Position,
    Range, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentContentChangeEvent, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, async_trait};

use crate::semantic_analyzer::compute_semantic_tokens;
use crate::syntax_analyzer::compute_syntax_diagnostics;

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

impl Backend {
    async fn publish_syntax_diagnostics(&self, uri: Url, text: &str) {
        // Collect syntax diagnostics
        let mut diags = match compute_syntax_diagnostics(text) {
            Ok(syntax_diags) => syntax_diags,
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Failed to parse: {e}"))
                    .await;
                Vec::new()
            },
        };

        // Collect semantic diagnostics
        match compute_semantic_diagnostics(text) {
            Ok(semantic_diags) => {
                diags.extend(semantic_diags);
            },
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Semantic analysis failed: {e}"))
                    .await;
            },
        }

        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
