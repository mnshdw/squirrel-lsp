use thiserror::Error;
use tree_sitter::Node;

use crate::helpers;

#[derive(Debug, Clone)]
pub struct FormatOptions {
    pub indent_style: IndentStyle,
    pub insert_final_newline: bool,
    pub trim_trailing_whitespace: bool,
    pub max_width: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent_style: IndentStyle::Tabs,
            insert_final_newline: true,
            trim_trailing_whitespace: true,
            max_width: 100,
        }
    }
}

impl FormatOptions {
    pub fn with_indent(indent_style: IndentStyle) -> Self {
        Self {
            indent_style,
            ..Self::default()
        }
    }

    fn push_indent(&self, buffer: &mut String, level: usize) {
        match self.indent_style {
            IndentStyle::Spaces(width) => {
                for _ in 0..level * width {
                    buffer.push(' ');
                }
            },
            IndentStyle::Tabs => {
                for _ in 0..level {
                    buffer.push('\t');
                }
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum IndentStyle {
    Spaces(usize),
    Tabs,
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("failed to configure squirrel parser: {0}")]
    Language(#[from] tree_sitter::LanguageError),
    #[error("failed to parse squirrel source")]
    ParseError,
    #[error("encountered invalid utf-8 in source text")]
    Utf8,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    kind: TokenKind,
    preceded_by_newline: bool,
    preceding_whitespace: String,
    starts_statement: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Keyword,
    Identifier,
    Number,
    String,
    Comment,
    Symbol,
    Other,
    Blankline,
}

#[derive(Debug, Clone)]
struct PrevToken {
    text: String,
    kind: TokenKind,
    was_unary: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BraceKind {
    ObjectInline,
    ObjectMultiline,
    Block,
    BlockInline,
    Switch,
    DoBlock,
}

impl BraceKind {
    fn is_object(self) -> bool {
        matches!(self, BraceKind::ObjectInline | BraceKind::ObjectMultiline)
    }

    fn is_inline(self) -> bool {
        matches!(self, BraceKind::ObjectInline | BraceKind::BlockInline)
    }
}

#[derive(Clone, Copy)]
struct BraceContext {
    kind: BraceKind,
    paren_depth_at_open: usize,
    bracket_depth_at_open: usize,
    // Switch-specific state (only used when kind == BraceKind::Switch)
    in_case_label: bool,
    case_body_indented: bool,
    // True if this brace was auto-inserted for single-statement if/else
    is_synthetic: bool,
}

#[derive(Debug, Clone, Copy)]
enum ParenKind {
    For,
    If,
    Switch,
    Function,
    Regular,
}

#[derive(Clone, Copy)]
struct ParenContext {
    kind: ParenKind,
    bracket_depth_at_open: usize,
    multiline: bool,
    indented: bool,
}

#[derive(Clone, Copy)]
struct BracketContext {
    pretty_print: bool,
    /// Output position where the '[' was written
    start_output_pos: usize,
}

#[derive(Clone, Copy)]
struct TernaryContext {
    /// Total depth (paren + bracket) when the ternary started
    depth_at_start: usize,
}

pub fn format_document(source: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let tree = helpers::parse_squirrel(source).map_err(|_| FormatError::ParseError)?;
    let root = tree.root_node();
    // Be tolerant of parse errors: many Squirrel files use a more lenient syntax
    // than the official grammar. Tree-sitter still produces a
    // concrete syntax tree with ERROR nodes, so we can continue token collection and
    // formatting without crashing the server or tests. This makes the formatter resilient
    // while the grammar is extended to support lenient variants.
    // if root.has_error() { return Err(FormatError::ParseError); }

    let tokens = collect_tokens(root, source)?;

    let mut formatter = Formatter::new(options);
    for (idx, token) in tokens.iter().enumerate() {
        let next = tokens.get(idx + 1);
        let remaining = &tokens[idx + 1..];
        formatter.write_token(token, next, remaining);
    }

    let mut output = formatter.finish();
    if options.insert_final_newline && !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

struct Formatter<'a> {
    options: &'a FormatOptions,
    output: String,
    indent_level: usize,
    paren_depth: usize,
    bracket_depth: usize,
    needs_indent: bool,
    pending_space: bool,
    prev: Vec<PrevToken>,
    braces: Vec<BraceContext>,
    parens: Vec<ParenContext>,
    brackets: Vec<BracketContext>,
    ternaries: Vec<TernaryContext>,
    // Track the kind of the last closed paren (used to detect switch blocks before '{')
    last_closed_paren_kind: Option<ParenKind>,
    // Track the paren_depth at which we started breaking logical operators
    breaking_logical_at_depth: Option<usize>,
    // Track the paren_depth at which we started breaking concat operators
    breaking_concat_at_depth: Option<usize>,
}

impl<'a> Formatter<'a> {
    fn new(options: &'a FormatOptions) -> Self {
        Self {
            options,
            output: String::new(),
            indent_level: 0,
            paren_depth: 0,
            bracket_depth: 0,
            needs_indent: true,
            pending_space: false,
            prev: Vec::new(),
            braces: Vec::new(),
            parens: Vec::new(),
            brackets: Vec::new(),
            ternaries: Vec::new(),
            last_closed_paren_kind: None,
            breaking_logical_at_depth: None,
            breaking_concat_at_depth: None,
        }
    }

    fn finish(mut self) -> String {
        // A single-statement block that runs to the end of the file still has to be closed
        self.close_synthetic_blocks(None);

        if self.options.trim_trailing_whitespace {
            trim_trailing_whitespace(&mut self.output);
        }
        self.output
    }

    fn total_depth(&self) -> usize {
        self.paren_depth + self.bracket_depth
    }

    fn write_token(&mut self, token: &Token, next: Option<&Token>, remaining: &[Token]) {
        // A statement the source started on its own line is separated by that newline, which
        // may be the only thing separating it from the previous one. Keep it. Statements the
        // source wrote on one line ('if (x) break') are left to the rules below.
        if token.starts_statement
            && token.preceded_by_newline
            && !self.output.is_empty()
            && !matches!(token.kind, TokenKind::Comment | TokenKind::Blankline)
        {
            self.end_statement();
            self.close_synthetic_blocks(Some(token));
            self.push_newline();
        }

        // A comment between two statements is (in theory) not part of either, so the state the
        // previous statement left behind (a ternary indent) must not indent it.
        if token.kind == TokenKind::Comment
            && token.preceded_by_newline
            && Self::next_non_comment(remaining)
                .is_some_and(|t| t.starts_statement && t.preceded_by_newline)
        {
            self.end_statement();
        }

        // A blank line after the statement of a single-statement block belongs after the
        // block, not inside it. Nothing written since the '{' means the statement is still
        // to come, and the block stays open.
        if token.kind == TokenKind::Blankline
            && self.braces.last().is_some_and(|b| b.is_synthetic)
            && !self.output.trim_end().ends_with('{')
        {
            self.close_synthetic_blocks(None);
        }

        // Handle case/default in switch blocks before other processing
        if self.in_switch_block() && matches!(token.text.as_str(), "case" | "default") {
            self.close_synthetic_blocks(Some(token));
            self.write_case_label(token);
            return;
        }

        let is_symbol = token.kind == TokenKind::Symbol;
        match token.text.as_str() {
            "{" if is_symbol => self.write_open_brace(token, next),
            "}" if is_symbol => {
                // The enclosing block ends, so any single-statement block inside it ends too
                self.close_synthetic_blocks(Some(token));
                self.write_close_brace(token, next);
            },
            ";" if is_symbol => self.write_semicolon(token, next),
            "," if is_symbol => self.write_comma(token, next),
            "(" if is_symbol => self.write_open_paren(token, remaining),
            ")" if is_symbol => self.write_close_paren(token, remaining),
            "[" if is_symbol => self.write_open_bracket(token, next, remaining),
            "]" if is_symbol => self.write_close_bracket(token),
            "." | "::" => self.write_member_access(token),
            "?" => self.write_ternary(token, remaining),
            ":" => self.write_colon(token, next),
            "++" | "--" => self.write_increment(token),
            "else" if token.kind == TokenKind::Keyword => self.write_else(token, remaining),
            _ if token.kind == TokenKind::Comment => self.write_comment(token),
            _ if token.kind == TokenKind::Blankline => self.write_blankline(next),
            _ if token.kind != TokenKind::String && is_operator(token.text.as_str()) => {
                self.write_operator(token, remaining)
            },
            _ if self.is_binary_signed_number(token) => self.write_signed_number(token),
            _ => self.write_default(token),
        }
    }

    /// Helper function to find the next non-comment token
    fn next_non_comment(remaining: &[Token]) -> Option<&Token> {
        remaining
            .iter()
            .find(|t| t.kind != TokenKind::Comment && t.kind != TokenKind::Blankline)
    }

    /// Check if a token is an inline comment (not preceded by newline)
    fn is_inline_comment(token: &Token) -> bool {
        token.kind == TokenKind::Comment
            && is_line_comment(token.text.trim_start())
            && !token.preceded_by_newline
    }

    fn ensure_indent(&mut self) {
        if self.needs_indent {
            self.options
                .push_indent(&mut self.output, self.indent_level);
            self.needs_indent = false;
        }
    }

    fn ends_with_whitespace(&self) -> bool {
        matches!(
            self.output.chars().last(),
            Some(' ') | Some('\n') | Some('\t')
        )
    }

    fn in_for_header(&self) -> bool {
        self.paren_depth > 0
            && self
                .parens
                .last()
                .is_some_and(|f| matches!(f.kind, ParenKind::For))
    }

    fn in_function_params(&self) -> bool {
        self.paren_depth > 0
            && self
                .parens
                .last()
                .is_some_and(|f| matches!(f.kind, ParenKind::Function))
    }

    fn in_multiline_call(&self) -> bool {
        self.paren_depth > 0 && self.parens.last().is_some_and(|f| f.multiline)
    }

    fn in_object_top_level(&self) -> bool {
        self.braces.last().is_some_and(|f| {
            f.kind == BraceKind::ObjectMultiline
                && f.paren_depth_at_open == self.paren_depth
                && f.bracket_depth_at_open == self.bracket_depth
        })
    }

    // True when we're positioned at the top level of an object literal (either inline or multiline)
    // with matching paren/bracket depth where properties are written (i.e., not inside nested () or []).
    fn in_object_property_position(&self) -> bool {
        self.braces.last().is_some_and(|f| {
            f.kind.is_object()
                && f.paren_depth_at_open == self.paren_depth
                && f.bracket_depth_at_open == self.bracket_depth
        })
    }

    fn in_pretty_array(&self) -> bool {
        let paren_bracket_depth = self
            .parens
            .last()
            .map(|f| f.bracket_depth_at_open)
            .unwrap_or(0);
        let bracket_opened_in_paren =
            self.paren_depth > 0 && self.bracket_depth > paren_bracket_depth;
        self.brackets
            .last()
            .map(|b| b.pretty_print)
            .unwrap_or(false)
            && (self.paren_depth == 0 || bracket_opened_in_paren)
    }

    fn in_switch_block(&self) -> bool {
        self.braces
            .last()
            .is_some_and(|f| f.kind == BraceKind::Switch)
    }

    fn apply_pending_space(&mut self) {
        if self.pending_space && !self.ends_with_whitespace() {
            self.output.push(' ');
        }
        self.pending_space = false;
    }

    fn push_newline(&mut self) {
        if self.options.trim_trailing_whitespace {
            trim_trailing_whitespace_line(&mut self.output);
        }
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.needs_indent = true;
        self.pending_space = false;
        self.prev.clear();
    }

    fn prev(&self) -> Option<&PrevToken> {
        self.prev.last()
    }

    fn prev_n(&self, n: usize) -> Option<&PrevToken> {
        let len = self.prev.len();
        if len > n {
            self.prev.get(len - 1 - n)
        } else {
            None
        }
    }

    fn push_prev(&mut self, token: &Token, was_unary: bool) {
        self.prev.push(PrevToken {
            text: token.text.clone(),
            kind: token.kind,
            was_unary,
        });
        // Keep only last few tokens
        if self.prev.len() > 3 {
            self.prev.remove(0);
        }
    }

    fn set_prev(&mut self, token: &Token) {
        self.push_prev(token, false);
    }

    fn prepare_token(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();
        let prev_was_unary = self.prev().is_some_and(|p| p.was_unary);
        if !prev_was_unary && needs_space(self.prev(), token) && !self.ends_with_whitespace() {
            self.output.push(' ');
        }
    }

    fn write_open_brace(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);

        // Determine brace kind (object literal vs code block, inline vs multiline)
        // Check if the previous closing paren was for a switch statement
        let is_switch = matches!(self.last_closed_paren_kind, Some(ParenKind::Switch));
        let is_block = self
            .prev()
            .is_some_and(|p| p.text == ")" || is_block_introducing_keyword(p.text.as_str()));
        let is_empty = matches!(next.map(|n| n.text.as_str()), Some("}"));
        let is_do = self.prev().is_some_and(|p| p.text == "do");

        let kind = if is_switch {
            BraceKind::Switch
        } else if is_do {
            BraceKind::DoBlock
        } else if is_empty && !is_block {
            BraceKind::ObjectInline
        } else if is_empty && matches!(self.last_closed_paren_kind, Some(ParenKind::Function)) {
            BraceKind::BlockInline
        } else if is_block {
            BraceKind::Block
        } else {
            BraceKind::ObjectMultiline
        };

        self.output.push('{');

        self.braces.push(BraceContext {
            kind,
            paren_depth_at_open: self.paren_depth,
            bracket_depth_at_open: self.bracket_depth,
            in_case_label: false,
            case_body_indented: false,
            is_synthetic: false,
        });

        // Clear the last closed paren after consuming it
        self.last_closed_paren_kind = None;

        if kind.is_inline() {
            // Keep {} inline (no indent or newline)
            self.set_prev(token);
            return;
        }

        self.indent_level += 1;
        self.push_newline();
    }

    fn write_close_brace(&mut self, token: &Token, next: Option<&Token>) {
        let frame = self.braces.pop();
        let kind = frame.map(|f| f.kind);
        let inline = kind.is_some_and(|k| k.is_inline());
        let is_object = kind.is_some_and(|k| k.is_object());

        // If closing a switch block with an active case body, dedent the case body first
        if let Some(f) = frame
            && f.kind == BraceKind::Switch
            && f.case_body_indented
        {
            self.indent_level = self.indent_level.saturating_sub(1);
        }

        if !inline {
            self.indent_level = self.indent_level.saturating_sub(1);
        }
        if !self.output.ends_with('\n') && !inline {
            self.push_newline();
        }

        // Extra indent for objects in non-pretty-printed arrays (aligns } with ])
        let next_is_bracket = matches!(next.map(|t| t.text.as_str()), Some("]"));
        let in_pretty_array = self
            .brackets
            .last()
            .map(|b| b.pretty_print)
            .unwrap_or(false);
        let needs_array_indent = !inline && next_is_bracket && !in_pretty_array && is_object;

        if needs_array_indent {
            self.indent_level += 1;
        }

        // Default: emit closing brace
        self.ensure_indent();
        self.output.push('}');
        if needs_array_indent {
            self.indent_level = self.indent_level.saturating_sub(1);
        }
        self.set_prev(token);

        // Determine what follows the brace
        if let Some(next_token) = next {
            match next_token.text.as_str() {
                ")" | ";" | "," | "." => {
                    self.needs_indent = false;
                    return;
                },
                "else" | "catch" | "finally" => {
                    self.output.push(' ');
                    self.needs_indent = false;
                    self.prev.clear();
                    return;
                },
                "while" if kind == Some(BraceKind::DoBlock) => {
                    self.output.push(' ');
                    self.needs_indent = false;
                    self.prev.clear();
                    return;
                },
                _ if Self::is_inline_comment(next_token) => {
                    self.needs_indent = false;
                    return;
                },
                _ => {},
            }
        }

        if !inline || kind == Some(BraceKind::BlockInline) {
            self.push_newline();
        }
    }

    fn write_semicolon(&mut self, token: &Token, next: Option<&Token>) {
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(';');

        self.end_statement();

        // If a line comment follows on the same line (not preceded by newline),
        // keep it on the same line.
        if next.is_some_and(Self::is_inline_comment) {
            self.set_prev(token);
            return;
        }

        if self.in_for_header() {
            self.output.push(' ');
            self.set_prev(token);
        } else {
            self.push_newline();
        }

        // If we auto-opened a block for a single-statement if/else, close it now
        self.close_synthetic_blocks(next);
    }

    /// Close the blocks auto-opened for a single-statement if/else.
    ///
    /// The statement they wrap ends at a ';' but also at the newline before whatever comes
    /// next, so this runs at every statement boundary. A block left open would swallow the
    /// rest of the file.
    fn close_synthetic_blocks(&mut self, next: Option<&Token>) {
        while self.braces.last().is_some_and(|b| b.is_synthetic) {
            let synthetic = Token {
                text: "}".to_string(),
                kind: TokenKind::Symbol,
                preceded_by_newline: false,
                preceding_whitespace: String::new(),
                starts_statement: false,
            };
            self.write_close_brace(&synthetic, next);
        }
    }

    fn write_comma(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);

        let in_object_top_level = self.in_object_top_level();
        let in_function_params = self.in_function_params();
        let in_pretty_array = self.in_pretty_array();
        let in_multiline_call = self.in_multiline_call();

        self.output.push(',');

        // If a line comment follows on the same line (not preceded by newline),
        // keep it on the same line.
        if next.is_some_and(Self::is_inline_comment) {
            self.set_prev(token);
            return;
        }

        if in_object_top_level && !in_function_params {
            match next {
                Some(t) if t.text.as_str() == "function" => self.write_blankline(next),
                _ => self.push_newline(),
            }
        } else if in_multiline_call {
            self.push_newline();
        } else if in_pretty_array {
            // In a pretty-printed array, commas should create newlines
            self.push_newline();
        } else {
            let should_space = next.is_none_or(|t| !matches!(t.text.as_str(), ")" | "]" | "}"));
            if should_space {
                self.output.push(' ');
            }
        }
        self.set_prev(token);
    }

    fn write_open_paren(&mut self, token: &Token, remaining: &[Token]) {
        self.prepare_token(token);
        self.output.push('(');
        self.paren_depth += 1;

        let kind = match self.prev().map(|p| p.text.as_str()) {
            Some("for") => ParenKind::For,
            Some("if") => ParenKind::If,
            Some("switch") => ParenKind::Switch,
            Some("function") => ParenKind::Function,
            _ if self.prev_n(1).is_some_and(|p| p.text == "function") => ParenKind::Function,
            _ => ParenKind::Regular,
        };

        // Preserve a source newline after `(`, but not for trivial closers or
        // for function calls whose first arg manages its own formatting.
        let next_breaks_line = remaining
            .first()
            .is_some_and(|t| t.preceded_by_newline && !matches!(t.text.as_str(), ")" | "[" | "{"));
        let should_multiline = next_breaks_line && !matches!(kind, ParenKind::Function);
        let should_indent = should_multiline && matches!(kind, ParenKind::Regular);

        self.parens.push(ParenContext {
            kind,
            bracket_depth_at_open: self.bracket_depth,
            multiline: should_multiline,
            indented: should_indent,
        });

        if should_indent {
            self.indent_level += 1;
            self.push_newline();
        }

        self.set_prev(token);
    }

    fn write_close_paren(&mut self, token: &Token, remaining: &[Token]) {
        self.paren_depth = self.paren_depth.saturating_sub(1);

        // If we're closing a paren and a ternary indent is active, reset it
        while let Some(ctx) = self.ternaries.last() {
            if self.total_depth() < ctx.depth_at_start {
                self.indent_level = self.indent_level.saturating_sub(1);
                self.ternaries.pop();
            } else {
                break;
            }
        }

        let frame = self.parens.pop();
        let frame_kind = frame.as_ref().map(|f| f.kind);
        let is_if_header = frame_kind.is_some_and(|k| matches!(k, ParenKind::If));
        let was_multiline = frame.is_some_and(|f| f.multiline);
        let was_indented = frame.is_some_and(|f| f.indented);

        // Track the paren kind for the next open brace (e.g., to detect switch blocks)
        self.last_closed_paren_kind = frame_kind;

        if was_indented {
            self.indent_level = self.indent_level.saturating_sub(1);
        }

        // For multiline function calls, close paren goes on its own line.
        if was_multiline
            && matches!(frame_kind, Some(ParenKind::Regular))
            && !self.output.ends_with('\n')
        {
            self.push_newline();
        }

        if matches!(
            frame_kind,
            Some(ParenKind::If | ParenKind::For | ParenKind::Switch)
        ) {
            self.breaking_logical_at_depth = None;
            self.breaking_concat_at_depth = None;
        }

        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(')');

        // Check if there's an inline comment immediately after the closing paren
        let next_is_inline_comment = remaining.first().is_some_and(Self::is_inline_comment);

        // Look ahead past comments to find the next non-comment token
        let next_non_comment = Self::next_non_comment(remaining);
        let next_is_brace = next_non_comment.is_some_and(|t| t.text == "{");
        if next_is_brace {
            // Place opening brace on a new line if the condition was multiline
            if was_multiline {
                self.push_newline();
            } else if !next_is_inline_comment {
                self.output.push(' ');
                self.needs_indent = false;
            }
        } else if is_if_header {
            // Auto-insert a block for single-statement ifs
            self.output.push(' ');
            self.output.push('{');
            self.indent_level += 1;
            self.push_newline();

            // Push synthetic brace context
            self.braces.push(BraceContext {
                kind: BraceKind::Block,
                paren_depth_at_open: self.paren_depth,
                bracket_depth_at_open: self.bracket_depth,
                in_case_label: false,
                case_body_indented: false,
                is_synthetic: true,
            });
        }
        self.set_prev(token);
    }

    fn write_open_bracket(&mut self, token: &Token, next: Option<&Token>, remaining: &[Token]) {
        self.prepare_token(token);
        self.output.push('[');
        self.bracket_depth += 1;

        // Detect if this is an array subscript (foo[x]) vs array literal ([1, 2, 3])
        let is_subscript = self.prev().is_some_and(|p| {
            matches!(
                p.kind,
                TokenKind::Identifier | TokenKind::Number | TokenKind::String
            ) || matches!(p.text.as_str(), "]" | ")" | "}")
        });

        // Don't pretty-print array subscripts or empty arrays
        let is_empty = matches!(next.map(|n| n.text.as_str()), Some("]"));

        if is_subscript || is_empty {
            self.brackets.push(BracketContext {
                pretty_print: false,
                start_output_pos: self.output.len(),
            });
            self.set_prev(token);
            return;
        }

        // Enable pretty-printing for arrays of objects/arrays
        let next_is_complex = matches!(next.map(|n| n.text.as_str()), Some("{") | Some("["));

        // Estimate if array content would exceed max_width chars on one line
        let estimated_length = self.estimate_array_length(remaining);

        // For arrays inside function calls, only check the array length itself.
        // For top-level arrays (assignments, etc.), check the full line length.
        let would_be_too_long = if self.paren_depth > 0 {
            estimated_length > self.options.max_width
        } else {
            let current_line_length = self.get_current_line_length();
            current_line_length + estimated_length > self.options.max_width
        };

        // If the input had a newline after '[', keep it for consistency
        let user_pref = {
            let first = remaining.first();
            let check_token = if first.is_some_and(Self::is_inline_comment) {
                remaining.get(1)
            } else {
                first
            };
            check_token.is_some_and(|t| t.preceded_by_newline && !matches!(t.text.as_str(), "]"))
        };

        // Pretty-print if:
        // - Contains complex elements (objects/arrays)
        // - Would exceed max_width
        // - User explicitly formatted it multiline
        let should_pretty_print = next_is_complex || would_be_too_long || user_pref;

        self.brackets.push(BracketContext {
            pretty_print: should_pretty_print,
            start_output_pos: self.output.len(),
        });

        if should_pretty_print {
            if !remaining.first().is_some_and(Self::is_inline_comment) {
                self.push_newline();
            }
            self.indent_level += 1;
        }
        self.set_prev(token);
    }

    fn write_close_bracket(&mut self, token: &Token) {
        self.bracket_depth = self.bracket_depth.saturating_sub(1);

        let ctx = self.brackets.pop();
        let was_pretty = ctx.map(|c| c.pretty_print).unwrap_or(false);
        let start_idx = ctx.map(|c| c.start_output_pos).unwrap_or(self.output.len());

        if was_pretty {
            self.indent_level = self.indent_level.saturating_sub(1);
            if !self.output.ends_with('\n') {
                self.push_newline();
            }
        } else {
            let had_newline_since_open = self.output[start_idx..].contains('\n');
            if had_newline_since_open && !self.output.ends_with('\n') {
                self.push_newline();
            }
        }
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(']');
        self.set_prev(token);
    }

    fn write_member_access(&mut self, token: &Token) {
        self.prepare_token(token);

        let keep_space = token.text == "::"
            || self.prev().is_some_and(|p| {
                p.kind == TokenKind::Keyword
                    || is_operator(&p.text)
                    || p.text == ","
                    || p.text == ":"
                    || p.text == "?"
            });
        if self.output.ends_with(' ') && !keep_space {
            self.output.pop();
        }
        self.output.push_str(&token.text);
        self.set_prev(token);
    }

    fn write_ternary(&mut self, token: &Token, remaining: &[Token]) {
        let line_length = self.get_current_line_length();
        let estimated_length = self.estimate_ternary_length(remaining);

        // " ? " contributes 3 characters
        let would_exceed = line_length + 3 + estimated_length > self.options.max_width;

        if would_exceed {
            // Break to new line and indent
            self.push_newline();
            self.indent_level += 1;
            self.ternaries.push(TernaryContext {
                depth_at_start: self.total_depth(),
            });
            self.ensure_indent();
            self.output.push('?');
            self.output.push(' ');
            self.set_prev(token);
            return;
        }

        // Default inline formatting
        self.prepare_token(token);
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push('?');
        self.output.push(' ');
        self.set_prev(token);
    }

    fn write_colon(&mut self, token: &Token, next: Option<&Token>) {
        // Handle case/default label colons specially
        let in_case_label = self
            .braces
            .last()
            .is_some_and(|f| f.kind == BraceKind::Switch && f.in_case_label);

        if in_case_label {
            // For case labels, remove any pending space before the colon
            if self.output.ends_with(' ') {
                self.output.pop();
            }
            self.ensure_indent();
            self.output.push(':');
            self.push_newline();
            self.indent_level += 1;

            // Mark the case body as indented in the switch frame
            if let Some(frame) = self.braces.last_mut() {
                frame.case_body_indented = true;
                frame.in_case_label = false;
            }
            return;
        }

        // Decide between ternary colon and object property colon
        let prev_is_question = self.prev().is_some_and(|p| p.text.as_str() == "?");
        let in_object_property = self.in_object_property_position();

        if !self.ternaries.is_empty() && !in_object_property {
            if !self.output.ends_with('\n') {
                self.push_newline();
            }

            self.ensure_indent();
            self.output.push(':');
            self.output.push(' ');
            self.set_prev(token);
            return;
        }

        self.prepare_token(token);

        if prev_is_question {
            // Inline ternary: ensure space before and after
            if !self.output.ends_with(' ') {
                self.output.push(' ');
            }
            self.output.push(':');
            self.output.push(' ');
            self.set_prev(token);
            return;
        }

        if in_object_property {
            // Object property: no space before colon, optional space after
            if self.output.ends_with(' ') {
                self.output.pop();
            }
            self.output.push(':');
            let should_space = !matches!(next.map(|t| t.text.as_str()), Some("}" | "," | ";"));
            if should_space {
                self.output.push(' ');
            }
            self.set_prev(token);
            return;
        }

        // Default behavior: treat as ternary-style spacing
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push(':');
        self.output.push(' ');
        self.set_prev(token);
    }

    fn write_increment(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.set_prev(token);
    }

    fn write_operator(&mut self, token: &Token, remaining: &[Token]) {
        if is_unary_operator(token.text.as_str()) && is_unary_context(self.prev()) {
            self.write_unary_operator(token);
            return;
        }

        let is_logical_op = matches!(token.text.as_str(), "&&" | "||");
        let is_binary_op = matches!(
            token.text.as_str(),
            "+" | "-" | "*" | "/" | "==" | "!=" | "<" | "<=" | ">" | ">="
        );

        // Logical operators can break anywhere when lines are too long
        // Other binary operators only break at top level (paren_depth == 0),
        // except for string concat chains which are allowed to break at any depth.
        let in_string_concat = token.text == "+"
            && self.prev().is_some_and(|p| p.kind == TokenKind::String)
            && remaining
                .iter()
                .find(|t| t.kind != TokenKind::Blankline && t.kind != TokenKind::Comment)
                .is_some_and(|t| t.kind == TokenKind::String);
        let can_break = is_logical_op || self.paren_depth == 0 || in_string_concat;

        if (is_logical_op || is_binary_op)
            && can_break
            && self.should_break_before_operator(token, remaining, is_logical_op)
        {
            self.write_operator_with_line_break(token, is_logical_op);
            return;
        }

        self.write_operator_default(token);
    }

    fn write_unary_operator(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        // Mark that this was a unary operator so the next token doesn't
        // get a space inserted after it.
        self.push_prev(token, true);
    }

    fn is_in_condition(&self) -> bool {
        self.paren_depth > 0
            && self
                .parens
                .last()
                .is_some_and(|f| matches!(f.kind, ParenKind::If | ParenKind::Switch))
    }

    fn is_at_condition_top_level(&self) -> bool {
        self.parens
            .last()
            .is_some_and(|f| matches!(f.kind, ParenKind::If | ParenKind::For | ParenKind::Switch))
    }

    fn should_break_before_operator(
        &self,
        token: &Token,
        remaining: &[Token],
        is_logical_op: bool,
    ) -> bool {
        let line_length = self.get_current_line_length();
        let in_condition = self.is_in_condition();
        let at_condition_top_level = self.is_at_condition_top_level();

        let active_break_depth = if is_logical_op {
            self.breaking_logical_at_depth
        } else {
            self.breaking_concat_at_depth
        };

        let should_break = if active_break_depth == Some(self.paren_depth) {
            // Already breaking operators of this kind at this depth
            true
        } else if is_logical_op {
            let estimated_remaining = if in_condition {
                self.estimate_paren_content_length(remaining)
            } else {
                self.estimate_statement_length(remaining)
            };
            // " <op> " contributes 1 + op.len() + 1 characters
            let op_len = 1 + token.text.len() + 1;
            // For conditions, also include ") {"
            let cond_len = if in_condition { 3 } else { 0 };
            line_length + op_len + estimated_remaining + cond_len > self.options.max_width
        } else {
            let op_len = 1 + token.text.len() + 1;
            line_length + op_len + Self::next_operand_len(remaining) > self.options.max_width
        };

        if !should_break {
            return false;
        }

        // Prefer breaking at the top-level of a condition, not inside nested
        // parenthesized sub-expressions like `(a || b)`.
        let inside_any_condition = self
            .parens
            .iter()
            .any(|f| matches!(f.kind, ParenKind::If | ParenKind::For | ParenKind::Switch));

        // Don't break inside nested conditions (e.g., inside `(a || b)` within an if)
        if is_logical_op && inside_any_condition && !at_condition_top_level {
            return false;
        }

        true
    }

    /// Treats consecutive String tokens (open quote / body / close quote) as one operand.
    fn next_operand_len(remaining: &[Token]) -> usize {
        let mut len = 0;
        let mut in_string = false;
        for t in remaining {
            if matches!(t.kind, TokenKind::Blankline | TokenKind::Comment) {
                continue;
            }
            if t.kind == TokenKind::String {
                len += t.text.len();
                in_string = true;
            } else {
                if !in_string {
                    len += t.text.len();
                }
                break;
            }
        }
        len
    }

    fn write_operator_with_line_break(&mut self, token: &Token, is_logical_op: bool) {
        // Mark that we're breaking operators at this depth
        if is_logical_op && self.breaking_logical_at_depth.is_none() {
            self.breaking_logical_at_depth = Some(self.paren_depth);
        } else if !is_logical_op && self.breaking_concat_at_depth.is_none() {
            self.breaking_concat_at_depth = Some(self.paren_depth);
        }

        // A line break inside an if/for/switch makes the content multiline
        if let Some(frame) = self.parens.last_mut()
            && matches!(
                frame.kind,
                ParenKind::If | ParenKind::For | ParenKind::Switch
            )
        {
            frame.multiline = true;
        }

        self.push_newline();

        let extra_indent = self.calculate_operator_indent(is_logical_op);
        self.indent_level += extra_indent;
        self.ensure_indent();
        self.indent_level = self.indent_level.saturating_sub(extra_indent);

        self.output.push_str(&token.text);
        self.pending_space = true;
        self.set_prev(token);
    }

    fn calculate_operator_indent(&self, is_logical_op: bool) -> usize {
        let in_condition = self.is_in_condition();

        // Calculate extra indentation:
        // - Base: +1 for continuation line
        // - If we're inside parens deeper than where we started: +1 for each extra level
        let breaking_depth = self.breaking_logical_at_depth.unwrap_or(0);
        let extra_paren_indent =
            if is_logical_op && self.paren_depth > breaking_depth && !in_condition {
                self.paren_depth - breaking_depth
            } else {
                0
            };

        1 + extra_paren_indent
    }

    fn write_operator_default(&mut self, token: &Token) {
        self.prepare_token(token);
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push_str(&token.text);
        self.pending_space = true;
        self.set_prev(token);
    }

    fn write_comment(&mut self, token: &Token) {
        let text = token.text.replace("\r\n", "\n");
        let trimmed_text = text.trim_start();

        // A comment the source put on its own line stays on its own line. The statement
        // before it may have ended on a newline (rather than a ;) in which case nothing
        // has broken the line yet.
        if token.preceded_by_newline && !self.output.is_empty() && !self.output.ends_with('\n') {
            self.push_newline();
        }

        if is_line_comment(trimmed_text) {
            if !self.output.is_empty() && !self.output.ends_with('\n') {
                // Inline comment after code - preserve alignment whitespace
                let spacing: String = token
                    .preceding_whitespace
                    .chars()
                    // Convert tabs to single spaces (tabs in alignment don't make sense)
                    .map(|c| if c == '\t' { ' ' } else { c })
                    .collect();
                // Ensure at least one space if no spacing was preserved
                if spacing.is_empty() {
                    if !matches!(self.output.chars().last(), Some(' ') | Some('\t')) {
                        self.output.push(' ');
                    }
                } else {
                    self.output.push_str(&spacing);
                }
                self.output.push_str(trimmed_text);
                self.push_newline();
            } else {
                // Comment on its own line - use trimmed version to normalize indentation
                self.ensure_indent();
                self.output.push_str(trimmed_text);
                self.push_newline();
            }
            return;
        }

        if text.contains('\n') {
            // Multiline comments should be preserved exactly as written
            for (idx, line) in text.lines().enumerate() {
                if idx > 0 {
                    self.push_newline();
                }
                // Only indent the first line; preserve internal formatting
                if idx == 0 {
                    self.ensure_indent();
                }
                self.output.push_str(line);
            }
            self.push_newline();
            return;
        }

        if self.output.is_empty() || self.output.ends_with('\n') {
            self.ensure_indent();
            self.output.push_str(&text);
            self.set_prev(token);
            return;
        }

        self.prepare_token(token);
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push_str(&text);
        self.set_prev(token);
    }

    fn write_default(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.set_prev(token);
    }

    /// `a -4` sadly lexes the sign into the number, so what the source means as a subtraction
    /// arrives as a single Number token, we use the operand before to tell them apart.
    fn is_binary_signed_number(&self, token: &Token) -> bool {
        token.kind == TokenKind::Number
            && token.text.len() > 1
            && token.text.starts_with(['-', '+'])
            && !is_unary_context(self.prev())
    }

    fn write_signed_number(&mut self, token: &Token) {
        let (sign, magnitude) = token.text.split_at(1);

        self.ensure_indent();
        self.apply_pending_space();
        if !self.ends_with_whitespace() {
            self.output.push(' ');
        }
        self.output.push_str(sign);
        self.output.push(' ');
        self.output.push_str(magnitude);
        self.set_prev(token);
    }

    fn end_statement(&mut self) {
        self.breaking_logical_at_depth = None;
        self.breaking_concat_at_depth = None;
        while self.ternaries.pop().is_some() {
            self.indent_level = self.indent_level.saturating_sub(1);
        }
    }

    fn write_else(&mut self, token: &Token, remaining: &[Token]) {
        // The if body ends here, even when no ';' closed it
        self.close_synthetic_blocks(Some(token));

        self.prepare_token(token);
        self.output.push_str(&token.text);

        // Check if there's an inline comment immediately after else
        let next_is_inline_comment = remaining.first().is_some_and(Self::is_inline_comment);

        // Look ahead past comments to find the next non-comment token
        let next_non_comment = Self::next_non_comment(remaining);
        let next_is_brace = next_non_comment.is_some_and(|t| t.text == "{");
        let next_is_if = next_non_comment.is_some_and(|t| t.text == "if");

        if !next_is_brace && !next_is_if {
            // Auto-insert block for single-statement else
            self.output.push(' ');
            self.output.push('{');
            self.indent_level += 1;
            self.push_newline();

            self.braces.push(BraceContext {
                kind: BraceKind::Block,
                paren_depth_at_open: self.paren_depth,
                bracket_depth_at_open: self.bracket_depth,
                in_case_label: false,
                case_body_indented: false,
                is_synthetic: true,
            });
        } else if !next_is_inline_comment {
            self.output.push(' ');
            self.needs_indent = false;
        }

        self.set_prev(token);
    }

    fn write_case_label(&mut self, token: &Token) {
        // If we were in a case body, dedent before the new case label
        if let Some(frame) = self.braces.last_mut() {
            if frame.case_body_indented {
                self.indent_level = self.indent_level.saturating_sub(1);
                frame.case_body_indented = false;
            }
            // Mark that we're now in a case label (before the colon)
            frame.in_case_label = true;
        }

        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.set_prev(token);
    }

    fn write_blankline(&mut self, next: Option<&Token>) {
        // Blank lines between array elements are dropped as noise, but one that sets off a
        // commented group of elements is what makes the group readable.
        let separates_comment = next.is_some_and(|t| t.kind == TokenKind::Comment);
        if !self.brackets.is_empty() && !separates_comment {
            return;
        }
        if self.output.ends_with("\n\n") {
            return;
        }
        if !self.output.ends_with('\n') {
            self.push_newline();
        }
        self.output.push('\n');
        self.needs_indent = true;
        self.pending_space = false;
        self.prev.clear();
    }

    fn estimate_token_spacing(&self, prev_text: &str, token: &Token) -> usize {
        // No space before closers or punctuation that doesn't take a leading space
        if matches!(token.text.as_str(), "]" | ")" | "}" | "," | "." | "::") {
            return 0;
        }

        // No space right after openers or member access
        if matches!(prev_text, "[" | "(" | "{" | "." | "::") {
            return 0;
        }

        // Space before operator tokens
        if is_operator(&token.text) {
            return 1;
        }

        // Space after comma
        if prev_text == "," {
            return 1;
        }

        // Space after operator
        if is_operator(prev_text) {
            return 1;
        }

        0
    }

    fn estimate_array_length(&self, remaining: &[Token]) -> usize {
        let mut length = 1; // Opening '['
        let mut depth = 0; // Track nested brackets (starts at 0, first ']' we encounter closes our array)
        let mut prev_text = "[";

        for token in remaining {
            // If we hit the closing bracket at depth 0, we're done
            if token.text == "]" && depth == 0 {
                length += 1; // Closing ']'
                break;
            }

            match token.text.as_str() {
                "[" => depth += 1,
                "]" if depth > 0 => {
                    depth -= 1;
                },
                _ => {},
            }

            // Skip blanklines and comments for estimation
            if token.kind == TokenKind::Blankline || token.kind == TokenKind::Comment {
                continue;
            }

            length += token.text.len();
            length += self.estimate_token_spacing(prev_text, token);
            prev_text = &token.text;
        }

        length
    }

    fn get_current_line_length(&self) -> usize {
        // Find the last newline and count visual width (tabs count as 4 spaces)
        let line = self
            .output
            .rsplit_once('\n')
            .map(|(_, after)| after)
            .unwrap_or(&self.output);

        line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
    }

    fn estimate_paren_content_length(&self, remaining: &[Token]) -> usize {
        let mut length = 0;
        let mut depth = 0;
        let mut prev_text = "(";

        for token in remaining {
            // Track paren depth to find the matching closing paren
            match token.text.as_str() {
                "(" => depth += 1,
                ")" => {
                    if depth == 0 {
                        // Found the closing paren for this condition
                        break;
                    }
                    depth -= 1;
                },
                _ => {},
            }

            // Skip blanklines and comments for estimation
            if token.kind == TokenKind::Blankline || token.kind == TokenKind::Comment {
                continue;
            }

            length += token.text.len();
            length += self.estimate_token_spacing(prev_text, token);
            prev_text = &token.text;
        }

        length
    }

    fn estimate_statement_length(&self, remaining: &[Token]) -> usize {
        let mut length = 0;
        let mut prev_text = "";

        for token in statement_slice(remaining) {
            if matches!(token.text.as_str(), ";" | "{" | "}") {
                break;
            }

            if token.kind == TokenKind::Blankline || token.kind == TokenKind::Comment {
                continue;
            }

            length += token.text.len();
            length += self.estimate_token_spacing(prev_text, token);
            prev_text = &token.text;
        }

        length
    }

    fn estimate_ternary_length(&self, remaining: &[Token]) -> usize {
        let mut length = 0;
        let mut prev_text = "?";
        let mut ternary_depth = 0;
        let mut nesting_depth = 0;
        let mut seen_colon = false;

        for token in statement_slice(remaining) {
            match token.text.as_str() {
                "?" => ternary_depth += 1,
                ":" => {
                    if ternary_depth == 0 {
                        if seen_colon {
                            break;
                        }
                        seen_colon = true;
                        length += 2; // ": "
                        prev_text = ":";
                        continue;
                    }
                    ternary_depth -= 1;
                },
                "(" | "[" | "{" => nesting_depth += 1,
                ")" | "]" | "}" => {
                    if nesting_depth == 0 {
                        if seen_colon && ternary_depth == 0 {
                            break;
                        }
                    } else {
                        nesting_depth -= 1;
                    }
                },
                ";" => break,
                "," if nesting_depth == 0 && ternary_depth == 0 && seen_colon => {
                    break;
                },
                _ => {},
            }

            if token.kind == TokenKind::Blankline || token.kind == TokenKind::Comment {
                continue;
            }

            length += token.text.len();
            length += self.estimate_token_spacing(prev_text, token);
            prev_text = &token.text;
        }

        length
    }
}

/// The tokens up to the end of the statement `remaining` starts in. Squirrel (sadly) accepts a
/// newline as a statement separator, so we can't stop only at ';' or we would go into the next
/// statements and measures them too.
fn statement_slice(remaining: &[Token]) -> &[Token] {
    let end = remaining
        .iter()
        .position(|t| t.starts_statement && t.preceded_by_newline)
        .unwrap_or(remaining.len());
    &remaining[..end]
}

/// A comment that runs to the end of its line. Squirrel accepts both forms, and whatever
/// follows one has to be written on the next line or the code ends up commented out.
fn is_line_comment(trimmed_text: &str) -> bool {
    trimmed_text.starts_with("//") || trimmed_text.starts_with('#')
}

fn trim_trailing_whitespace(buffer: &mut String) {
    while matches!(buffer.chars().last(), Some(' ') | Some('\t') | Some('\r')) {
        buffer.pop();
    }
}

fn trim_trailing_whitespace_line(buffer: &mut String) {
    while matches!(buffer.chars().last(), Some(' ') | Some('\t')) {
        buffer.pop();
    }
}

fn collect_tokens(root: Node, source: &str) -> Result<Vec<Token>, FormatError> {
    let mut tokens = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;
    let bytes = source.as_bytes();
    let mut prev_end: usize = 0;

    loop {
        let node = cursor.node();
        // A char literal's contents are not nodes of their own: only its two quotes are. It
        // has to be taken whole, or the character between them is dropped.
        let is_atomic = is_atomic_literal_kind(node.kind());

        if !visited_children && (node.child_count() == 0 || is_atomic) {
            let start = node.start_byte();
            let mut preceded_by_newline = false;
            let mut preceding_whitespace = String::new();
            if start > prev_end {
                preceding_whitespace = source[prev_end..start].to_string();
                let newline_count = preceding_whitespace
                    .chars()
                    .filter(|&ch| ch == '\n')
                    .count();
                preceded_by_newline = newline_count > 0;
                if newline_count >= 2 {
                    tokens.push(Token {
                        text: String::new(),
                        kind: TokenKind::Blankline,
                        preceded_by_newline: true,
                        preceding_whitespace: String::new(),
                        starts_statement: false,
                    });
                }
            }
            let text = node
                .utf8_text(bytes)
                .map_err(|_| FormatError::Utf8)?
                .to_string();
            if !text.is_empty() {
                let kind = if node
                    .parent()
                    .filter(|p| is_string_node_kind(p.kind()))
                    .is_some()
                {
                    TokenKind::String
                } else {
                    classify_token(&node)
                };
                tokens.push(Token {
                    kind,
                    text,
                    preceded_by_newline,
                    preceding_whitespace,
                    starts_statement: starts_statement(node),
                });
            }
            prev_end = node.end_byte();
        }

        if !visited_children && !is_atomic && cursor.goto_first_child() {
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

    Ok(tokens)
}

/// Whether `leaf` is the first token of a statement, of a class member or of an enum entry.
fn starts_statement(leaf: Node) -> bool {
    let mut node = leaf;
    loop {
        let Some(parent) = node.parent() else {
            return false;
        };
        if node.start_byte() != leaf.start_byte() {
            return false;
        }

        let starts_one = match parent.kind() {
            "script" | "block" | "case_statement" | "default_statement" => node.is_named(),
            // The labels of a switch: each one starts the statement list that follows it
            "switch_statement" => matches!(node.kind(), "case_statement" | "default_statement"),
            // A table's slots are separated by a ',' or (sadly) a newline
            "table_slots" => node.is_named(),
            // 'class' and the class name also start here, but only members are statements
            "class_declaration" => node.kind() == "member_declaration",
            // The entries. The enum's own name matches too, but it shares the 'enum' line,
            // so no line break is kept for it.
            "enum_declaration" => node.kind() == "identifier",
            _ => false,
        };
        if starts_one {
            return true;
        }

        node = parent;
    }
}

fn classify_token(node: &Node) -> TokenKind {
    let kind = node.kind();

    if node.is_extra() || kind.contains("comment") {
        return TokenKind::Comment;
    }

    if is_keyword_kind(kind) {
        return TokenKind::Keyword;
    }

    match kind {
        "identifier" => TokenKind::Identifier,
        "number" | "integer" | "float" | "float_literal" | "integer_literal" => TokenKind::Number,
        "string" | "string_literal" | "raw_string" => TokenKind::String,
        _ if node.is_named() => TokenKind::Other,
        _ => TokenKind::Symbol,
    }
}

fn is_string_node_kind(kind: &str) -> bool {
    matches!(kind, "string" | "string_literal" | "raw_string")
}

/// A literal that has to be written out as it was written in the source.
fn is_atomic_literal_kind(kind: &str) -> bool {
    kind == "char"
}

fn is_keyword_kind(kind: &str) -> bool {
    matches!(
        kind,
        "if" | "else"
            | "for"
            | "foreach"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "default"
            | "break"
            | "continue"
            | "return"
            | "local"
            | "class"
            | "enum"
            | "const"
            | "function"
            | "try"
            | "catch"
            | "throw"
            | "static"
            | "yield"
            | "in"
            | "extends"
            | "clone"
            | "typeof"
    )
}

// Only these keywords introduce code blocks directly before a '{'
fn is_block_introducing_keyword(text: &str) -> bool {
    matches!(
        text,
        "if" | "else"
            | "for"
            | "foreach"
            | "while"
            | "switch"
            | "try"
            | "catch"
            | "finally"
            | "do"
            | "class"
            | "enum"
            | "function"
    )
}

fn needs_space(prev: Option<&PrevToken>, current: &Token) -> bool {
    let prev = match prev {
        Some(prev) => prev,
        None => return false,
    };

    let prev_text = prev.text.as_str();
    let curr_text = current.text.as_str();

    // Never insert spaces inside or around parts of string literals
    if matches!(prev.kind, TokenKind::String)
        || (matches!(current.kind, TokenKind::String) && prev.kind != TokenKind::Keyword)
    {
        return false;
    }

    if matches!(prev_text, "(" | "[" | "{" | "." | "::") {
        return false;
    }

    if matches!(curr_text, ")" | "]" | "," | ";") {
        return false;
    }

    if curr_text == "::" {
        return true;
    }

    if curr_text == "." {
        return prev.kind == TokenKind::Keyword;
    }

    if curr_text == "(" {
        return keyword_requires_space_before_paren(prev_text);
    }

    if curr_text == "{" {
        return matches!(
            prev.kind,
            TokenKind::Identifier | TokenKind::Other | TokenKind::Keyword
        ) || prev_text == ")";
    }

    if curr_text == "}" {
        return false;
    }

    if is_operator(curr_text) || is_operator(prev_text) {
        return true;
    }

    if prev.kind == TokenKind::Keyword {
        return true;
    }

    if prev.kind == TokenKind::Identifier
        && matches!(current.kind, TokenKind::Identifier | TokenKind::Keyword)
    {
        return true;
    }

    if matches!(prev.kind, TokenKind::Identifier | TokenKind::Number)
        && current.kind == TokenKind::Number
    {
        return true;
    }

    if current.kind == TokenKind::Comment {
        return true;
    }

    if prev_text == ")" && current.kind == TokenKind::Keyword {
        return true;
    }

    if prev_text == ")" && current.kind == TokenKind::Identifier {
        return true;
    }

    false
}

fn keyword_requires_space_before_paren(text: &str) -> bool {
    matches!(
        text,
        "if" | "for"
            | "foreach"
            | "while"
            | "switch"
            | "catch"
            | "function"
            | "return"
            | "throw"
            | "yield"
    )
}

fn is_operator(text: &str) -> bool {
    matches!(
        text,
        "=" | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "<-"
            | "=="
            | "!="
            | "<"
            | "<="
            | "<=>"
            | ">"
            | ">="
            | "&&"
            | "||"
            | "&"
            | "|"
            | "^"
            | "~"
            | "!"
            | "+="
            | "-="
            | "*="
            | "/="
            | "%="
            | "<<"
            | "<<="
            | ">>"
            | ">>="
            | "|="
            | "&="
            | "^="
            | "in"
            | "instanceof"
    )
}

fn is_unary_operator(text: &str) -> bool {
    matches!(text, "-" | "+" | "!" | "~")
}

fn is_unary_context(prev: Option<&PrevToken>) -> bool {
    match prev {
        None => true,
        Some(prev) => {
            let text = prev.text.as_str();
            matches!(
                text,
                "(" | "["
                    | "{"
                    | ","
                    | ";"
                    | "="
                    | "+="
                    | "-="
                    | "*="
                    | "/="
                    | "%="
                    | "=="
                    | "!="
                    | "<"
                    | "<="
                    | ">"
                    | ">="
                    | "&&"
                    | "||"
                    | "&"
                    | "|"
                    | "^"
                    | "?"
                    | ":"
            ) || is_operator(text)
                || matches!(prev.kind, TokenKind::Keyword)
        },
    }
}
