//! Symbol resolution for Squirrel semantic analysis.
//!
//! This module resolves identifiers against the symbol maps,
//! checking scope, class members, and inherited members.

use std::collections::HashSet;
use std::sync::LazyLock;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, Position, Range};
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers;
use crate::symbol_extractor::extract_file_symbols;
use crate::symbols::FileSymbols;

static BUILTINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        // Core built-in functions
        "array",
        "assert",
        "callee",
        "clone",
        "collectgarbage",
        "compilestring",
        "enabledebuginfo",
        "error",
        "format",
        "getconsttable",
        "getroottable",
        "getstackinfos",
        "newthread",
        "print",
        "regexp",
        "resurrectunreachable",
        "setconsttable",
        "setdebughook",
        "seterrorhandler",
        "setroottable",
        "suspend",
        "throw",
        "type",
        "typeof",
        // Special keywords
        "this",
        "Math",
        // Battle Brothers specific
        "inherit",
    ])
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    Parameter,
    Local,
    LoopVariable,
    CatchVariable,
}

#[derive(Debug, Clone)]
struct Declaration {
    name: String,
    range: Range,
    kind: DeclarationKind,
}

#[derive(Debug, Clone)]
struct ResolverContext {
    locals: HashSet<String>,
    declarations: Vec<Declaration>,
    references: HashSet<String>,
    has_parent: bool,
}

impl ResolverContext {
    fn new() -> Self {
        Self {
            locals: HashSet::new(),
            declarations: Vec::new(),
            references: HashSet::new(),
            has_parent: false,
        }
    }

    fn add_declaration(&mut self, name: String, range: Range, kind: DeclarationKind) {
        self.locals.insert(name.clone());
        self.declarations.push(Declaration { name, range, kind });
    }

    fn record_reference(&mut self, name: &str) {
        self.references.insert(name.to_string());
    }

    fn merge_references(&mut self, child: &ResolverContext) {
        for name in &child.references {
            if self.locals.contains(name) {
                self.references.insert(name.clone());
            }
        }
    }

    fn child(&self) -> Self {
        Self {
            locals: self.locals.clone(),
            declarations: Vec::new(),
            references: HashSet::new(),
            has_parent: self.has_parent,
        }
    }
}

