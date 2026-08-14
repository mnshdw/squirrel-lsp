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
}

#[derive(Debug, Clone, Copy)]
pub enum IndentStyle {
    Spaces(usize),
    Tabs,
}

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("failed to parse squirrel source")]
    ParseError,
}

const TAB_WIDTH: usize = 4;

/// Where the formatter stands on the page.
#[derive(Clone, Copy)]
struct Shape {
    /// How many levels deep the formatter indents the lines it starts here.
    indent: usize,
    /// How many columns the formatter has already written on this line.
    offset: usize,
    /// When true the formatter ignores `max_width`.
    unbounded: bool,
}

impl Shape {
    fn new(indent: usize, offset: usize) -> Self {
        Self {
            indent,
            offset,
            unbounded: false,
        }
    }

    fn unbounded() -> Self {
        Self {
            indent: 0,
            offset: 0,
            unbounded: true,
        }
    }

    /// Moves one level deeper, to the start of a fresh line.
    fn block(self, ctx: &Ctx) -> Self {
        let indent = self.indent + 1;
        Self {
            indent,
            offset: ctx.indent_width(indent),
            unbounded: self.unbounded,
        }
    }

    /// Keeps `columns` free at the end of the line.
    fn reserve(self, columns: usize) -> Self {
        Self {
            offset: self.offset + columns,
            ..self
        }
    }

    /// Where the formatter stands once it has written `text`.
    /// If `text` broke over lines, the formatter is on the last of them.
    /// That line starts at column zero, so the old offset no longer counts.
    fn after(self, text: &str) -> Self {
        let offset = match text.rsplit_once('\n') {
            Some((_, last)) => display_width(last),
            None => self.offset + display_width(text),
        };
        Self { offset, ..self }
    }

    /// Whether the formatter can write `text` here without passing `max_width`.
    /// Text that already broke over lines never fits, however short its lines are.
    fn fits(self, text: &str, ctx: &Ctx) -> bool {
        if self.unbounded {
            return true;
        }
        !text.contains('\n') && self.offset + display_width(text) <= ctx.options.max_width
    }

    /// The formatter accepts a broken layout when its first line still fits.
    fn first_line_fits(self, text: &str, ctx: &Ctx) -> bool {
        self.fits(text.lines().next().unwrap_or(text), ctx)
    }

    /// The formatter compares two layouts with this.
    /// It keeps the broken one only when breaking actually made it narrower.
    fn widest(self, text: &str) -> usize {
        text.lines()
            .enumerate()
            .map(|(index, line)| {
                let offset = if index == 0 { self.offset } else { 0 };
                offset + display_width(line)
            })
            .max()
            .unwrap_or(0)
    }
}

/// Text the formatter is building, and the column it has reached.
/// Every `put` moves that column along, so a renderer never measures its own output twice.
struct Line<'ctx, 'src> {
    text: String,
    shape: Shape,
    ctx: &'ctx Ctx<'src>,
}

impl<'ctx, 'src> Line<'ctx, 'src> {
    fn new(ctx: &'ctx Ctx<'src>, shape: Shape) -> Self {
        Self {
            text: String::new(),
            shape,
            ctx,
        }
    }

    /// Writes literal text: a keyword, a delimiter, a separator.
    fn put(&mut self, text: &str) -> &mut Self {
        self.shape = self.shape.after(text);
        self.text.push_str(text);
        self
    }

    /// Renders a node into whatever room is left on the line.
    fn node(&mut self, node: Node) -> &mut Self {
        let rendered = render(node, self.ctx, self.shape);
        self.put(&rendered)
    }

    fn opt_node(&mut self, node: Option<Node>) -> &mut Self {
        match node {
            Some(node) => self.node(node),
            None => self,
        }
    }

    fn finish(&mut self) -> String {
        std::mem::take(&mut self.text)
    }
}

struct Ctx<'a> {
    source: &'a str,
    options: &'a FormatOptions,
}

impl<'a> Ctx<'a> {
    /// The result borrows the source, not this `Ctx`, so it outlives the call.
    fn text(&self, node: Node) -> &'a str {
        &self.source[node.byte_range()]
    }

    fn indent_width(&self, level: usize) -> usize {
        match self.options.indent_style {
            IndentStyle::Spaces(width) => level * width,
            IndentStyle::Tabs => level * TAB_WIDTH,
        }
    }

    fn indent(&self, level: usize) -> String {
        match self.options.indent_style {
            IndentStyle::Spaces(width) => " ".repeat(level * width),
            IndentStyle::Tabs => "\t".repeat(level),
        }
    }
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if ch == '\t' { TAB_WIDTH } else { 1 })
        .sum()
}

pub fn format_document(source: &str, options: &FormatOptions) -> Result<String, FormatError> {
    let tree = helpers::parse_squirrel(source).map_err(|_| FormatError::ParseError)?;
    let ctx = Ctx { source, options };

    // Through `render`, so the whole file gets the same guards a single node does
    let mut output = render(tree.root_node(), &ctx, Shape::new(0, 0));

    if options.trim_trailing_whitespace {
        output = output
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
    }
    if options.insert_final_newline && !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

/// Finds the ';' the source put at the end of a statement.
/// It is the next sibling, except after `return`, which holds its own.
/// The formatter wants the node itself, because the gap after a statement starts where the
/// ';' ends.
fn semicolon_after(node: Node) -> Option<Node> {
    fn semicolon(node: Option<Node>) -> Option<Node> {
        node.filter(|node| node.kind() == ";")
    }
    let own_last = node.child(node.child_count().saturating_sub(1));

    semicolon(own_last).or_else(|| semicolon(node.next_sibling()))
}

/// ";" when the source ended this statement with one.
fn semicolon_text(node: Node) -> &'static str {
    semicolon_after(node).map_or("", |_| ";")
}

