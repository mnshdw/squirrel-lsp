mod bb_support;
mod code_actions;
mod errors;
mod formatter;
mod helpers;
mod navigation;
mod semantic_analyzer;
mod symbol_extractor;
mod symbol_resolver;
mod symbols;
mod syntax_analyzer;
mod workspace;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bb_support::{analyze_hooks, analyze_inheritance};
use code_actions::generate_code_actions;
use formatter::{FormatError, FormatOptions, IndentStyle, format_document};
use symbol_resolver::compute_symbol_diagnostics_with_globals;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse,
    InitializeParams, InitializeResult, MessageType, OneOf, Position, Range, SemanticTokenModifier,
    SemanticTokenType, SemanticTokens, SemanticTokensFullOptions, SemanticTokensLegend,
    SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SymbolInformation,
    TextDocumentContentChangeEvent, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Url, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, LspService, Server, async_trait};
use workspace::Workspace;

use crate::semantic_analyzer::compute_semantic_tokens;
use crate::syntax_analyzer::compute_syntax_diagnostics;

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, String>>>,
    workspace: Arc<RwLock<Workspace>>,
    workspace_folders: Arc<RwLock<Vec<PathBuf>>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(Workspace::new())),
            workspace_folders: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Recursively find all .nut files in a directory
    fn find_nut_files(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Skip common non-source directories
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !name.starts_with('.') && name != "node_modules" && name != "target" {
                        files.extend(Self::find_nut_files(&path));
                    }
                } else if path.extension().is_some_and(|ext| ext == "nut") {
                    files.push(path);
                }
            }
        }
        files
    }

    /// Index all .nut files in the workspace
    async fn index_workspace(&self) {
        let folders = self.workspace_folders.read().await;
        if folders.is_empty() {
            self.client
                .log_message(MessageType::INFO, "No workspace folders to index")
                .await;
            return;
        }

        let mut all_files = Vec::new();
        for folder in folders.iter() {
            all_files.extend(Self::find_nut_files(folder));
        }

        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Indexing {} .nut files for class hierarchy...",
                    all_files.len()
                ),
            )
            .await;

        let mut workspace = self.workspace.write().await;
        let mut indexed_count = 0;
        let mut error_count = 0;

        for file_path in &all_files {
            if let Ok(content) = std::fs::read_to_string(file_path) {
                if let Err(e) = workspace.index_file(file_path, &content) {
                    error_count += 1;
                    // Only log first few errors to avoid spam
                    if error_count <= 5 {
                        self.client
                            .log_message(
                                MessageType::WARNING,
                                format!("Failed to index {}: {}", file_path.display(), e),
                            )
                            .await;
                    }
                } else {
                    indexed_count += 1;
                }
            }
        }

        // Build inheritance relationships after all files are indexed
        workspace.build_inheritance_graph();

        self.client
            .log_message(
                MessageType::INFO,
                format!(
                    "Indexed {} files from {} total ({} errors). Workspace has {} script paths.",
                    indexed_count,
                    all_files.len(),
                    error_count,
                    workspace.files().len()
                ),
            )
            .await;
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
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Store workspace folders for later indexing
        let mut folders = self.workspace_folders.write().await;
        if let Some(workspace_folders) = params.workspace_folders {
            for folder in workspace_folders {
                if let Ok(path) = folder.uri.to_file_path() {
                    folders.push(path);
                }
            }
        } else if let Some(root_uri) = params.root_uri
            && let Ok(path) = root_uri.to_file_path()
        {
            folders.push(path);
        }
        drop(folders);

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
            code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
                code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                resolve_provider: Some(false),
                work_done_progress_options: Default::default(),
            })),
            definition_provider: Some(OneOf::Left(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            workspace_symbol_provider: Some(OneOf::Left(true)),
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
                "Squirrel LSP initialized. Starting workspace indexing...",
            )
            .await;

        // Index the workspace in the background
        self.index_workspace().await;

        self.client
            .log_message(MessageType::INFO, "Squirrel LSP ready.")
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
        drop(store);

        // Update workspace index for this file
        if let Ok(path) = uri.to_file_path() {
            let mut workspace = self.workspace.write().await;
            let _ = workspace.index_file(&path, &text);
            workspace.build_inheritance_graph();
        }

        self.publish_syntax_diagnostics(uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut store = self.documents.write().await;
        let uri = params.text_document.uri;
        if let Some(entry) = store.get_mut(&uri)
            && let Some(TextDocumentContentChangeEvent { text, .. }) =
                params.content_changes.into_iter().next_back()
        {
            *entry = text.clone();
            drop(store);

            // Update workspace index for this file
            if let Ok(path) = uri.to_file_path() {
                let mut workspace = self.workspace.write().await;
                let _ = workspace.index_file(&path, &text);
                workspace.build_inheritance_graph();
            }

            self.publish_syntax_diagnostics(uri, &text).await;
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

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let text = match self.get_document(&uri).await {
            Some(text) => text,
            None => return Ok(None),
        };

        let actions = generate_code_actions(&text, &params.context.diagnostics, &uri);

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect(),
            ))
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let text = match self.get_document(&uri).await {
            Some(text) => text,
            None => return Ok(None),
        };

        let file_path = uri.to_file_path().unwrap_or_default();
        let workspace = self.workspace.read().await;

        if let Some(result) = navigation::find_definition(&text, position, &file_path, &workspace)
            && let Some(location) = navigation::definition_to_location(result)
        {
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        let text = match self.get_document(&uri).await {
            Some(text) => text,
            None => return Ok(None),
        };

        let symbols = navigation::get_document_symbols(&text);

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(symbols)))
        }
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let query = &params.query;

        // Get workspace symbols matching the query
        let workspace = self.workspace.read().await;
        let symbols = navigation::get_workspace_symbols(query, &workspace);

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(symbols))
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

        // Get workspace for globals and other analyses
        let workspace = self.workspace.read().await;

        // Get file path from URI for symbol resolution
        let file_path = uri
            .to_file_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| uri.path().to_string());

        // Collect semantic diagnostics using symbol resolver
        match compute_symbol_diagnostics_with_globals(&file_path, text, workspace.globals()) {
            Ok(semantic_diags) => {
                diags.extend(semantic_diags);
            },
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Semantic analysis failed: {e}"))
                    .await;
            },
        }

        // Analyze hooks for method validation
        match analyze_hooks(text, &workspace) {
            Ok(hook_diags) => {
                diags.extend(hook_diags);
            },
            Err(e) => {
                self.client
                    .log_message(MessageType::ERROR, format!("Hook analysis failed: {e}"))
                    .await;
            },
        }

        // Analyze inheritance patterns (validates parent paths exist and no circular inheritance)
        match analyze_inheritance(text, &workspace) {
            Ok(inherit_diags) => {
                diags.extend(inherit_diags);
            },
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::ERROR,
                        format!("Inheritance analysis failed: {e}"),
                    )
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
