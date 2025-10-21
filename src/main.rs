mod formatter;

use std::collections::HashMap;
use std::sync::Arc;

use formatter::{FormatError, FormatOptions, IndentStyle, format_document};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, InitializeParams, InitializeResult, MessageType, OneOf, Position,
    Range, ServerCapabilities, ServerInfo, TextDocumentContentChangeEvent,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, async_trait};

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
        // overwrite defaults with user-provided preferences when available
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
            }
            Err(err) => {
                self.report_format_error(&err).await;
                Ok(None)
            }
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
        store.insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut store = self.documents.write().await;
        if let Some(entry) = store.get_mut(&params.text_document.uri)
            && let Some(TextDocumentContentChangeEvent { text, .. }) =
                params.content_changes.into_iter().next_back()
        {
            *entry = text;
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

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