/// `f() -4` is one subtraction, and the grammar reads it as two statements.
/// The formatter joins them back up when the number starts on the row the call ended on.
/// Returns the number as the source wrote it, sign included.
fn signed_continuation<'a>(
    node: Node,
    ctx: &Ctx<'a>,
    prev_end_row: Option<usize>,
) -> Option<&'a str> {
    if !matches!(node.kind(), "integer" | "float")
        || prev_end_row != Some(node.start_position().row)
    {
        return None;
    }
    let text = ctx.text(node);
    text.starts_with(['-', '+']).then_some(text)
}

/// The spacing the source put in front of a trailing comment.
/// The formatter copies it, so a column of aligned end-of-line comments stays aligned.
fn gap<'a>(ctx: &Ctx<'a>, from: usize, to: usize) -> &'a str {
    // The ',' sits in this span too, and the formatter writes that one itself
    let gap = ctx
        .source
        .get(from..to)
        .unwrap_or(" ")
        .trim_start_matches([',', ';']);

    if gap.is_empty() || !gap.trim().is_empty() || gap.contains('\n') {
        " "
    } else {
        gap
    }
}

fn blank_before(node: Node, prev_end_row: Option<usize>) -> bool {
    prev_end_row.is_some_and(|prev| node.start_position().row > prev + 1)
}

/// Whether the comment sits at the end of the previous node's line.
fn trails(comment: Node, prev_end_row: Option<usize>) -> bool {
    prev_end_row == Some(comment.start_position().row)
}

/// Reads a `// fmt: off` or `// fmt: on` marker.
/// The '*' trimming lets `/* fmt: off */` work too.
fn fmt_marker(ctx: &Ctx, node: Node) -> Option<bool> {
    if node.kind() != "comment" {
        return None;
    }
    let body = ctx
        .text(node)
        .trim_matches(['/', '*'])
        .trim()
        .replace(' ', "");

    match body.as_str() {
        "fmt:off" => Some(false),
        "fmt:on" => Some(true),
        _ => None,
    }
}

/// How far a `// fmt: off` reaches.
fn fmt_region_end(ctx: &Ctx, start: Node) -> usize {
    let mut end = start.end_byte();
    let mut next = start.next_sibling();

    while let Some(node) = next {
        if matches!(node.kind(), "}" | ")" | "]") {
            break;
        }
        end = node.end_byte();
        if fmt_marker(ctx, node) == Some(true) {
            break;
        }
        next = node.next_sibling();
    }
    end
}

/// Writes the children of a `script`, a `block` or a class body, one per line.
fn render_statements(parent: Node, ctx: &Ctx, shape: Shape, keep: impl Fn(Node) -> bool) -> String {
    let indent = ctx.indent(shape.indent);
    let mut out = String::new();
    let mut prev_end_row: Option<usize> = None;
    let mut prev_end_byte = 0;

    // The formatter keeps a blank line the source set right after the '{'.
    // A block opens on its '{', a class a little later, once it has named itself.
    let mut cursor = parent.walk();
    let brace_row = parent
        .children(&mut cursor)
        .find(|child| child.kind() == "{")
        .map(|brace| brace.end_position().row);

    // Everything up to here has been copied out under a '// fmt: off'
    let mut copied_to = 0;

    let mut cursor = parent.walk();
    for child in parent.children(&mut cursor) {
        if child.start_byte() < copied_to {
            continue;
        }
        if !child.is_named() || !keep(child) {
            continue;
        }

        // Between '// fmt: off' and '// fmt: on' the source stands as the author wrote it
        if fmt_marker(ctx, child) == Some(false) {
            let end = fmt_region_end(ctx, child);
            if prev_end_row.is_some() {
                out.push('\n');
                if blank_before(child, prev_end_row) {
                    out.push('\n');
                }
            }
            out.push_str(&indent);
            out.push_str(&ctx.source[child.start_byte()..end]);

            prev_end_row = Some(ctx.source[..end].lines().count().saturating_sub(1));
            prev_end_byte = end;
            copied_to = end;
            continue;
        }

        // The formatter keeps a comment at the end of a statement on that line
        if child.kind() == "comment" && trails(child, prev_end_row) {
            out.push_str(gap(ctx, prev_end_byte, child.start_byte()));
            out.push_str(ctx.text(child));
            prev_end_row = Some(child.end_position().row);
            prev_end_byte = child.end_byte();
            continue;
        }

        // The formatter measures the gap from where the ';' ends, not where the statement does
        let semicolon = semicolon_after(child);

        // 'f() -4' is one subtraction, and the grammar read it as two statements
        if let Some(number) = signed_continuation(child, ctx, prev_end_row) {
            let (sign, digits) = number.split_at(1);
            out.push(' ');
            out.push_str(sign);
            out.push(' ');
            out.push_str(digits);
        } else {
            match prev_end_row {
                Some(prev) => {
                    out.push('\n');
                    // One blank line at most, and only where the source had one too
                    if blank_before(child, Some(prev)) {
                        out.push('\n');
                    }
                },
                None if brace_row.is_some_and(|row| blank_before(child, Some(row))) => {
                    out.push('\n');
                },
                None => {},
            }

            out.push_str(&indent);
            // The ';' the formatter is about to write needs a column of its own
            let room = shape.reserve(usize::from(semicolon.is_some()));
            out.push_str(&render(child, ctx, room));
        }

        if semicolon.is_some() {
            out.push(';');
        }
        prev_end_row = Some(child.end_position().row);
        prev_end_byte = semicolon.map_or(child.end_byte(), |semicolon| semicolon.end_byte());
    }

    out
}

