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
        formatter.write_token(token, next);
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
        }
    }

    fn finish(mut self) -> String {
        if self.options.trim_trailing_whitespace {
            trim_trailing_whitespace(&mut self.output);
        }
        self.output
    }

    fn write_token(&mut self, token: &Token, next: Option<&Token>) {
        match token.text.as_str() {
            "{" => self.write_open_brace(token, next),
            "}" => self.write_close_brace(token, next),
            ";" => self.write_semicolon(token, next),
            "," => self.write_comma(token, next),
            "(" => self.write_open_paren(token),
            ")" => self.write_close_paren(token, next),
            "[" => self.write_open_bracket(token, next),
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
        let is_object = frame.map(|f| f.context == BraceContext::Object).unwrap_or(false);

        if self.indent_level > 0 && !inline {
            self.indent_level -= 1;
        }
        if !self.output.ends_with('\n') && !inline {
            self.push_newline();
        }

        // Check if we need extra indent for array-of-objects alignment
        let next_is_bracket = matches!(next.map(|t| t.text.as_str()), Some("]"));
        let needs_array_indent = !inline
            && next_is_bracket
            && (self.bracket_indent_bump_stack.last().copied().unwrap_or(false) || is_object);

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
                ")" | ";" => {
                    self.needs_indent = false;
                    return;
                }
                "else" | "catch" | "finally" | "while" => {
                    self.output.push(' ');
                    self.needs_indent = false;
                    self.prev = None;
                    return;
                }
                _ if next_token.kind == TokenKind::Comment
                    && next_token.text.trim_start().starts_with("//") => {
                    self.output.push(' ');
                    self.needs_indent = false;
                    return;
                }
                _ => {}
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

        // If a line comment follows immediately, keep it on the same line
        let next_is_line_comment = next
            .filter(|t| t.kind == TokenKind::Comment && t.text.trim_start().starts_with("//"))
            .is_some();

        if next_is_line_comment {
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
            };
            self.write_close_brace(&synthetic, None);
        }
    }

    fn write_comma(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);
        self.output.push(',');

        let in_object_top_level = self.braces.last().map_or(false, |f| {
            f.context == BraceContext::Object
                && !f.inline
                && f.paren_depth_at_open == self.paren_depth
                && f.bracket_depth_at_open == self.bracket_depth
        });
        let in_function_params =
            self.func_paren_stack.last().copied().unwrap_or(false) && self.paren_depth > 0;

        if in_object_top_level && !in_function_params {
            match next {
                Some(t) if t.text.as_str() == "function" => self.write_blankline(),
                _ => self.push_newline(),
            }
        } else {
            let should_space = next.map_or(true, |t| !matches!(t.text.as_str(), ")" | "]" | "}"));
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
        self.prev = Some(token.clone());
    }

    fn write_close_paren(&mut self, token: &Token, next: Option<&Token>) {
        if self.paren_depth > 0 {
            self.paren_depth -= 1;
        }
        self.paren_stack.pop();
        self.func_paren_stack.pop();
        let is_if_header = self.if_stack.pop().unwrap_or(false);

        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(')');

        let next_is_brace = next.map_or(false, |t| t.text == "{");
        if next_is_brace {
            self.output.push(' ');
            self.needs_indent = false;
        } else if is_if_header {
            // Auto-insert a block for single-statement ifs
            self.output.push(' ');
            let synthetic = Token {
                text: "{".to_string(),
                kind: TokenKind::Symbol,
            };
            self.write_open_brace(&synthetic, next);
            self.auto_brace_stack.push(true);
        }
        self.prev = Some(token.clone());
    }

    fn write_open_bracket(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);
        self.output.push('[');
        self.bracket_depth += 1;

        let next_is_object = matches!(next.map(|n| n.text.as_str()), Some("{"));
        if next_is_object {
            self.push_newline();
            self.indent_level += 1;
            self.bracket_indent_bump_stack.push(true);
        } else {
            self.bracket_indent_bump_stack.push(false);
        }
        self.prev = Some(token.clone());
    }

    fn write_close_bracket(&mut self, token: &Token) {
        if self.bracket_depth > 0 {
            self.bracket_depth -= 1;
        }
        // Remove any indentation bump we added for an array that contains an object
        if self.bracket_indent_bump_stack.pop().unwrap_or(false) && self.indent_level > 0 {
            self.indent_level -= 1;
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
        let keep_space = prev_text.map_or(false, |t| is_operator(t) || t == ",");
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
        self.ensure_indent();
        self.apply_pending_space();
        if self.output.ends_with(' ') {
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

        if text.starts_with("//") {
            if !self.output.ends_with('\n') {
                if !matches!(self.output.chars().last(), Some(' ') | Some('\t')) {
                    self.output.push(' ');
                }
                self.output.push_str(&text);
                self.push_newline();
            } else {
                self.ensure_indent();
                self.output.push_str(&text);
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
            if start > prev_end {
                let newline_count = source[prev_end..start].chars().filter(|&ch| ch == '\n').count();
                if newline_count >= 2 {
                    tokens.push(Token {
                        text: String::new(),
                        kind: TokenKind::Blankline,
                    });
                }
            }
            let text = node.utf8_text(bytes).map_err(|_| FormatError::Utf8)?.to_string();
            tokens.push(Token {
                kind: classify_token(&node),
                text,
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
