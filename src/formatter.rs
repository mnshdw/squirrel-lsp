use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
pub struct FormatOptions {
    pub indent_style: IndentStyle,
    pub insert_final_newline: bool,
    pub trim_trailing_whitespace: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent_style: IndentStyle::Spaces(4),
            insert_final_newline: true,
            trim_trailing_whitespace: true,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BraceContext {
    Object,
    Block,
}

#[derive(Clone, Copy)]
struct BraceFrame {
    context: BraceContext,
    inline: bool,
    paren_depth_at_open: usize,
    bracket_depth_at_open: usize,
}

pub fn format_document(source: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let mut parser = Parser::new();
    parser.set_language(tree_sitter_squirrel::language())?;

    let tree = parser.parse(source, None).ok_or(FormatError::ParseError)?;
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
    prev: Option<Token>,
    braces: Vec<BraceFrame>,
    paren_stack: Vec<bool>,
    // Tracks whether a paren belongs to an `if` header
    if_stack: Vec<bool>,
    // Track auto-inserted blocks (for single-statement ifs) to close on ';'
    auto_brace_stack: Vec<bool>,
    // Tracks whether a paren belongs to a `function` parameter list
    func_paren_stack: Vec<bool>,
    // Tracks whether we increased indentation after an array '[' for pretty-printing
    bracket_indent_bump_stack: Vec<bool>,
    // Tracks the bracket_depth when each paren was opened
    paren_bracket_depth_stack: Vec<usize>,
    // Tracks the output position where each '[' was written
    array_start_indices: Vec<usize>,
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
            paren_stack: Vec::new(),
            if_stack: Vec::new(),
            auto_brace_stack: Vec::new(),
            func_paren_stack: Vec::new(),
            bracket_indent_bump_stack: Vec::new(),
            paren_bracket_depth_stack: Vec::new(),
            array_start_indices: Vec::new(),
        }
    }

    fn finish(mut self) -> String {
        if self.options.trim_trailing_whitespace {
            trim_trailing_whitespace(&mut self.output);
        }
        self.output
    }

    fn write_token(&mut self, token: &Token, next: Option<&Token>, remaining: &[Token]) {
        match token.text.as_str() {
            "{" => self.write_open_brace(token, next),
            "}" => self.write_close_brace(token, next),
            ";" => self.write_semicolon(token, next),
            "," => self.write_comma(token, next),
            "(" => self.write_open_paren(token),
            ")" => self.write_close_paren(token, next),
            "[" => self.write_open_bracket(token, next, remaining),
            "]" => self.write_close_bracket(token),
            "." | "::" => self.write_member_access(token),
            "?" => self.write_question(token),
            ":" => self.write_colon(token, next),
            "++" | "--" => self.write_increment(token),
            _ if token.kind == TokenKind::Comment => self.write_comment(token),
            _ if token.kind == TokenKind::Blankline => self.write_blankline(),
            _ if is_operator(token.text.as_str()) => self.write_operator(token),
            _ => self.write_default(token),
        }
    }

    fn ensure_indent(&mut self) {
        if self.needs_indent {
            self.options
                .push_indent(&mut self.output, self.indent_level);
            self.needs_indent = false;
        }
    }

    fn apply_pending_space(&mut self) {
        if self.pending_space
            && !matches!(
                self.output.chars().last(),
                Some(' ') | Some('\n') | Some('\t')
            )
        {
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

    fn prepare_token(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();
        if !self.prev_was_unary
            && needs_space(self.prev.as_ref(), token)
            && !matches!(
                self.output.chars().last(),
                Some(' ') | Some('\n') | Some('\t')
            )
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
        // Determine brace context (object literal vs code block)
        let is_block = match self.prev.as_ref() {
            Some(p) => p.text == ")" || matches!(p.kind, TokenKind::Keyword),
            None => false,
        };
        let context = if is_block {
            BraceContext::Block
        } else {
            BraceContext::Object
        };

        let inline =
            matches!(next.map(|n| n.text.as_str()), Some("}")) && context == BraceContext::Object;

        // Write '{' first
        self.output.push('{');

        // Push frame and manage indentation/newline
        self.braces.push(BraceFrame {
            context,
            inline,
            paren_depth_at_open: self.paren_depth,
            bracket_depth_at_open: self.bracket_depth,
        });
        if inline {
            // Keep {} inline (no indent or newline)
            self.prev = Some(token.clone());
            return;
        }

        self.indent_level += 1;
        self.push_newline();
    }

    fn write_close_brace(&mut self, token: &Token, next: Option<&Token>) {
        let frame = self.braces.pop();
        let inline = frame.map(|f| f.inline).unwrap_or(false);
        let is_object = frame
            .map(|f| f.context == BraceContext::Object)
            .unwrap_or(false);

        if self.indent_level > 0 && !inline {
            self.indent_level -= 1;
        }
        if !self.output.ends_with('\n') && !inline {
            self.push_newline();
        }

        // Extra indent for objects in non-pretty-printed arrays (aligns } with ])
        let next_is_bracket = matches!(next.map(|t| t.text.as_str()), Some("]"));
        let in_pretty_array = self
            .bracket_indent_bump_stack
            .last()
            .copied()
            .unwrap_or(false);
        let needs_array_indent = !inline && next_is_bracket && !in_pretty_array && is_object;

        if needs_array_indent {
            self.indent_level += 1;
        }

        // Special case: object literal followed by ')' - add blank line
        if is_object && !inline && matches!(next.map(|t| t.text.as_str()), Some(")")) {
            self.push_newline();
            self.write_blankline();
            self.ensure_indent();
            self.output.push('}');
            if needs_array_indent && self.indent_level > 0 {
                self.indent_level -= 1;
            }
            self.prev = Some(token.clone());
            self.needs_indent = false;
            return;
        }

        // Default: emit closing brace
        self.ensure_indent();
        self.output.push('}');
        if needs_array_indent && self.indent_level > 0 {
            self.indent_level -= 1;
        }
        self.prev = Some(token.clone());

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
            self.prev = Some(token.clone());
            return;
        }

        let in_for_header =
            self.paren_depth > 0 && self.paren_stack.last().copied().unwrap_or(false);
        if in_for_header {
            self.output.push(' ');
            self.prev = Some(token.clone());
        } else {
            self.push_newline();
        }

        // If we auto-opened a block for a single-statement if, close it now
        if self.auto_brace_stack.last().copied().unwrap_or(false) {
            self.auto_brace_stack.pop();
            let synthetic = Token {
                text: "}".to_string(),
                kind: TokenKind::Symbol,
                preceded_by_newline: false,
            };
            self.write_close_brace(&synthetic, None);
        }
    }

    fn write_comma(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);

        let in_object_top_level = self.braces.last().is_some_and(|f| {
            f.context == BraceContext::Object
                && !f.inline
                && f.paren_depth_at_open == self.paren_depth
                && f.bracket_depth_at_open == self.bracket_depth
        });
        let in_function_params =
            self.func_paren_stack.last().copied().unwrap_or(false) && self.paren_depth > 0;

        // Apply pretty-array formatting if we're in a pretty-printed array
        // and the bracket depth has increased since any containing paren was opened
        let paren_bracket_depth = self
            .paren_bracket_depth_stack
            .last()
            .copied()
            .unwrap_or(0);
        let bracket_opened_in_paren = self.paren_depth > 0 && self.bracket_depth > paren_bracket_depth;
        let in_pretty_array = self
            .bracket_indent_bump_stack
            .last()
            .copied()
            .unwrap_or(false)
            && (self.paren_depth == 0 || bracket_opened_in_paren);

        // Skip trailing commas in objects (but allow them in arrays)
        let is_trailing = matches!(next.map(|t| t.text.as_str()), Some("}"));
        if is_trailing && in_object_top_level && !in_function_params {
            self.push_newline();
            self.prev = Some(token.clone());
            return;
        }

        self.output.push(',');

        if in_object_top_level && !in_function_params {
            match next {
                Some(t) if t.text.as_str() == "function" => self.write_blankline(),
                _ => self.push_newline(),
            }
        } else if in_pretty_array {
            // In a pretty-printed array, commas should create newlines
            self.push_newline();
        } else {
            let should_space = next.is_none_or(|t| !matches!(t.text.as_str(), ")" | "]" | "}"));
            if should_space {
                self.output.push(' ');
            }
        }
        self.prev = Some(token.clone());
    }

    fn write_open_paren(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push('(');
        self.paren_depth += 1;

        let prev_text = self.prev.as_ref().map(|p| p.text.as_str());
        self.paren_stack.push(prev_text == Some("for"));
        self.if_stack.push(prev_text == Some("if"));
        self.func_paren_stack.push(prev_text == Some("function"));
        self.paren_bracket_depth_stack.push(self.bracket_depth);
        self.prev = Some(token.clone());
    }

    fn write_close_paren(&mut self, token: &Token, next: Option<&Token>) {
        if self.paren_depth > 0 {
            self.paren_depth -= 1;
        }
        self.paren_stack.pop();
        self.func_paren_stack.pop();
        self.paren_bracket_depth_stack.pop();
        let is_if_header = self.if_stack.pop().unwrap_or(false);

        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(')');

        let next_is_brace = next.is_some_and(|t| t.text == "{");
        if next_is_brace {
            self.output.push(' ');
            self.needs_indent = false;
        } else if is_if_header {
            // Auto-insert a block for single-statement ifs
            self.output.push(' ');
            let synthetic = Token {
                text: "{".to_string(),
                kind: TokenKind::Symbol,
                preceded_by_newline: false,
            };
            self.write_open_brace(&synthetic, next);
            self.auto_brace_stack.push(true);
        }
        self.prev = Some(token.clone());
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
            self.bracket_indent_bump_stack.push(false);
            self.prev = Some(token.clone());
            self.array_start_indices.push(self.output.len());
            return;
        }

        // Enable pretty-printing for arrays of objects/arrays, or inherit from pretty-printed parent
        let next_is_complex = matches!(next.map(|n| n.text.as_str()), Some("{") | Some("["));
        let parent_is_pretty = self
            .bracket_indent_bump_stack
            .last()
            .copied()
            .unwrap_or(false);

        // Estimate if array content would exceed 100 chars on one line
        let estimated_length = self.estimate_array_length(remaining);
        let would_be_too_long = estimated_length > 100;

        let should_pretty_print = next_is_complex || parent_is_pretty || would_be_too_long;

        if should_pretty_print {
            self.push_newline();
            self.indent_level += 1;
            self.bracket_indent_bump_stack.push(true);
        } else {
            self.bracket_indent_bump_stack.push(false);
        }
        self.prev = Some(token.clone());
        self.array_start_indices.push(self.output.len());
    }

    fn write_close_bracket(&mut self, token: &Token) {
        if self.bracket_depth > 0 {
            self.bracket_depth -= 1;
        }

        // Pop the array start position (we don't use it anymore but need to keep stack in sync)
        self.array_start_indices.pop();

        let was_pretty = self.bracket_indent_bump_stack.pop().unwrap_or(false);
        if was_pretty {
            if self.indent_level > 0 {
                self.indent_level -= 1;
            }
            // Add newline before ] for pretty-printed arrays, but only if we're not
            // already on a newline and if the previous token suggests we should (e.g., after array element, not in middle of expression)
            if !self.output.ends_with('\n') {
                let parent_is_pretty = self
                    .bracket_indent_bump_stack
                    .last()
                    .copied()
                    .unwrap_or(false);
                let prev_is_closing = self
                    .prev
                    .as_ref()
                    .is_some_and(|p| matches!(p.text.as_str(), "]" | "}"));
                // Also add newline if prev is identifier, number, string (typical array elements)
                let prev_is_value = self.prev.as_ref().is_some_and(|p| {
                    matches!(
                        p.kind,
                        TokenKind::Identifier | TokenKind::Number | TokenKind::String
                    )
                });
                if parent_is_pretty || prev_is_closing || prev_is_value {
                    self.push_newline();
                }
            }
        }
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(']');
        self.prev = Some(token.clone());
    }

    fn write_member_access(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();

        let prev_text = self.prev.as_ref().map(|p| p.text.as_str());
        let keep_space = prev_text.is_some_and(|t| is_operator(t) || t == ",");
        if self.output.ends_with(' ') && !keep_space {
            self.output.pop();
        }
        self.output.push_str(&token.text);
        self.prev = Some(token.clone());
    }

    fn write_question(&mut self, token: &Token) {
        self.prepare_token(token);
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push('?');
        self.output.push(' ');
        self.prev = Some(token.clone());
    }

    fn write_colon(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);

        // Detect if this is a ternary colon by checking if we're in expression context
        // (parens or brackets) vs object literal context
        let is_ternary = self.paren_depth > 0 || self.bracket_depth > 0;

        // For ternary operators, ensure space before colon
        if is_ternary && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        // For object literals, remove space before colon
        else if !is_ternary && self.output.ends_with(' ') {
            self.output.pop();
        }

        self.output.push(':');

        let should_space = !matches!(next.map(|t| t.text.as_str()), Some("}" | "," | ";"));
        if should_space {
            self.output.push(' ');
        }
        self.prev = Some(token.clone());
    }

    fn write_increment(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.prev = Some(token.clone());
    }

    fn write_operator(&mut self, token: &Token) {
        if is_unary_operator(token.text.as_str()) && is_unary_context(self.prev.as_ref()) {
            self.prepare_token(token);
            self.output.push_str(&token.text);
            // Mark that the previous token was a unary operator so the next token doesn't
            // get a space inserted after it.
            self.prev_was_unary = true;
            self.prev = Some(token.clone());
            return;
        }

        self.prepare_token(token);
        if !self.output.ends_with(' ') {
            self.output.push(' ');
        }
        self.output.push_str(&token.text);
        self.pending_space = true;
        self.prev = Some(token.clone());
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
            for (idx, line) in text.lines().enumerate() {
                if idx > 0 {
                    self.push_newline();
                }
                self.ensure_indent();
                self.output.push_str(line.trim_start());
            }
            self.push_newline();
            return;
        }

        self.prepare_token(token);
        if !self.output.ends_with(' ') && !self.output.ends_with('\n') {
            self.output.push(' ');
        }
        self.output.push_str(&text);
        self.prev = Some(token.clone());
    }

    fn write_default(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.prev = Some(token.clone());
    }

    fn write_blankline(&mut self) {
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
                }
                _ => {}
            }

            // Skip blanklines and comments for estimation
            if token.kind == TokenKind::Blankline || token.kind == TokenKind::Comment {
                continue;
            }

            // Estimate token length
            length += token.text.len();

            // Add space for separators
            if token.text == "," {
                length += 1; // space after comma
            } else if !matches!(prev_text, "[" | "(" | "{" | "." | "::")
                && !matches!(token.text.as_str(), "]" | ")" | "}" | "," | "." | "::")
            {
                length += 1; // potential space between tokens
            }

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
            tokens.push(Token {
                kind: classify_token(&node),
                text,
                preceded_by_newline,
            });
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
    )
}

fn needs_space(prev: Option<&Token>, current: &Token) -> bool {
    let prev = match prev {
        Some(prev) => prev,
        None => return false,
    };

    let prev_text = prev.text.as_str();
    let curr_text = current.text.as_str();

    if matches!(prev_text, "(" | "[" | "{" | "." | "::") {
        return false;
    }

    if matches!(curr_text, ")" | "]" | "," | ";" | "." | "::") {
        return false;
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

fn is_unary_context(prev: Option<&Token>) -> bool {
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