/// `collapse_empty` lets the formatter write an empty body as `{}`.
/// An `if` needs it off, or the `else` ends up on the same line as the closing brace.
fn render_block(node: Node, ctx: &Ctx, shape: Shape, collapse_empty: bool) -> String {
    let inner = render_statements(node, ctx, shape.block(ctx), |_| true);
    if inner.trim().is_empty() {
        return if collapse_empty {
            "{}".to_string()
        } else {
            format!("{{\n{}}}", ctx.indent(shape.indent))
        };
    }
    format!("{{\n{}\n{}}}", inner, ctx.indent(shape.indent))
}

/// Writes the body of an `if`, a loop or a function.
/// The formatter adds braces to a bare statement, so `if (x) y;` comes out `if (x) { y; }`.
fn render_body(node: Node, ctx: &Ctx, shape: Shape) -> String {
    if node.kind() == "block" {
        let block = render_block(node, ctx, shape, false);
        return with_header_comments(node, ctx, shape, block);
    }

    let inner = shape.block(ctx);
    let body = format!(
        "{{\n{}{}{}\n{}}}",
        ctx.indent(inner.indent),
        render(node, ctx, inner),
        semicolon_text(node),
        ctx.indent(shape.indent)
    );
    with_header_comments(node, ctx, shape, body)
}

fn render_if(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(condition) = named_child(node, 0) else {
        return ctx.text(node).to_string();
    };

    let body = named_child(node, 1);
    let newline = format!("\n{}", ctx.indent(shape.indent));

    let mut line = Line::new(ctx, shape);
    line.put("if ");
    let condition = render(condition, ctx, line.shape);
    line.put(&condition);

    // The formatter drops the brace to its own line when the condition needed more than
    // one, as rustfmt does
    if condition.contains('\n') {
        line.put(&newline);
    } else {
        line.put(" ");
    }

    if let Some(body) = body {
        line.put(&render_body(body, ctx, shape));
    }

    let Some(else_part) = named_child(node, 2) else {
        return line.finish();
    };

    // The formatter leaves a comment written between the '}' and the 'else' where it was.
    // It must then start the 'else' on the next line.
    let between = body.map_or_else(Vec::new, |body| comments_between(node, body, else_part));
    if between.is_empty() {
        line.put(" ");
    } else {
        let mut prev_end_row = body.map(|body| body.end_position().row);
        for comment in between {
            line.put(if trails(comment, prev_end_row) {
                " "
            } else {
                &newline
            });
            line.put(ctx.text(comment));
            prev_end_row = Some(comment.end_position().row);
        }
        line.put(&newline);
    }

    line.put(&render(else_part, ctx, shape)).finish()
}

/// The comments the source put between the body of an `if` and its `else`.
fn comments_between<'a>(node: Node<'a>, after: Node, before: Node) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| {
            child.kind() == "comment"
                && child.start_byte() >= after.end_byte()
                && child.start_byte() < before.start_byte()
        })
        .collect()
}

/// The comments the source put immediately before `node`, among its siblings.
fn comments_preceding(node: Node) -> Vec<Node> {
    let mut found = Vec::new();
    let mut previous = node.prev_sibling();

    while let Some(comment) = previous {
        if comment.kind() != "comment" {
            break;
        }
        found.push(comment);
        previous = comment.prev_sibling();
    }

    found.reverse();
    found
}

/// Writes the comments that trailed a header, then the body below them.
/// The formatter has to start the '{' on the next line, because a '//' comment takes the
/// rest of its own.
fn with_header_comments(node: Node, ctx: &Ctx, shape: Shape, body: String) -> String {
    let comments = comments_preceding(node);
    if comments.is_empty() {
        return body;
    }

    let mut out = String::new();
    for comment in comments {
        out.push_str(ctx.text(comment));
        out.push('\n');
        out.push_str(&ctx.indent(shape.indent));
    }
    out.push_str(&body);
    out
}

fn render_else(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(body) = named_child(node, 0) else {
        return "else".to_string();
    };
    if body.kind() == "if_statement" {
        return format!("else {}", render(body, ctx, shape.after("else ")));
    }
    format!("else {}", render_body(body, ctx, shape))
}

/// `while (cond) body`.
/// The formatter writes the parens itself: Squirrel requires them, and the grammar gives
/// them no node.
fn render_while(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(condition) = named_child(node, 0) else {
        return ctx.text(node).to_string();
    };

    let head = "while (";
    let mut out = format!("{head}{})", render(condition, ctx, shape.after(head)));

    if let Some(body) = named_child(node, 1) {
        out.push(' ');
        out.push_str(&render_body(body, ctx, shape));
    }
    out
}

/// `for (init; cond; step) body`.
///
/// Any clause can be empty, as in `for (local i = 0; i < 20;)`.
/// An empty clause has no node, so the formatter rebuilds the header from the ';' tokens.
/// Working from the clauses alone, it would write one ';' where the source had two.
fn render_for(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let parts = named_children(node);
    let Some((body, _)) = parts.split_last() else {
        return ctx.text(node).to_string();
    };

    let head = "for (";
    let at = shape.after(head);

    let mut clauses = vec![String::new()];
    let mut inside = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "(" => inside = true,
            ")" => break,
            ";" if inside => clauses.push(String::new()),
            _ if inside && is_part(&child) => {
                if let Some(clause) = clauses.last_mut() {
                    *clause = render(child, ctx, at);
                }
            },
            _ => {},
        }
    }

    let mut header = String::new();
    for (index, clause) in clauses.iter().enumerate() {
        if index > 0 {
            header.push(';');
            if !clause.is_empty() {
                header.push(' ');
            }
        }
        header.push_str(clause);
    }

    format!("{head}{header}) {}", render_body(*body, ctx, shape))
}

