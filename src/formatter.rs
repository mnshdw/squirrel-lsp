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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BraceKind {
    ObjectInline,
    ObjectMultiline,
    Block,
    Switch,
}

impl BraceKind {
    fn is_object(self) -> bool {
        matches!(self, BraceKind::ObjectInline | BraceKind::ObjectMultiline)
    }

    fn is_inline(self) -> bool {
        matches!(self, BraceKind::ObjectInline)
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
    // Be tolerant of parse errors: many modded Squirrel files (e.g., Battle Brothers mods)
    // use a more lenient syntax than the official grammar. Tree-sitter still produces a
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
    prev_was_unary: bool,
    prev: Option<PrevToken>,
    braces: Vec<BraceContext>,
    parens: Vec<ParenContext>,
    brackets: Vec<BracketContext>,
    ternaries: Vec<TernaryContext>,
    // Track the kind of the last closed paren (used to detect switch blocks before '{')
    last_closed_paren_kind: Option<ParenKind>,
    // Track the paren_depth at which we started breaking logical operators
    breaking_logical_at_depth: Option<usize>,
    // Stack of multiline ternary contexts (for nested ternaries)
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
            prev_was_unary: false,
            prev: None,
            braces: Vec::new(),
            parens: Vec::new(),
            brackets: Vec::new(),
            ternaries: Vec::new(),
            last_closed_paren_kind: None,
            breaking_logical_at_depth: None,
        }
    }

    fn finish(mut self) -> String {
        if self.options.trim_trailing_whitespace {
            trim_trailing_whitespace(&mut self.output);
        }
        self.output
    }

    fn total_depth(&self) -> usize {
        self.paren_depth + self.bracket_depth
    }

    fn write_token(&mut self, token: &Token, next: Option<&Token>, remaining: &[Token]) {
        // Handle case/default in switch blocks before other processing
        if self.in_switch_block() && matches!(token.text.as_str(), "case" | "default") {
            self.write_case_label(token);
            return;
        }

        let is_symbol = token.kind == TokenKind::Symbol;
        match token.text.as_str() {
            "{" if is_symbol => self.write_open_brace(token, next),
            "}" if is_symbol => self.write_close_brace(token, next),
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
            _ if token.kind == TokenKind::Blankline => self.write_blankline(),
            _ if token.kind != TokenKind::String && is_operator(token.text.as_str()) => {
                self.write_operator(token, remaining)
            },
            _ => self.write_default(token),
        }
    }

    /// Helper function to find the next non-comment token
    fn next_non_comment(remaining: &[Token]) -> Option<&Token> {
        remaining
            .iter()
            .find(|t| t.kind != TokenKind::Comment && t.kind != TokenKind::Blankline)
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
        self.prev = None;
    }

    fn set_prev(&mut self, token: &Token) {
        self.prev = Some(PrevToken {
            text: token.text.clone(),
            kind: token.kind,
        });
    }

    fn prepare_token(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();
        if !self.prev_was_unary
            && needs_space(self.prev.as_ref(), token)
            && !self.ends_with_whitespace()
        {
            self.output.push(' ');
        }
        // Only suppress a single post-unary space
        if self.prev_was_unary {
            self.prev_was_unary = false;
        }
    }

    fn write_open_brace(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);

        // Determine brace kind (object literal vs code block, inline vs multiline)
        // Check if the previous closing paren was for a switch statement
        let is_switch = matches!(self.last_closed_paren_kind, Some(ParenKind::Switch));
        let is_block = self
            .prev
            .as_ref()
            .is_some_and(|p| p.text == ")" || is_block_introducing_keyword(p.text.as_str()));

        let kind = if is_switch {
            BraceKind::Switch
        } else if is_block {
            BraceKind::Block
        } else if matches!(next.map(|n| n.text.as_str()), Some("}")) {
            // Empty objects {} are always inline
            BraceKind::ObjectInline
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
                ")" | ";" | "," => {
                    self.needs_indent = false;
                    return;
                },
                "else" | "catch" | "finally" | "while" => {
                    self.output.push(' ');
                    self.needs_indent = false;
                    self.prev = None;
                    return;
                },
                _ if next_token.kind == TokenKind::Comment
                    && next_token.text.trim_start().starts_with("//") =>
                {
                    self.output.push(' ');
                    self.needs_indent = false;
                    return;
                },
                _ => {},
            }
        }

        if !inline {
            self.push_newline();
        }
    }

    fn write_semicolon(&mut self, token: &Token, next: Option<&Token>) {
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(';');

        // Reset logical operator breaking state (statement ended)
        self.breaking_logical_at_depth = None;

        // If we're in a multiline ternary, dedent back
        if !self.ternaries.is_empty() {
            self.indent_level = self.indent_level.saturating_sub(1);
            self.ternaries.pop();
        }

        // If a line comment follows on the same line (not preceded by newline), keep it on the same line
        let next_is_same_line_comment = next
            .filter(|t| {
                t.kind == TokenKind::Comment
                    && t.text.trim_start().starts_with("//")
                    && !t.preceded_by_newline
            })
            .is_some();

        if next_is_same_line_comment {
            if !matches!(self.output.chars().last(), Some(' ') | Some('\t')) {
                self.output.push(' ');
            }
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
        if self.braces.last().is_some_and(|b| b.is_synthetic) {
            let synthetic = Token {
                text: "}".to_string(),
                kind: TokenKind::Symbol,
                preceded_by_newline: false,
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

        // Skip trailing commas in objects (but allow them in arrays)
        let is_trailing = matches!(next.map(|t| t.text.as_str()), Some("}"));
        if is_trailing && in_object_top_level && !in_function_params {
            self.push_newline();
            self.set_prev(token);
            return;
        }

        self.output.push(',');

        if in_object_top_level && !in_function_params {
            match next {
                Some(t) if t.text.as_str() == "function" => self.write_blankline(),
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

        let kind = match self.prev.as_ref().map(|p| p.text.as_str()) {
            Some("for") => ParenKind::For,
            Some("if") => ParenKind::If,
            Some("switch") => ParenKind::Switch,
            Some("function") => ParenKind::Function,
            _ => ParenKind::Regular,
        };

        // Determine if we should start multiline after this paren based on original layout.
        // We preserve an existing newline immediately after '(' for both function calls
        // and control-flow conditions (if/for/switch), excluding trivial closers.
        // For function calls, also exclude cases where first arg is array/object (they manage their own formatting).
        let next_breaks_line = remaining
            .first()
            .is_some_and(|t| t.preceded_by_newline && !matches!(t.text.as_str(), ")" | "[" | "{"));
        let should_multiline = match kind {
            ParenKind::Regular => next_breaks_line,
            ParenKind::If | ParenKind::For | ParenKind::Switch => next_breaks_line,
            ParenKind::Function => false,
        };

        self.parens.push(ParenContext {
            kind,
            bracket_depth_at_open: self.bracket_depth,
            multiline: should_multiline,
        });

        if should_multiline {
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

        // Track the paren kind for the next open brace (e.g., to detect switch blocks)
        self.last_closed_paren_kind = frame_kind;

        if was_multiline {
            // For multiline function calls, close paren on its own line based on prior indent
            if matches!(frame_kind, Some(ParenKind::Regular)) {
                self.indent_level = self.indent_level.saturating_sub(1);
                if !self.output.ends_with('\n') {
                    self.push_newline();
                }
            }
        }

        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(')');

        // Look ahead past comments to find the next non-comment token
        let next_non_comment = Self::next_non_comment(remaining);
        let next_is_brace = next_non_comment.is_some_and(|t| t.text == "{");
        if next_is_brace {
            // Place opening brace on a new line if the condition was multiline
            if was_multiline {
                self.push_newline();
            } else {
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
        let is_subscript = self.prev.as_ref().is_some_and(|p| {
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
        let user_pref = remaining
            .first()
            .is_some_and(|t| t.preceded_by_newline && !matches!(t.text.as_str(), "]"));

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
            self.push_newline();
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

        let keep_space = self.prev.as_ref().is_some_and(|p| {
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
        let prev_is_question = self.prev.as_ref().is_some_and(|p| p.text.as_str() == "?");
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
        if is_unary_operator(token.text.as_str()) && is_unary_context(self.prev.as_ref()) {
            self.write_unary_operator(token);
            return;
        }

        let is_logical_op = matches!(token.text.as_str(), "&&" | "||");
        let is_binary_op = matches!(
            token.text.as_str(),
            "+" | "-" | "*" | "/" | "==" | "!=" | "<" | "<=" | ">" | ">="
        );

        // Logical operators can break anywhere when lines are too long
        // Other binary operators only break at top level (paren_depth == 0)
        let can_break = is_logical_op || self.paren_depth == 0;

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
        // Mark that the previous token was a unary operator so the next token doesn't
        // get a space inserted after it.
        self.prev_was_unary = true;
        self.set_prev(token);
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

        // If the condition is already multiline (either because we broke earlier by width
        // or because the input had a newline after '('), force breaking before each top-level
        // logical operator to keep one operand per line.
        let should_break = if is_logical_op
            && in_condition
            && at_condition_top_level
            && self.parens.last().is_some_and(|f| f.multiline)
        {
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
            line_length + 1 + token.text.len() > self.options.max_width
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

    fn write_operator_with_line_break(&mut self, token: &Token, is_logical_op: bool) {
        // Mark that we're breaking logical operators at this depth
        if is_logical_op && self.breaking_logical_at_depth.is_none() {
            self.breaking_logical_at_depth = Some(self.paren_depth);
        }

        // If we break inside an if/for/switch, remember it became multiline
        if self.paren_depth > 0
            && let Some(frame) = self.parens.last_mut()
        {
            match frame.kind {
                ParenKind::If | ParenKind::For | ParenKind::Switch => {
                    frame.multiline = true;
                },
                _ => {},
            }
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

        if trimmed_text.starts_with("//") {
            if !self.output.is_empty() && !self.output.ends_with('\n') {
                // Inline comment after code - use trimmed version
                if !matches!(self.output.chars().last(), Some(' ') | Some('\t')) {
                    self.output.push(' ');
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

        self.prepare_token(token);
        if !self.output.ends_with(' ') && !self.output.ends_with('\n') {
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

    fn write_else(&mut self, token: &Token, remaining: &[Token]) {
        self.prepare_token(token);
        self.output.push_str(&token.text);

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
        } else {
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

    fn write_blankline(&mut self) {
        // Don't add blank lines in switch blocks
        if self.in_switch_block() {
            return;
        }
        // Skip blank lines inside array literals
        if !self.brackets.is_empty() {
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
        self.prev = None;
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
                "]" => {
                    if depth > 0 {
                        depth -= 1;
                    }
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

        for token in remaining {
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

        for token in remaining {
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
                "," => {
                    if nesting_depth == 0 && ternary_depth == 0 && seen_colon {
                        break;
                    }
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

        if !visited_children && node.child_count() == 0 {
            let start = node.start_byte();
            let mut preceded_by_newline = false;
            if start > prev_end {
                let newline_count = source[prev_end..start]
                    .chars()
                    .filter(|&ch| ch == '\n')
                    .count();
                preceded_by_newline = newline_count > 0;
                if newline_count >= 2 {
                    tokens.push(Token {
                        text: String::new(),
                        kind: TokenKind::Blankline,
                        preceded_by_newline: true,
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
                });
            }
            prev_end = node.end_byte();
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

    Ok(tokens)
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

    if matches!(curr_text, "." | "::") {
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

    if prev.kind == TokenKind::Identifier && current.kind == TokenKind::Identifier {
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

    false
}

fn keyword_requires_space_before_paren(text: &str) -> bool {
    matches!(
        text,
        "if" | "for" | "foreach" | "while" | "switch" | "catch"
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