pub struct SymbolResolver<'a> {
    text: &'a str,
    file_symbols: FileSymbols,
    known_globals: Option<&'a HashSet<String>>,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> SymbolResolver<'a> {
    #[allow(dead_code)]
    pub fn new(file_path: &str, text: &'a str) -> Result<Self, AnalysisError> {
        let file_symbols = extract_file_symbols(file_path, text)?;
        Ok(Self {
            text,
            file_symbols,
            known_globals: None,
            diagnostics: Vec::new(),
        })
    }

    pub fn with_globals(
        file_path: &str,
        text: &'a str,
        globals: &'a HashSet<String>,
    ) -> Result<Self, AnalysisError> {
        let file_symbols = extract_file_symbols(file_path, text)?;
        Ok(Self {
            text,
            file_symbols,
            known_globals: Some(globals),
            diagnostics: Vec::new(),
        })
    }

    pub fn analyze(mut self) -> Result<Vec<Diagnostic>, AnalysisError> {
        let tree = helpers::parse_squirrel(self.text)?;
        let root = tree.root_node();

        let mut ctx = ResolverContext::new();
        for name in self.file_symbols.symbols.keys() {
            ctx.locals.insert(name.clone());
        }

        self.analyze_script(root, &mut ctx);
        self.report_unused_variables(&ctx);

        Ok(self.diagnostics)
    }

    fn analyze_script(&mut self, node: Node, ctx: &mut ResolverContext) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "local_declaration" | "var_statement" | "const_declaration" => {
                    if let Some(ident) = self.find_declaration_name(child) {
                        let name = self.node_text(ident).to_string();
                        let range = Range::new(
                            self.position_at(ident.start_byte()),
                            self.position_at(ident.end_byte()),
                        );
                        ctx.add_declaration(name, range, DeclarationKind::Local);
                    }
                    self.analyze_declaration(child, ctx);
                },
                "function_declaration" => {
                    if let Some(ident) = self.find_first_identifier(child) {
                        ctx.locals.insert(self.node_text(ident).to_string());
                    }
                    self.analyze_function(child, ctx);
                },
                "class_declaration" => {
                    if let Some(ident) = self.find_first_identifier(child) {
                        ctx.locals.insert(self.node_text(ident).to_string());
                    }
                    self.analyze_node(child, ctx);
                },
                _ => {
                    self.analyze_node(child, ctx);
                },
            }
        }
    }

    fn analyze_node(&mut self, node: Node, ctx: &mut ResolverContext) {
        let kind = node.kind();

        match kind {
            "function_declaration" | "lambda_expression" | "anonymous_function" => {
                self.analyze_function(node, ctx);
                return;
            },
            "table" => {
                self.analyze_table(node, ctx);
                return;
            },
            "class_declaration" => {
                self.analyze_class(node, ctx);
                return;
            },
            "block" => {
                self.analyze_block(node, ctx);
                return;
            },
            "for_statement" => {
                self.analyze_for(node, ctx);
                return;
            },
            "foreach_statement" => {
                self.analyze_foreach(node, ctx);
                return;
            },
            "try_statement" => {
                self.analyze_try(node, ctx);
                return;
            },
            "catch_statement" => {
                self.analyze_catch(node, ctx);
                return;
            },
            "call_expression" => {
                if self.is_inherit_call(node) {
                    self.analyze_inherit_call(node, ctx);
                    return;
                }
            },
            "local_declaration" | "var_statement" => {
                self.analyze_declaration(node, ctx);
                return;
            },
            "identifier" => {
                self.check_identifier(node, ctx);
            },
            _ => {},
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.analyze_node(child, ctx);
        }
    }

    fn report_unused_variables(&mut self, ctx: &ResolverContext) {
        for decl in &ctx.declarations {
            if !ctx.references.contains(&decl.name) {
                let severity = match decl.kind {
                    DeclarationKind::Parameter => DiagnosticSeverity::HINT,
                    _ => DiagnosticSeverity::WARNING,
                };
                self.diagnostics.push(Diagnostic {
                    range: decl.range,
                    severity: Some(severity),
                    source: Some("squirrel-semantic".to_string()),
                    message: format!("Unused variable '{}'", decl.name),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Diagnostic::default()
                });
            }
        }
    }

    fn analyze_function(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        let mut ctx = parent_ctx.child();

        for child in node.children(&mut node.walk()) {
            if child.kind() == "parameters" {
                for param in child.children(&mut child.walk()) {
                    if param.kind() == "parameter"
                        && let Some(ident) = self.find_first_identifier(param)
                    {
                        let name = self.node_text(ident).to_string();
                        let range = Range::new(
                            self.position_at(ident.start_byte()),
                            self.position_at(ident.end_byte()),
                        );
                        ctx.add_declaration(name, range, DeclarationKind::Parameter);
                    }
                }
            }
        }

        let mut found_body = false;
        for child in node.children(&mut node.walk()) {
            if child.kind() == "block" {
                self.analyze_block_inner(child, &mut ctx, true);
                found_body = true;
            }
        }

        // Lambda expressions can have an expression body instead of a block
        // e.g., @(idx, item) item != null
        if !found_body && node.kind() == "lambda_expression" {
            for child in node.children(&mut node.walk()) {
                if child.kind() != "parameters"
                    && child.kind() != "@"
                    && child.kind() != "("
                    && child.kind() != ")"
                {
                    self.analyze_node(child, &mut ctx);
                }
            }
        }

        // Merge references back to parent so variables used in closures
        // are marked as used in the enclosing scope
        parent_ctx.merge_references(&ctx);
        self.report_unused_variables(&ctx);
    }

    /// If `is_function_body` is true, declarations stay in the function scope.
    /// Otherwise, a new block scope is created for if/while/etc blocks.
    fn analyze_block_inner(
        &mut self,
        node: Node,
        parent_ctx: &mut ResolverContext,
        is_function_body: bool,
    ) {
        if is_function_body {
            self.analyze_block_statements(node, parent_ctx);
        } else {
            let mut ctx = parent_ctx.child();
            self.analyze_block_statements(node, &mut ctx);
            self.report_unused_variables(&ctx);
            parent_ctx.merge_references(&ctx);
        }
    }

    fn analyze_block(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        self.analyze_block_inner(node, parent_ctx, false);
    }

    fn analyze_block_statements(&mut self, node: Node, ctx: &mut ResolverContext) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "local_declaration" | "var_statement" | "const_declaration" => {
                    if let Some(ident) = self.find_declaration_name(child) {
                        let name = self.node_text(ident).to_string();
                        let range = Range::new(
                            self.position_at(ident.start_byte()),
                            self.position_at(ident.end_byte()),
                        );
                        ctx.add_declaration(name, range, DeclarationKind::Local);
                    }
                    self.analyze_declaration(child, ctx);
                },
                "foreach_statement" => {
                    self.analyze_foreach(child, ctx);
                },
                "for_statement" => {
                    self.analyze_for(child, ctx);
                },
                _ => {
                    self.analyze_node(child, ctx);
                },
            }
        }
    }

    fn analyze_class(&mut self, node: Node, parent_ctx: &ResolverContext) {
        let members = self.extract_class_member_names(node);

        let mut ctx = parent_ctx.clone();
        ctx.has_parent = false;
        for member in &members {
            ctx.locals.insert(member.clone());
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "extends" {
                ctx.has_parent = true;
            }
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "class_body" {
                self.analyze_class_body(child, &ctx);
            }
        }
    }

    fn analyze_class_body(&mut self, node: Node, ctx: &ResolverContext) {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "class_member" {
                self.analyze_class_member(child, ctx);
            }
        }
    }

    fn analyze_class_member(&mut self, node: Node, ctx: &ResolverContext) {
        let mut ctx = ctx.clone();
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" | "=" | "static" => {},
                "function_declaration" => {
                    self.analyze_function(child, &mut ctx);
                },
                _ => {
                    self.analyze_node(child, &mut ctx);
                },
            }
        }
    }

    fn extract_class_member_names(&self, node: Node) -> HashSet<String> {
        let mut names = HashSet::new();
        for child in node.children(&mut node.walk()) {
            if child.kind() == "class_body" {
                for member in child.children(&mut child.walk()) {
                    if member.kind() == "class_member"
                        && let Some(name) = self.extract_class_member_name(member)
                    {
                        names.insert(name);
                    }
                }
            }
        }
        names
    }

    fn extract_class_member_name(&self, node: Node) -> Option<String> {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    return Some(self.node_text(child).to_string());
                },
                "function_declaration" => {
                    if let Some(ident) = self.find_first_identifier(child) {
                        return Some(self.node_text(ident).to_string());
                    }
                },
                _ => {},
            }
        }
        None
    }

    fn analyze_table(&mut self, node: Node, parent_ctx: &ResolverContext) {
        let slots = self.extract_table_slot_names(node);

        let mut ctx = parent_ctx.clone();
        for slot in &slots {
            ctx.locals.insert(slot.clone());
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "table_slots" {
                for slot in child.children(&mut child.walk()) {
                    if slot.kind() == "table_slot" {
                        self.analyze_table_slot(slot, &ctx);
                    }
                }
            }
        }
    }

    fn analyze_table_slot(&mut self, node: Node, ctx: &ResolverContext) {
        let mut ctx = ctx.clone();
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" | "=" | "," => {},
                "function_declaration" => {
                    self.analyze_function(child, &mut ctx);
                },
                _ => {
                    self.analyze_node(child, &mut ctx);
                },
            }
        }
    }

    fn analyze_inherit_call(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "call_args" {
                for arg in child.children(&mut child.walk()) {
                    if arg.kind() == "table" {
                        let slots = self.extract_table_slot_names(arg);

                        let mut ctx = parent_ctx.child();
                        ctx.has_parent = true;
                        for slot in &slots {
                            ctx.locals.insert(slot.clone());
                        }

                        self.analyze_table(arg, &ctx);
                    } else {
                        self.analyze_node(arg, parent_ctx);
                    }
                }
            }
        }
    }

    fn analyze_declaration(&mut self, node: Node, ctx: &mut ResolverContext) {
        let mut seen_declaration_name = false;
        for child in node.children(&mut node.walk()) {
            if matches!(child.kind(), "local" | "var" | "const" | "=") {
                continue;
            }
            if child.kind() == "identifier" && !seen_declaration_name {
                seen_declaration_name = true;
                continue;
            }
            self.analyze_node(child, ctx);
        }
    }

    fn analyze_foreach(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        let mut ctx = parent_ctx.child();

        if let Some(index) = node.child_by_field_name("index") {
            let name = self.node_text(index).to_string();
            let range = Range::new(
                self.position_at(index.start_byte()),
                self.position_at(index.end_byte()),
            );
            ctx.add_declaration(name, range, DeclarationKind::LoopVariable);
        }
        if let Some(value) = node.child_by_field_name("value") {
            let name = self.node_text(value).to_string();
            let range = Range::new(
                self.position_at(value.start_byte()),
                self.position_at(value.end_byte()),
            );
            ctx.add_declaration(name, range, DeclarationKind::LoopVariable);
        }

        let collection = node.child_by_field_name("collection");
        if let Some(coll) = collection {
            self.analyze_node(coll, &mut ctx);
        }

        let index = node.child_by_field_name("index");
        let value = node.child_by_field_name("value");
        for child in node.children(&mut node.walk()) {
            if Some(child) == index || Some(child) == value || Some(child) == collection {
                continue;
            }
            if child.kind() == "block" {
                self.analyze_block(child, &mut ctx);
            } else if !Self::is_syntax_token(child.kind())
                && child.kind() != "foreach"
                && child.kind() != "in"
            {
                self.analyze_node(child, &mut ctx);
            }
        }

        self.report_unused_variables(&ctx);
        parent_ctx.merge_references(&ctx);
    }

    fn analyze_for(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        let mut ctx = parent_ctx.child();

        for child in node.children(&mut node.walk()) {
            if child.kind() == "local_declaration"
                && let Some(ident) = self.find_declaration_name(child)
            {
                let name = self.node_text(ident).to_string();
                let range = Range::new(
                    self.position_at(ident.start_byte()),
                    self.position_at(ident.end_byte()),
                );
                ctx.add_declaration(name, range, DeclarationKind::LoopVariable);
            }
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "block" {
                self.analyze_block(child, &mut ctx);
            } else if !Self::is_syntax_token(child.kind()) {
                self.analyze_node(child, &mut ctx);
            }
        }

        self.report_unused_variables(&ctx);
        parent_ctx.merge_references(&ctx);
    }

    fn analyze_try(&mut self, node: Node, ctx: &mut ResolverContext) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.analyze_node(child, ctx);
        }
    }

    fn analyze_catch(&mut self, node: Node, parent_ctx: &mut ResolverContext) {
        let mut ctx = parent_ctx.child();

        for child in node.children(&mut node.walk()) {
            if child.kind() == "identifier" {
                let name = self.node_text(child).to_string();
                let range = Range::new(
                    self.position_at(child.start_byte()),
                    self.position_at(child.end_byte()),
                );
                ctx.add_declaration(name, range, DeclarationKind::CatchVariable);
            }
        }

        for child in node.children(&mut node.walk()) {
            if child.kind() == "block" {
                self.analyze_block(child, &mut ctx);
            }
        }

        self.report_unused_variables(&ctx);
        parent_ctx.merge_references(&ctx);
    }

    fn check_identifier(&mut self, node: Node, ctx: &mut ResolverContext) {
        if self.should_skip_identifier(node) {
            return;
        }

        let name = self.node_text(node);

        if BUILTINS.contains(name) {
            return;
        }

        // Important: check locals first as they shadow globals
        if ctx.locals.contains(name) {
            ctx.record_reference(name);
            return;
        }

        if self.known_globals.is_some_and(|g| g.contains(name)) {
            return;
        }

        // Inherited methods might come from parent class
        if ctx.has_parent && self.is_function_call(node) {
            return;
        }

        let start = self.position_at(node.start_byte());
        let end = self.position_at(node.end_byte());
        self.diagnostics.push(Diagnostic {
            range: Range::new(start, end),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("squirrel-semantic".to_string()),
            message: format!("Undeclared variable '{}'", name),
            ..Diagnostic::default()
        });
    }

    fn should_skip_identifier(&self, node: Node) -> bool {
        let Some(parent) = node.parent() else {
            return false;
        };
        let parent_kind = parent.kind();

        if matches!(
            parent_kind,
            "local_declaration"
                | "var_statement"
                | "const_declaration"
                | "function_declaration"
                | "class_declaration"
                | "parameter"
                | "table_slot"
                | "enum_declaration"
        ) && let Some(first) = self.find_first_identifier(parent)
            && first.id() == node.id()
        {
            return true;
        }

        // Skip property accesses (obj.property)
        if parent_kind == "deref_expression"
            && let Some(prev) = node.prev_sibling()
            && (prev.kind() == "." || prev.kind() == "::")
        {
            return true;
        }

        // Skip global variable syntax (::var)
        if parent_kind == "global_variable" {
            return true;
        }

        // Skip new slot declaration LHS (name <- value)
        if parent_kind == "update_expression" {
            let mut is_first = false;
            let mut has_new_slot = false;
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "identifier" && !is_first {
                    is_first = child.id() == node.id();
                }
                if child.kind() == "<-" {
                    has_new_slot = true;
                }
            }
            if is_first && has_new_slot {
                return true;
            }
        }

        false
    }

    fn is_function_call(&self, node: Node) -> bool {
        if let Some(parent) = node.parent()
            && parent.kind() == "call_expression"
        {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "identifier" {
                    return child.id() == node.id();
                }
                if child.kind() == "call_args" {
                    break;
                }
            }
        }
        false
    }

    fn is_inherit_call(&self, node: Node) -> bool {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    if self.node_text(child) == "inherit" {
                        return true;
                    }
                },
                "deref_expression" => {
                    let mut last_ident = "";
                    for subchild in child.children(&mut child.walk()) {
                        if subchild.kind() == "identifier" {
                            last_ident = self.node_text(subchild);
                        }
                    }
                    if last_ident == "inherit" {
                        return true;
                    }
                },
                _ => {},
            }
        }
        false
    }

    fn extract_table_slot_names(&self, node: Node) -> HashSet<String> {
        let mut names = HashSet::new();
        for child in node.children(&mut node.walk()) {
            if child.kind() == "table_slots" {
                for slot in child.children(&mut child.walk()) {
                    if slot.kind() == "table_slot"
                        && let Some(name) = self.extract_slot_name(slot)
                    {
                        names.insert(name);
                    }
                }
            }
        }
        names
    }

    fn extract_slot_name(&self, node: Node) -> Option<String> {
        for child in node.children(&mut node.walk()) {
            match child.kind() {
                "identifier" => {
                    return Some(self.node_text(child).to_string());
                },
                "function_declaration" => {
                    if let Some(ident) = self.find_first_identifier(child) {
                        return Some(self.node_text(ident).to_string());
                    }
                },
                _ => {},
            }
        }
        None
    }

    fn find_declaration_name<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        node.children(&mut node.walk())
            .find(|&child| child.kind() == "identifier")
    }

    fn find_first_identifier<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        node.children(&mut node.walk())
            .find(|&child| child.kind() == "identifier")
    }

    fn is_syntax_token(kind: &str) -> bool {
        matches!(
            kind,
            "(" | ")"
                | "{"
                | "}"
                | "["
                | "]"
                | ";"
                | ","
                | "."
                | ":"
                | "for"
                | "foreach"
                | "while"
                | "if"
                | "else"
                | "in"
                | "do"
        )
    }

    fn node_text(&self, node: Node) -> &str {
        node.utf8_text(self.text.as_bytes()).unwrap_or("")
    }

    fn position_at(&self, byte_offset: usize) -> Position {
        helpers::position_at(self.text, byte_offset)
    }
}