/// `try { ... } catch (e) { ... }`.
fn render_try(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put("try ");

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "block" => {
                let body = render_body(child, ctx, shape);
                line.put(&body);
            },
            "catch_statement" => {
                let handler = render_catch(child, ctx, shape);
                line.put(" ").put(&handler);
            },
            _ => {},
        }
    }

    line.finish()
}

/// `catch (e) { ... }`.
/// The formatter writes the parens itself: the grammar gives them no node.
fn render_catch(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(caught) = named_child(node, 0) else {
        return ctx.text(node).to_string();
    };

    let head = "catch (";
    let mut out = format!("{head}{})", render(caught, ctx, shape.after(head)));

    if let Some(body) = named_child(node, 1) {
        out.push(' ');
        out.push_str(&render_body(body, ctx, shape));
    }
    out
}

/// `enum Name { A = 1, B }`.
///
/// The grammar gives an entry no node of its own: a name, then an optional '=' and value.
/// The formatter rebuilds each entry from the tokens, so it can tell one entry from the next
/// rather than run them all together.
fn render_enum(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let inner = shape.block(ctx);

    let mut head = String::from("enum");
    // An entry, or a comment that gets a line to itself
    let mut entries: Vec<(String, bool)> = Vec::new();
    let mut inside = false;
    let mut awaiting_value = false;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "{" => inside = true,
            "}" => break,
            // The value belongs to the entry the formatter is already writing
            "=" if inside => {
                if let Some((entry, _)) = entries.last_mut() {
                    entry.push_str(" = ");
                    awaiting_value = true;
                }
            },
            // A comment goes inside the braces, wherever the source put it
            "comment" => entries.push((ctx.text(child).to_string(), false)),
            _ if !child.is_named() => {},
            _ if !inside => {
                head.push(' ');
                head.push_str(&render(child, ctx, shape));
            },
            _ if awaiting_value => {
                if let Some((entry, _)) = entries.last_mut() {
                    entry.push_str(&render(child, ctx, inner));
                }
                awaiting_value = false;
            },
            _ => entries.push((render(child, ctx, inner), true)),
        }
    }

    if entries.is_empty() {
        return format!("{head} {{}}");
    }

    // A short enum reads better on one line, as a short table does
    if entries.iter().all(|(_, is_entry)| *is_entry) {
        let joined = entries
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let one_line = format!("{head} {{ {joined} }}");
        if shape.fits(&one_line, ctx) {
            return one_line;
        }
    }

    // The ',' goes after every entry but the last. A trailing one is not always accepted.
    let last_entry = entries.iter().rposition(|(_, is_entry)| *is_entry);
    let indent = ctx.indent(inner.indent);
    let lines: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(index, (text, is_entry))| {
            let comma = if *is_entry && Some(index) != last_entry {
                ","
            } else {
                ""
            };
            format!("{indent}{text}{comma}")
        })
        .collect();

    format!(
        "{head} {{\n{}\n{}}}",
        lines.join("\n"),
        ctx.indent(shape.indent)
    )
}

/// `const NAME = value`.
/// The ';' is the node's own child, so the formatter leaves it to `render_statements`.
fn render_const(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let [name, value, ..] = named_children(node)[..] else {
        return ctx.text(node).to_string();
    };
    Line::new(ctx, shape)
        .put("const ")
        .node(name)
        .put(" = ")
        .node(value)
        .finish()
}

fn render_switch(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(subject) = named_child(node, 0) else {
        return ctx.text(node).to_string();
    };

    let head = "switch (";
    let mut out = format!("{head}{}) {{\n", render(subject, ctx, shape.after(head)));
    let arm_shape = shape.block(ctx);

    let mut cursor = node.walk();
    let mut prev_end_row: Option<usize> = None;
    let mut prev_end_byte = 0;
    for child in node.children(&mut cursor) {
        // A comment between two arms belongs to neither, so the switch places it
        let arm = matches!(child.kind(), "case_statement" | "default_statement");
        if !arm && child.kind() != "comment" {
            continue;
        }

        // The formatter keeps a comment at the end of an arm on that arm's last line
        if !arm && trails(child, prev_end_row) {
            out.push_str(gap(ctx, prev_end_byte, child.start_byte()));
            out.push_str(ctx.text(child));
            prev_end_row = Some(child.end_position().row);
            prev_end_byte = child.end_byte();
            continue;
        }

        if prev_end_row.is_some() {
            out.push('\n');
            // The formatter keeps a blank line the source set between two arms
            if blank_before(child, prev_end_row) {
                out.push('\n');
            }
        }
        prev_end_row = Some(child.end_position().row);
        prev_end_byte = child.end_byte();
        out.push_str(&ctx.indent(arm_shape.indent));
        if arm {
            out.push_str(&render_case(child, ctx, arm_shape));
        } else {
            out.push_str(ctx.text(child));
        }
    }

    out.push('\n');
    out.push_str(&ctx.indent(shape.indent));
    out.push('}');
    out
}

/// Writes a case label, then its statements one level deeper.
/// A case has no braces of its own, so the formatter does the indenting.
fn render_case(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let label = if node.kind() == "default_statement" {
        None
    } else {
        named_child(node, 0)
    };

    let mut out = match label {
        None => "default:".to_string(),
        Some(label) => format!("case {}:", render(label, ctx, shape.after("case "))),
    };

    // The formatter writes everything but the label as an ordinary run of statements
    let label_id = label.map(|label| label.id());
    let body = render_statements(node, ctx, shape.block(ctx), |child| {
        Some(child.id()) != label_id
    });
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(&body);
    }

    out
}

/// `(params) { body }`. Every function form ends this way.
fn put_function_tail(line: &mut Line<'_, '_>, node: Node, shape: Shape) {
    let ctx = line.ctx;
    let mut parameters = None;
    let mut body = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "parameters" => parameters = Some(child),
            "block" => body = Some(child),
            _ => {},
        }
    }

    let items = parameters.map(list_children).unwrap_or_default();
    let params = render_items(&items, &ARGS, ctx, line.shape);
    line.put(&params);

    if let Some(body) = body {
        let block = render_block(body, ctx, shape, true);
        line.put(" ")
            .put(&with_header_comments(body, ctx, shape, block));
    }
}

