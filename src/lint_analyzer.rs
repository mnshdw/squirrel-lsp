//! Style lints.

use std::collections::HashMap;

use serde::Deserialize;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers::{self, LineIndex};

pub const MISSING_SEMICOLON: &str = "missing-semicolon";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LintLevel {
    #[default]
    Off,
    Hint,
    Information,
    Warning,
    Error,
}

/// Which lints to run and their level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct LintConfig {
    pub levels: HashMap<String, LintLevel>,
}

impl LintConfig {
    fn severity(&self, lint: &str) -> Option<DiagnosticSeverity> {
        match self.levels.get(lint).copied().unwrap_or_default() {
            LintLevel::Off => None,
            LintLevel::Hint => Some(DiagnosticSeverity::HINT),
            LintLevel::Information => Some(DiagnosticSeverity::INFORMATION),
            LintLevel::Warning => Some(DiagnosticSeverity::WARNING),
            LintLevel::Error => Some(DiagnosticSeverity::ERROR),
        }
    }
}

/// Run the enabled lints over `text`.
pub fn compute_lint_diagnostics(
    text: &str,
    config: &LintConfig,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let Some(severity) = config.severity(MISSING_SEMICOLON) else {
        return Ok(Vec::new());
    };

    let tree = helpers::parse_squirrel(text)?;
    let mut found = Vec::new();
    check_statements(tree.root_node(), text, &LineIndex::new(text), &mut found);

    Ok(found
        .into_iter()
        .map(|range| Diagnostic {
            range,
            severity: Some(severity),
            // Editors show the code, so the reader can see what to turn off.
            code: Some(NumberOrString::String(MISSING_SEMICOLON.to_string())),
            source: Some("squirrel-lint".to_string()),
            message: "Missing semicolon".to_string(),
            ..Diagnostic::default()
        })
        .collect())
}

/// Walk the tree while checking statements.
fn check_statements(node: Node<'_>, text: &str, lines: &LineIndex<'_>, found: &mut Vec<Range>) {
    let children_are_statements = helpers::is_statement_list(node.kind());

    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let child = cursor.node();
        if child.is_named() {
            let is_statement = children_are_statements && cursor.field_name().is_none();
            if is_statement && let Some(spot) = missing_semicolon(child, text, lines) {
                found.push(spot);
            }
            check_statements(child, text, lines, found);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

/// Where the `;` should have gone, or `None` if the statement is fine as it stands.
fn missing_semicolon(statement: Node<'_>, text: &str, lines: &LineIndex<'_>) -> Option<Range> {
    // Comments are not statements.
    if statement.is_extra() {
        return None;
    }

    let source = text.get(statement.start_byte()..statement.end_byte())?;

    // `const` and a few others swallow the newline that ends them, so the statement itself
    // ends with one and nothing is missing.
    if source.ends_with('\n') {
        return None;
    }

    let body = source.trim_end();

    // Nothing to add when a `;` is already there, or when the statement closes on a brace.
    if has_semicolon_after(statement) || body.ends_with(';') || body.ends_with('}') {
        return None;
    }

    // Underline the last character, which is where the `;` belongs.
    let last = body.chars().next_back()?;
    let end = statement.start_byte() + body.len();
    Some(Range::new(
        lines.position_at(end - last.len_utf8()),
        lines.position_at(end),
    ))
}

/// Whether a `;` follows the statement.
fn has_semicolon_after(statement: Node<'_>) -> bool {
    std::iter::successors(statement.next_sibling(), |node| node.next_sibling())
        .find(|node| !node.is_extra())
        .is_some_and(|node| node.kind() == ";")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(level: LintLevel, text: &str) -> Vec<Diagnostic> {
        let config = LintConfig {
            levels: HashMap::from([(MISSING_SEMICOLON.to_string(), level)]),
        };
        compute_lint_diagnostics(text, &config).expect("lint failed")
    }

    fn warnings(text: &str) -> Vec<Diagnostic> {
        at(LintLevel::Warning, text)
    }

    #[test]
    fn off_unless_the_client_asks_for_a_level() {
        assert!(
            compute_lint_diagnostics("local x = 10\nfoo()\n", &LintConfig::default())
                .unwrap()
                .is_empty()
        );
        assert!(at(LintLevel::Off, "local x = 10\nfoo()\n").is_empty());
    }

    #[test]
    fn levels_are_read_by_lint_name() {
        let config: LintConfig =
            serde_json::from_str(r#"{"missing-semicolon": "error"}"#).expect("valid config");
        assert_eq!(
            config.severity(MISSING_SEMICOLON),
            Some(DiagnosticSeverity::ERROR)
        );
        assert_eq!(config.severity("some-future-lint"), None);
    }

    #[test]
    fn each_statement_is_reported_once() {
        assert_eq!(warnings("local x = 10\nfoo()\n").len(), 2, "file scope");
        assert_eq!(warnings("function f()\n{\n\tfoo()\n}\n").len(), 1, "block");
        assert_eq!(
            warnings("switch (a)\n{\ncase 1:\n\tfoo()\n\tbreak;\n}\n").len(),
            1,
            "case body"
        );
    }

    #[test]
    fn terminated_statements_are_quiet() {
        assert!(warnings("local x = 10;\nfoo();\nthis.m.A <- 1;\n").is_empty());
        assert!(warnings("foo() /* wait */ ;\n").is_empty());
    }

    #[test]
    fn statements_the_grammar_terminates_are_quiet() {
        assert!(warnings("const A = 1\n").is_empty(), "const");
        assert!(
            warnings("enum E { A, B }\nclass C {}\nfunction f() {}\nif (a) {}\nwhile (b) {}\n")
                .is_empty(),
            "declarations and compound statements"
        );
    }

    #[test]
    fn nested_expressions_are_not_statements() {
        assert!(warnings("bar(foo());\nlocal x = baz();\n").is_empty());
        assert!(warnings("for (local i = 0; i < 5; i++)\n{\n\tfoo();\n}\n").is_empty());
    }

    #[test]
    fn severity_comes_from_the_configured_level() {
        let diags = at(LintLevel::Hint, "foo()\n");
        assert_eq!(diags[0].severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(
            diags[0].code,
            Some(NumberOrString::String(MISSING_SEMICOLON.to_string()))
        );
    }

    #[test]
    fn the_range_covers_the_last_character() {
        let range = warnings("local x = 10\n")[0].range;
        assert_eq!((range.start.character, range.end.character), (11, 12));
        assert_eq!(warnings("local s = \"héllo→\"\n").len(), 1);
    }
}