#[allow(dead_code)]
pub fn compute_symbol_diagnostics(
    file_path: &str,
    text: &str,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let resolver = SymbolResolver::new(file_path, text)?;
    resolver.analyze()
}

pub fn compute_symbol_diagnostics_with_globals(
    file_path: &str,
    text: &str,
    globals: &HashSet<String>,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let resolver = SymbolResolver::with_globals(file_path, text, globals)?;
    resolver.analyze()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_variable() {
        let code = r#"
            function test() {
                local x = 1;
                return x;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(diagnostics.is_empty(), "Got: {:?}", diagnostics);
    }

    #[test]
    fn test_undeclared_variable() {
        let code = r#"
            function test() {
                return x;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Undeclared variable 'x'"));
    }

    #[test]
    fn test_table_slot_visibility() {
        let code = r#"
            my_class <- {
                m = { Container = null },
                function getContainer() {
                    return m.Container;
                }
            };
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        // 'm' should be visible to getContainer
        let undeclared: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Undeclared"))
            .collect();
        assert!(undeclared.is_empty(), "Got: {:?}", undeclared);
    }

    #[test]
    fn test_sibling_function_visibility() {
        let code = r#"
            my_class <- inherit("scripts/base", {
                function helper() {
                    return 42;
                },
                function main() {
                    local x = helper();
                    return x;
                }
            });
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        // 'helper' should be visible to main
        let undeclared: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Undeclared"))
            .collect();
        assert!(undeclared.is_empty(), "Got: {:?}", undeclared);
    }

    #[test]
    fn test_inherited_function_call() {
        let code = r#"
            this.perk <- this.inherit("scripts/skills/skill", {
                function onAdded() {
                    local container = getContainer();
                }
            });
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        // 'getContainer' is inherited from parent, should not error
        let undeclared: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Undeclared"))
            .collect();
        assert!(undeclared.is_empty(), "Got: {:?}", undeclared);
    }

    #[test]
    fn test_foreach_variables() {
        let code = r#"
            function test() {
                local items = [1, 2, 3];
                foreach (item in items) {
                    print(item);
                }
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(diagnostics.is_empty(), "Got: {:?}", diagnostics);
    }

    #[test]
    fn test_for_loop_variable() {
        let code = r#"
            function test() {
                for (local i = 0; i < 10; i++) {
                    print(i);
                }
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(diagnostics.is_empty(), "Got: {:?}", diagnostics);
    }

    #[test]
    fn test_unused_local_variable() {
        let code = r#"
            function test() {
                local unused = 1;
                local used = 2;
                return used;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unused"))
            .collect();
        assert_eq!(unused.len(), 1);
        assert!(unused[0].message.contains("'unused'"));
        assert_eq!(unused[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn test_unused_parameter_is_hint() {
        let code = r#"
            function test(unused_param) {
                return 42;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unused"))
            .collect();
        assert_eq!(unused.len(), 1);
        assert!(unused[0].message.contains("'unused_param'"));
        // Parameters should be HINT, not WARNING
        assert_eq!(unused[0].severity, Some(DiagnosticSeverity::HINT));
    }

    #[test]
    fn test_unused_loop_variable() {
        let code = r#"
            function test() {
                local items = [1, 2, 3];
                foreach (i, item in items) {
                    print(item);
                }
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        // 'i' is unused, 'item' is used
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unused"))
            .collect();
        assert_eq!(unused.len(), 1);
        assert!(unused[0].message.contains("'i'"));
        assert_eq!(unused[0].severity, Some(DiagnosticSeverity::WARNING));
    }

    #[test]
    fn test_used_variable_not_reported() {
        let code = r#"
            function test() {
                local x = 1;
                local y = x + 1;
                return y;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unused"))
            .collect();
        assert!(unused.is_empty(), "Got: {:?}", unused);
    }

    #[test]
    fn test_top_level_undeclared_and_unused() {
        let code = r#"
            local x = y;
            local z;
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        // Should have undeclared 'y'
        let undeclared: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Undeclared"))
            .collect();
        assert_eq!(undeclared.len(), 1);
        assert!(undeclared[0].message.contains("'y'"));

        // Should have unused 'x' and 'z'
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("Unused"))
            .collect();
        assert_eq!(unused.len(), 2, "Expected 2 unused, got: {:?}", unused);
    }

    #[test]
    fn test_foreach_with_if_statement() {
        let code = r#"
            function test() {
                local itemsInBag = [1, 2, 3];
                foreach (item in itemsInBag) {
                    if (item != null && item == 1)
                        return true;
                }
                return false;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        let unused: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            unused.is_empty(),
            "item should not be reported as unused: {:?}",
            unused
        );
    }

    #[test]
    fn test_foreach_with_method_call() {
        let code = r#"
            function test(_bro) {
                local itemsInBag = _bro.getItems().getAllItemsAtSlot(1);
                foreach (item in itemsInBag) {
                    if (item != null && item.getID() == "accessory.arena_collar")
                        return true;
                }
                return false;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should not have any diagnostics: {:?}",
            item_diags
        );
    }

    #[test]
    fn test_foreach_shadowing_local() {
        let code = r#"
            function hasCollar(_bro) {
                local item = _bro.getItems().getItemAtSlot(1);
                if (item != null && item.getID() == "accessory.arena_collar")
                    return true;

                local itemsInBag = _bro.getItems().getAllItemsAtSlot(2);
                foreach (item in itemsInBag) {
                    if (item != null && item.getID() == "accessory.arena_collar")
                        return true;
                }
                return false;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        // The foreach 'item' shadows the outer 'item', both are used
        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should not have any diagnostics: {:?}",
            item_diags
        );
    }

    #[test]
    fn test_global_anonymous_function_foreach() {
        let code = r#"
            ::Legends.Arena.hasCollar <- function (_bro) {
                local item = _bro.getItems().getItemAtSlot(1);
                if (item != null && item.getID() == "accessory.arena_collar")
                    return true;

                local itemsInBag = _bro.getItems().getAllItemsAtSlot(2);
                foreach (item in itemsInBag) {
                    if (item != null && item.getID() == "accessory.arena_collar")
                        return true;
                }
                return false;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should not have any diagnostics: {:?}",
            item_diags
        );
    }

    #[test]
    fn test_global_anonymous_function_foreach_with_globals() {
        let code = r#"
            ::Legends.Arena.hasCollar <- function (_bro) {
                local item = _bro.getItems().getItemAtSlot(1);
                if (item != null && item.getID() == "accessory.arena_collar")
                    return true;

                local itemsInBag = _bro.getItems().getAllItemsAtSlot(2);
                foreach (item in itemsInBag) {
                    if (item != null && item.getID() == "accessory.arena_collar")
                        return true;
                }
                return false;
            }
        "#;
        let globals: HashSet<String> = HashSet::new();
        let diagnostics =
            compute_symbol_diagnostics_with_globals("test.nut", code, &globals).unwrap();

        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should not have any diagnostics: {:?}",
            item_diags
        );
    }

    #[test]
    fn test_foreach_field_names() {
        let code = "foreach (item in items) { print(item); }";
        let tree = helpers::parse_squirrel(code).unwrap();
        let root = tree.root_node();

        fn find_foreach(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
            if node.kind() == "foreach_statement" {
                return Some(node);
            }
            for child in node.children(&mut node.walk()) {
                if let Some(found) = find_foreach(child) {
                    return Some(found);
                }
            }
            None
        }

        let foreach_node = find_foreach(root).expect("should find foreach");

        // Check field lookups
        let value = foreach_node.child_by_field_name("value");
        let collection = foreach_node.child_by_field_name("collection");

        assert!(value.is_some(), "value field should exist");
        assert_eq!(value.unwrap().utf8_text(code.as_bytes()).unwrap(), "item");
        assert!(collection.is_some(), "collection field should exist");
        assert_eq!(
            collection.unwrap().utf8_text(code.as_bytes()).unwrap(),
            "items"
        );
    }

    #[test]
    fn test_arena_nut_exact() {
        // Test the EXACT content from arena.nut with tabs
        let code = r#"
            if (!("Arena" in ::Legends))
                ::Legends.Arena <- {};

            ::Legends.Arena.getCollaredBros <- function () { return ::World.getPlayerRoster().getAll().filter(@(idx, bro) ::Legends.Arena.hasCollar(bro)); }

            ::Legends.Arena.hasCollar <- function (_bro) {
                local item = _bro.getItems().getItemAtSlot(::Const.ItemSlot.Accessory);
                if (item != null && item.getID() == "accessory.arena_collar")
                    return true;

                local itemsInBag = _bro.getItems().getAllItemsAtSlot(::Const.ItemSlot.Bag);
                foreach (item in itemsInBag) {
                    if (item != null && item.getID() == "accessory.arena_collar")
                        return true;
                }
                return false;
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("arena.nut", code).unwrap();

        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should not have any diagnostics: {:?}",
            item_diags
        );
    }

    #[test]
    fn test_lambda_expression_body() {
        // Lambda with expression body (not block body)
        let code = r#"
            local items = [1, 2, 3];
            local filtered = items.filter(@(idx, item) item != null);
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        // idx is unused (only item is used in the body)
        let idx_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'idx'"))
            .collect();
        assert_eq!(idx_diags.len(), 1, "idx should be reported as unused");

        // item IS used in the lambda body expression
        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should NOT be reported as unused: {:?}",
            item_diags
        );

        // filtered is unused
        let filtered_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'filtered'"))
            .collect();
        assert_eq!(
            filtered_diags.len(),
            1,
            "filtered should be reported as unused"
        );
    }

    #[test]
    fn test_lambda_in_filter_chain() {
        let code = r#"
            ::Legends.Maps.getAll <- function() {
                return ::World.Assets.getStash().getItems()
                    .filter(@(idx, item) item != null && (item.m.ID == ::Legends.Maps.Type.Legendary || item.m.ID == ::Legends.Maps.Type.Named));
            }

            ::Legends.Maps.removeLegendary <- function(_target) {
                foreach(map in ::Legends.Maps.getAll().filter(@(idx, item) item.m.ID == ::Legends.Maps.Type.Legendary && item.m.Target == _target))
                    ::World.Assets.getStash().remove(map);
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        let map_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'map'"))
            .collect();
        assert!(
            map_diags.is_empty(),
            "map should NOT be reported as unused: {:?}",
            map_diags
        );

        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should NOT be reported as unused: {:?}",
            item_diags
        );

        let idx_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'idx'"))
            .collect();
        assert_eq!(
            idx_diags.len(),
            2,
            "idx should be reported as unused twice (once per filter)"
        );
    }

    #[test]
    fn test_lambda_with_block_body() {
        // Lambda with block body (now correctly parsed as block, not table)
        let code = r#"
            local items = [1, 2, 3];
            local mapped = items.map(@(idx, item) { return item * 2; });
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();

        // item IS used in the lambda block body
        let item_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'item'"))
            .collect();
        assert!(
            item_diags.is_empty(),
            "item should NOT be reported as unused: {:?}",
            item_diags
        );

        // idx is unused
        let idx_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.contains("'idx'"))
            .collect();
        assert_eq!(idx_diags.len(), 1, "idx should be reported as unused");
    }

    #[test]
    fn test_closure_captures_outer_variable() {
        let code = r#"
            ::mods_hookExactClass("some/path", function (o) {
                local old_create = o.create;
                o.create = function () {
                    old_create();
                    this.m.IsRemovedAfterBattle = false;
                }
            });
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.message.contains("'old_create'"))
        );
    }

    #[test]
    fn test_variable_used_in_nested_if_block() {
        let code = r#"
            o.onMissed <- function (_attacker, _skill) {
                local actor = this.getContainer().getActor();
                if (_attacker != null
                    && _attacker.isAlive()
                    && !_attacker.isAlliedWith(actor)
                    && _attacker.getTile().getDistanceTo(actor.getTile()) == 1)
                {
                    doSomething(actor);
                }
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(!diagnostics.iter().any(|d| d.message.contains("'actor'")));
    }

    #[test]
    fn test_unused_block_variable() {
        let code = r#"
            o.onMissed <- function (_attacker, _skill) {
                local actor = this.getContainer().getActor();
                if (_attacker != null
                    && _attacker.isAlive()
                    && !_attacker.isAlliedWith(actor)
                    && _attacker.getTile().getDistanceTo(actor.getTile()) == 1
                    && !_attacker.getCurrentProperties().IsImmuneToDisarm
                    && !_skill.isIgnoringRiposte()
                    && _skill.m.IsWeaponSkill)
                {
                    ::FOTN.applyVulnerability(actor, _attacker, 1);
                    this.Sound.play(this.m.ParrySounds[::Math.rand(0, this.m.ParrySounds.len() - 1)], ::Const.Sound.Volume.Skill, actor.getPos());
                }
            }
        "#;
        let diagnostics = compute_symbol_diagnostics("test.nut", code).unwrap();
        assert!(!diagnostics.iter().any(|d| d.message.contains("'actor'")));
    }
}