fn render_function_declaration(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put("function");

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "identifier" | "deref_expression") {
            line.put(" ").node(child);
        }
    }

    put_function_tail(&mut line, node, shape);
    line.finish()
}

fn render_anonymous_function(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put("function ");
    put_function_tail(&mut line, node, shape);
    line.finish()
}

fn render_class(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put("class");

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "extends" => {
                line.put(" extends");
            },
            // The members are written below, as a run of statements.
            // A comment goes down there with them: on this line, '//' would
            // comment out the '{' the formatter is about to write.
            "member_declaration" | "comment" => {},
            _ if child.is_named() => {
                line.put(" ").node(child);
            },
            _ => {},
        }
    }
    line.put(" {");

    let members = render_statements(node, ctx, shape.block(ctx), |child| {
        matches!(child.kind(), "member_declaration" | "comment")
    });
    if members.trim().is_empty() {
        return line.put("}").finish();
    }

    line.put("\n")
        .put(&members)
        .put("\n")
        .put(&ctx.indent(shape.indent))
        .put("}")
        .finish()
}

/// A `name = value` pair. The formatter writes a class member and a table slot alike.
///
/// Four things here are anonymous tokens, so they are not among the named children.
/// A member can be `static`, and a member can be the `constructor`, which has no name
/// of its own. A slot can compute its key, as in `["a.b"] = 1`, and it can separate key
/// from value with ':' instead of '=', as in `"a": 1`.
/// The formatter reads all four off the node, or it drops them.
fn render_slot(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut parts = named_children(node);

    let mut cursor = node.walk();
    let mut is_static = false;
    let mut is_constructor = false;
    let mut computed = false;
    let mut colon = false;
    for child in node.children(&mut cursor) {
        if child.is_named() {
            continue;
        }
        match child.kind() {
            "static" => is_static = true,
            "constructor" => is_constructor = true,
            "[" => computed = true,
            ":" => colon = true,
            _ => {},
        }
    }

    let mut line = Line::new(ctx, shape);

    // A '</ ... />' attribute stands in front of the name, and is not the name itself
    let attributed = parts.first().is_some_and(is_attribute);
    if attributed {
        line.put(ctx.text(parts.remove(0))).put(" ");
    }
    if is_static {
        line.put("static ");
    }

    // The keyword names a constructor, and the rest of it reads as any other function
    if is_constructor {
        line.put("constructor");
        put_function_tail(&mut line, node, shape);
        return line.finish();
    }

    let Some(name) = parts.first() else {
        return ctx.text(node).to_string();
    };
    if computed {
        line.put("[").node(*name).put("]");
    } else {
        line.node(*name);
    }
    if let Some(value) = parts.get(1) {
        line.put(if colon { ": " } else { " = " }).node(*value);
    }

    line.finish()
}

/// The punctuation the formatter uses for one kind of comma-separated list.
#[derive(PartialEq, Eq)]
struct ListStyle {
    open: &'static str,
    close: &'static str,
    /// A space inside the delimiters when the list is on one line: `{ a = 1 }` but `[1, 2]`
    pad: bool,
}

const ARGS: ListStyle = ListStyle {
    open: "(",
    close: ")",
    pad: false,
};

const ARRAY: ListStyle = ListStyle {
    open: "[",
    close: "]",
    pad: false,
};

const TABLE: ListStyle = ListStyle {
    open: "{",
    close: "}",
    pad: true,
};

/// Writes a comma-separated list and its delimiters.
/// The formatter tries three layouts in order and takes the first that fits.
fn render_items(items: &[Node], style: &ListStyle, ctx: &Ctx, shape: Shape) -> String {
    if items.is_empty() {
        return format!("{}{}", style.open, style.close);
    }

    // If a list contains a comment, the formatter must keep it on its line
    if !items.iter().any(|item| item.kind() == "comment") {
        let one_line = items_inline(items, style, ctx);
        if shape.fits(&one_line, ctx) {
            return one_line;
        }

        // When the list will not fit on one line, we choose to keep every item on the line and
        // break only the last one:
        //
        // ```text
        // f(a, b, function () {
        //     ...
        // })
        // ```
        //
        // It's only done when the last item is a table, array or function, and only for arguments,
        // it would be ugly for tables and arrays.
        if *style == ARGS && (items.len() == 1 || last_item_is_block(items)) {
            let packed = items_with_last_broken(items, style, ctx, shape);
            if shape.first_line_fits(&packed, ctx) {
                return packed;
            }
        }
    }

    items_one_per_line(items, style, ctx, shape)
}

/// Writes every item on one line, however long that comes out.
fn items_inline(items: &[Node], style: &ListStyle, ctx: &Ctx) -> String {
    let joined = items
        .iter()
        .map(|item| render_inline(*item, ctx))
        .collect::<Vec<_>>()
        .join(", ");

    if style.pad {
        format!("{} {} {}", style.open, joined, style.close)
    } else {
        format!("{}{}{}", style.open, joined, style.close)
    }
}

/// Writes every item on one line and lets the last one break inside itself.
fn items_with_last_broken(items: &[Node], style: &ListStyle, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put(style.open);
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            line.put(", ");
        }
        line.node(*item);
    }
    line.put(style.close).finish()
}

/// One output line: an item with any comment trailing it, or a comment that had a line to
/// itself.
struct Row {
    text: String,
    trailing: String,
    /// A ',' the source wrote after this item
    comma: bool,
    /// A blank line the source set before this row
    preceded_by_blank: bool,
}

