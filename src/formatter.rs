use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
pub struct FormatOptions {
    indent_style: IndentStyle,
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
            }
            IndentStyle::Tabs => {
                for _ in 0..level {
                    buffer.push('\t');
                }
            }
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
}

pub fn format_document(source: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let mut parser = Parser::new();
    parser.set_language(tree_sitter_squirrel::language())?;

    let tree = parser.parse(source, None).ok_or(FormatError::ParseError)?;
    let root = tree.root_node();
    if root.has_error() {
        return Err(FormatError::ParseError);
    }

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
    prev: Option<Token>,
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
            prev: None,
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
            "{" => self.write_open_brace(token),
            "}" => self.write_close_brace(token, next),
            ";" => self.write_semicolon(token),
            "," => self.write_comma(token, next),
            "(" => self.write_open_paren(token),
            ")" => self.write_close_paren(token, next),
            "[" => self.write_open_bracket(token),
            "]" => self.write_close_bracket(token),
            "." | "::" => self.write_member_access(token),
            "?" => self.write_question(token),
            ":" => self.write_colon(token, next),
            "++" | "--" => self.write_increment(token),
            _ if token.kind == TokenKind::Comment => self.write_comment(token),
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
        if needs_space(self.prev.as_ref(), token)
            && !matches!(
                self.output.chars().last(),
                Some(' ') | Some('\n') | Some('\t')
            )
        {
            self.output.push(' ');
        }
    }

    fn write_open_brace(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push('{');
        self.indent_level += 1;
        self.push_newline();
    }

    fn write_close_brace(&mut self, token: &Token, next: Option<&Token>) {
        if self.indent_level > 0 {
            self.indent_level -= 1;
        }
        if !self.output.ends_with('\n') {
            self.push_newline();
        }
        self.ensure_indent();
        self.output.push('}');
        self.prev = Some(token.clone());
        if let Some(next_token) = next {
            if matches!(
                next_token.text.as_str(),
                "else" | "catch" | "finally" | "while"
            ) {
                self.output.push(' ');
                self.needs_indent = false;
                self.prev = None;
                return;
            }
            if next_token.kind == TokenKind::Comment
                && next_token.text.trim_start().starts_with("//")
            {
                self.output.push(' ');
                self.needs_indent = false;
                return;
            }
        }
        self.push_newline();
    }

    fn write_semicolon(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(';');
        if self.paren_depth > 0 {
            self.output.push(' ');
            self.prev = Some(token.clone());
        } else {
            self.push_newline();
        }
    }

    fn write_comma(&mut self, token: &Token, next: Option<&Token>) {
        self.prepare_token(token);
        self.output.push(',');
        if let Some(next_token) = next {
            if !matches!(next_token.text.as_str(), ")" | "]" | "}") {
                self.output.push(' ');
            }
        } else {
            self.output.push(' ');
        }
        self.prev = Some(token.clone());
    }

    fn write_open_paren(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push('(');
        self.paren_depth += 1;
        self.prev = Some(token.clone());
    }

    fn write_close_paren(&mut self, token: &Token, next: Option<&Token>) {
        if self.paren_depth > 0 {
            self.paren_depth -= 1;
        }
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(')');
        if let Some(next_token) = next {
            if next_token.text == "{" {
                self.output.push(' ');
                self.needs_indent = false;
            }
        }
        self.prev = Some(token.clone());
    }

    fn write_open_bracket(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push('[');
        self.bracket_depth += 1;
        self.prev = Some(token.clone());
    }

    fn write_close_bracket(&mut self, token: &Token) {
        if self.bracket_depth > 0 {
            self.bracket_depth -= 1;
        }
        self.ensure_indent();
        self.apply_pending_space();
        self.output.push(']');
        self.prev = Some(token.clone());
    }

    fn write_member_access(&mut self, token: &Token) {
        self.ensure_indent();
        self.apply_pending_space();
        if self.output.ends_with(' ') {
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
        if let Some(next_token) = next {
            if !matches!(next_token.text.as_str(), "}" | "," | ";") {
                self.output.push(' ');
            }
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
            self.ensure_indent();
            self.apply_pending_space();
            if !matches!(self.output.chars().last(), Some(' ') | Some('\n')) {
                self.output.push(' ');
            }
            self.output.push_str(&text);
            self.push_newline();
            return;
        }

        if text.contains('\n') {
            for (idx, line) in text.lines().enumerate() {
                if idx == 0 {
                    self.ensure_indent();
                } else {
                    self.push_newline();
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
        self.prev = Some(token.clone());
    }

    fn write_default(&mut self, token: &Token) {
        self.prepare_token(token);
        self.output.push_str(&token.text);
        self.prev = Some(token.clone());
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

    loop {
        let node = cursor.node();

        if !visited_children && node.child_count() == 0 {
            let text = node
                .utf8_text(bytes)
                .map_err(|_| FormatError::Utf8)?
                .to_string();
            tokens.push(Token {
                kind: classify_token(&node),
                text,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_simple_function() {
        let input = r#"
function foo(a,b){
if(a>0){
print("hi");
}//c
else{
print("bye");
}
}
"#;
        let expected = r#"function foo(a, b) {
    if (a > 0) {
        print("hi");
    } //c
    else {
        print("bye");
    }
}
"#;
        let output = format_document(input, &FormatOptions::default()).unwrap();
        assert_eq!(output, expected);
    }

    #[test]
    fn formats_for_loop() {
        let input = "for(local i=0;i<10;i++){print(i);}";

        let expected = "for (local i = 0; i < 10; i++) {\n    print(i);\n}\n";
        let output = format_document(input, &FormatOptions::default()).unwrap();
        assert_eq!(output, expected);
    }
}