impl Row {
    fn new(text: String, comma: bool, preceded_by_blank: bool) -> Self {
        Self {
            text,
            trailing: String::new(),
            comma,
            preceded_by_blank,
        }
    }
}

/// Returns where there is a ',' after this item.
///
/// A table can have its slots separated with a newline or not, and both are valid, so we keep
/// whatever was in the source chose rather than rewrite one into the other.
fn comma_follows(item: Node) -> bool {
    let mut next = item.next_sibling();
    while let Some(node) = next {
        match node.kind() {
            "," => return true,
            "comment" => next = node.next_sibling(),
            _ => return false,
        }
    }
    false
}

/// A table slot holding a named function, as in `function create() { ... }`.
/// A lambda or an anonymous function does not count, only `function name()`.
fn is_function_slot(node: Node) -> bool {
    node.kind() == "table_slot"
        && named_child(node, 0).is_some_and(|child| child.kind() == "function_declaration")
}

/// Writes one item per line, one level deeper.
fn items_one_per_line(items: &[Node], style: &ListStyle, ctx: &Ctx, shape: Shape) -> String {
    let inner = shape.block(ctx);
    let indent = ctx.indent(inner.indent);

    let mut rows: Vec<Row> = Vec::new();
    let mut prev_end_row: Option<usize> = None;
    let mut prev_end_byte = 0;
    // Where the run of comments sitting directly above the next item starts
    let mut comments_start: Option<usize> = None;

    for item in items {
        let blank = blank_before(*item, prev_end_row);
        match rows.last_mut() {
            // The formatter keeps a comment at the end of an item on that item's line
            Some(row) if item.kind() == "comment" && trails(*item, prev_end_row) => {
                row.trailing
                    .push_str(gap(ctx, prev_end_byte, item.start_byte()));
                row.trailing.push_str(ctx.text(*item));
            },
            _ if item.kind() == "comment" => {
                comments_start.get_or_insert(rows.len());
                rows.push(Row::new(ctx.text(*item).to_string(), false, blank));
            },
            _ => {
                rows.push(Row::new(
                    render(*item, ctx, inner),
                    comma_follows(*item),
                    blank,
                ));

                // The formatter sets a named function off with a blank line, so a table of
                // them reads as a list of methods. That blank belongs above the comments
                // that document the function, never between them and the function itself.
                // Nothing to set off when the comments open the table, so it stays out.
                let start = comments_start.unwrap_or(rows.len() - 1);
                if !blank && start > 0 && is_function_slot(*item) {
                    rows[start].preceded_by_blank = true;
                }
                comments_start = None;
            },
        }
        prev_end_row = Some(item.end_position().row);
        prev_end_byte = item.end_byte();
    }

    // The formatter writes the ',' after the item and before its comment
    let lines: Vec<String> = rows
        .iter()
        .map(|row| {
            let comma = if row.comma { "," } else { "" };
            let blank = if row.preceded_by_blank { "\n" } else { "" };
            format!("{blank}{indent}{}{comma}{}", row.text, row.trailing)
        })
        .collect();

    format!(
        "{}\n{}\n{}{}",
        style.open,
        lines.join("\n"),
        ctx.indent(shape.indent),
        style.close
    )
}

fn last_item_is_block(items: &[Node]) -> bool {
    items
        .iter()
        .rev()
        .find(|item| item.kind() != "comment")
        .is_some_and(|item| {
            matches!(
                item.kind(),
                "table"
                    | "array"
                    | "anonymous_function"
                    | "lambda_expression"
                    | "function_declaration"
            )
        })
}

/// A binary chain, flattened so `a && b && c` is three operands and not two nestings.
/// There is always a first operand, and each later one carries the operator in front of it.
struct Chain<'tree, 'src> {
    head: Node<'tree>,
    rest: Vec<(&'src str, Node<'tree>)>,
}

impl<'tree, 'src> Chain<'tree, 'src> {
    fn leaf(node: Node<'tree>) -> Self {
        Self {
            head: node,
            rest: Vec::new(),
        }
    }

    /// Collects the operands of a run of operators of the same precedence.
    fn flatten(node: Node<'tree>, ctx: &Ctx<'src>) -> Self {
        let Some(operator) = binary_operator(node, ctx) else {
            return Self::leaf(node);
        };
        let (Some(left), Some(right)) = (named_child(node, 0), named_child(node, 1)) else {
            return Self::leaf(node);
        };

        let same_precedence = binary_operator(left, ctx)
            .is_some_and(|left_op| precedence(left_op) == precedence(operator));
        let mut chain = if same_precedence {
            Self::flatten(left, ctx)
        } else {
            Self::leaf(left)
        };

        chain.rest.push((operator, right));
        chain
    }
}

/// The formatter breaks a chain at every operator, or at none of them.
/// That stops a mixed `&&` and `||` from breaking inconsistently.
fn render_binary(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let chain = Chain::flatten(node, ctx);

    // Built from the operands, never from this node.
    // Asking whether this node fits would put the formatter back where it started, forever.
    let mut one_line = render_inline(chain.head, ctx);
    for (operator, operand) in &chain.rest {
        one_line.push(' ');
        one_line.push_str(operator);
        one_line.push(' ');
        one_line.push_str(&render_inline(*operand, ctx));
    }
    if shape.fits(&one_line, ctx) {
        return one_line;
    }

    let inner = shape.block(ctx);
    let indent = ctx.indent(inner.indent);
    let mut out = render(chain.head, ctx, shape);

    for (operator, operand) in &chain.rest {
        let head = format!("{operator} ");
        out.push('\n');
        out.push_str(&indent);
        out.push_str(&head);
        out.push_str(&render(*operand, ctx, inner.after(&head)));
    }

    if shape.widest(&out) >= shape.widest(&one_line) {
        return one_line;
    }
    out
}

fn binary_operator<'a>(node: Node, ctx: &Ctx<'a>) -> Option<&'a str> {
    if node.kind() != "binary_expression" {
        return None;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| !child.is_named())
        .map(|child| ctx.text(child))
}

fn precedence(operator: &str) -> u8 {
    match operator {
        "||" => 1,
        "&&" => 2,
        "|" => 3,
        "^" => 4,
        "&" => 5,
        "==" | "!=" | "<=>" => 6,
        "<" | ">" | "<=" | ">=" => 7,
        "<<" | ">>" | ">>>" => 8,
        "+" | "-" => 9,
        "*" | "/" | "%" => 10,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Renders a node on a single line, however long that line comes out.
fn render_inline(node: Node, ctx: &Ctx) -> String {
    render(node, ctx, Shape::unbounded())
}

fn render(node: Node, ctx: &Ctx, shape: Shape) -> String {
    // The parser invents tokens a half-typed file is missing, such as an unclosed table's '}',
    // and it gives up outright on source it cannot fit the grammar.
    // The formatter would then write code nobody typed, so it copies the source instead.
    if parser_gave_up(node) {
        return ctx.text(node).to_string();
    }

    let rendered = dispatch(node, ctx, shape);

    // A renderer takes its node apart, and some have nowhere to put a comment the source
    // wedged inside. Copying the node keeps that comment, and keeps a '//' from swallowing
    // the code the formatter would have written after it.
    if drops_a_comment(node, ctx, &rendered) {
        return ctx.text(node).to_string();
    }

    rendered
}

fn dispatch(node: Node, ctx: &Ctx, shape: Shape) -> String {
    match node.kind() {
        "script" => render_statements(node, ctx, shape, |_| true),
        "block" => render_block(node, ctx, shape, true),

        "if_statement" => render_if(node, ctx, shape),
        "else_statement" => render_else(node, ctx, shape),
        "while_statement" => render_while(node, ctx, shape),
        "for_statement" => render_for(node, ctx, shape),
        "foreach_statement" => render_foreach(node, ctx, shape),
        "do_while_statement" => render_do_while(node, ctx, shape),
        "switch_statement" => render_switch(node, ctx, shape),
        "case_statement" | "default_statement" => render_case(node, ctx, shape),
        "try_statement" => render_try(node, ctx, shape),
        "catch_statement" => render_catch(node, ctx, shape),

        "function_declaration" => render_function_declaration(node, ctx, shape),
        "class_declaration" => render_class(node, ctx, shape),
        "member_declaration" | "table_slot" => render_slot(node, ctx, shape),

        "local_declaration" => render_local(node, ctx, shape),
        "const_declaration" => render_const(node, ctx, shape),
        "enum_declaration" => render_enum(node, ctx, shape),
        "assignment_expression" => render_assignment(node, ctx, shape),
        "return" | "return_statement" => render_prefixed(node, ctx, shape, "return"),
        "throw_statement" => render_prefixed(node, ctx, shape, "throw"),
        // Each of these holds its own ';', which render_statements writes
        "yield" => render_prefixed(node, ctx, shape, "yield"),
        "delete_expression" => render_prefixed(node, ctx, shape, "delete"),
        "resume_expression" => render_prefixed(node, ctx, shape, "resume"),
        // The keyword node holds its own ';', which render_statements has already written
        "break" | "continue" => render_prefixed(node, ctx, shape, node.kind()),

        "binary_expression" => render_binary(node, ctx, shape),
        "ternary_expression" => render_ternary(node, ctx, shape),
        "unary_expression" | "update_expression" | "clone_expression" => {
            render_tight(node, ctx, shape)
        },
        "parenthesized_expression" => render_parenthesized(node, ctx, shape),
        "call_expression" => render_call(node, ctx, shape),
        "index_expression" => render_index(node, ctx, shape),
        "deref_expression" => render_deref(node, ctx, shape),
        "array" => render_items(&list_children(node), &ARRAY, ctx, shape),
        "table" => render_table(node, ctx, shape),
        "anonymous_function" => render_anonymous_function(node, ctx, shape),
        "lambda_expression" => render_lambda(node, ctx, shape),
        "parameters" | "call_args" => render_items(&list_children(node), &ARGS, ctx, shape),
        "parameter" => ctx.text(node).to_string(),

        // Literals, identifiers, comments and anything the dispatch does not know yet
        _ => ctx.text(node).to_string(),
    }
}

/// `local a = 1`, and the comma form `local a = 1, b = 2`.
/// The ',' and the '=' are anonymous tokens, so the formatter reads them off the node.
/// Joining every named child with " = " would turn the comma form into one long assignment.
fn render_local(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut line = Line::new(ctx, shape);
    line.put("local ");

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "=" => line.put(" = "),
            "," => line.put(", "),
            _ if is_part(&child) => line.node(child),
            _ => &mut line,
        };
    }

    line.finish()
}

fn render_assignment(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let [target, value, ..] = named_children(node)[..] else {
        return ctx.text(node).to_string();
    };
    Line::new(ctx, shape)
        .node(target)
        .put(" ")
        .put(assignment_operator(node, ctx))
        .put(" ")
        .node(value)
        .finish()
}

fn assignment_operator<'a>(node: Node, ctx: &Ctx<'a>) -> &'a str {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| !child.is_named())
        .map_or("=", |child| ctx.text(child))
}

fn render_prefixed(node: Node, ctx: &Ctx, shape: Shape, keyword: &str) -> String {
    match named_child(node, 0) {
        None => keyword.to_string(),
        Some(value) => Line::new(ctx, shape)
            .put(keyword)
            .put(" ")
            .node(value)
            .finish(),
    }
}

fn render_call(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let Some(callee) = named_child(node, 0) else {
        return ctx.text(node).to_string();
    };

    let items = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "call_args")
        .map(list_children)
        .unwrap_or_default();

    let mut line = Line::new(ctx, shape);
    line.node(callee);
    let args = render_items(&items, &ARGS, ctx, line.shape);
    line.put(&args).finish()
}

fn render_index(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let [target, key, ..] = named_children(node)[..] else {
        return ctx.text(node).to_string();
    };
    Line::new(ctx, shape)
        .node(target)
        .put("[")
        .node(key)
        .put("]")
        .finish()
}

fn render_deref(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let [target, field, ..] = named_children(node)[..] else {
        return ctx.text(node).to_string();
    };
    Line::new(ctx, shape)
        .node(target)
        .put(".")
        .node(field)
        .finish()
}

fn render_parenthesized(node: Node, ctx: &Ctx, shape: Shape) -> String {
    match named_child(node, 0) {
        None => "()".to_string(),
        Some(inner) => Line::new(ctx, shape).put("(").node(inner).put(")").finish(),
    }
}

fn render_table(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let slots: Vec<Node> = node
        .children(&mut node.walk())
        .filter(|child| child.kind() == "table_slots")
        .flat_map(list_children)
        .collect();
    render_items(&slots, &TABLE, ctx, shape)
}

fn render_lambda(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let mut value = None;
    let mut parameters = None;

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "parameters" => parameters = Some(child),
            _ if child.is_named() => value = Some(child),
            _ => {},
        }
    }

    let items = parameters.map(list_children).unwrap_or_default();
    let mut line = Line::new(ctx, shape);
    line.put("@");
    let params = render_items(&items, &ARGS, ctx, line.shape);
    line.put(&params);

    if value.is_some() {
        line.put(" ");
    }
    line.opt_node(value).finish()
}

fn render_ternary(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let parts = named_children(node);
    if parts.len() < 3 {
        return ctx.text(node).to_string();
    }

    let one_line = format!(
        "{} ? {} : {}",
        render_inline(parts[0], ctx),
        render_inline(parts[1], ctx),
        render_inline(parts[2], ctx)
    );
    if shape.fits(&one_line, ctx) {
        return one_line;
    }

    let inner = shape.block(ctx);
    let indent = ctx.indent(inner.indent);
    format!(
        "{}\n{}? {}\n{}: {}",
        render(parts[0], ctx, shape),
        indent,
        render(parts[1], ctx, inner.after("? ")),
        indent,
        render(parts[2], ctx, inner.after(": "))
    )
}

/// An operator the grammar gives no node of its own: `!x`, `-x`, `x++`, `clone x`, `x <- 1`.
/// With one operand the formatter writes the operator tight against it.
/// With two, such as `<-`, it puts a space either side.
/// After a word like `clone` it adds a space, unless a paren follows: `typeof(x)` stays tight.
fn render_tight(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let binary = named_child_count(node) >= 2;
    let mut line = Line::new(ctx, shape);
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.is_named() {
            line.node(child);
            continue;
        }

        let text = ctx.text(child);
        let parenthesized = child
            .next_sibling()
            .is_some_and(|next| ctx.text(next).starts_with('('));

        if binary {
            line.put(" ").put(text).put(" ");
        } else if text.chars().all(char::is_alphabetic) && !parenthesized {
            line.put(text).put(" ");
        } else {
            line.put(text);
        }
    }

    line.finish()
}

fn render_foreach(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let parts = named_children(node);
    let Some((body, head)) = parts.split_last() else {
        return ctx.text(node).to_string();
    };
    let Some((sequence, names)) = head.split_last() else {
        return ctx.text(node).to_string();
    };

    let names: Vec<String> = names.iter().map(|name| render(*name, ctx, shape)).collect();

    format!(
        "foreach ({} in {}) {}",
        names.join(", "),
        render(*sequence, ctx, shape),
        render_body(*body, ctx, shape)
    )
}

fn render_do_while(node: Node, ctx: &Ctx, shape: Shape) -> String {
    let parts = named_children(node);
    if parts.len() < 2 {
        return ctx.text(node).to_string();
    }
    format!(
        "do {} while ({})",
        render_body(parts[0], ctx, shape),
        render(parts[1], ctx, shape)
    )
}

// ---------------------------------------------------------------------------
// Tree helpers
// ---------------------------------------------------------------------------

/// Whether the parser could not read this node as the grammar says it should be.
/// Either it invented a token the source does not have, such as an unclosed table's '}',
/// or it gave up on a run of source it could not fit the grammar at all.
/// Either way the tree no longer says what the source says, and a renderer working from
/// it writes something else again. The formatter copies such a node instead.
fn parser_gave_up(node: Node) -> bool {
    if node.is_error() || node.is_missing() {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.is_missing() || child.is_error())
}

/// Whether `rendered` lost a comment the source wrote inside `node`.
/// The formatter asks after the fact, rather than keep a list of the renderers that
/// know what to do with one: a renderer that learns to place a comment stops answering
/// yes on its own.
fn drops_a_comment(node: Node, ctx: &Ctx, rendered: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == "comment")
        .any(|comment| !rendered.contains(ctx.text(comment)))
}

/// The `</ ... />` a member can carry in front of its name.
fn is_attribute(node: &Node) -> bool {
    node.kind() == "attribute_declaration"
}

/// A child a renderer takes apart: named, and not a comment.
/// The formatter places comments from where the source put them, not from the node's shape.
fn is_part(node: &Node) -> bool {
    node.is_named() && node.kind() != "comment"
}

fn named_child(node: Node, index: usize) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(is_part).nth(index)
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(is_part).collect()
}

fn named_child_count(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).filter(is_part).count()
}

/// The children of a list, comments included.
fn list_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(tree_sitter::Node::is_named)
        .collect()
}
