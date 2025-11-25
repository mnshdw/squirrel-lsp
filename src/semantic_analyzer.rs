use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, Position, Range, SemanticToken,
};
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers;

// Semantic token type constants (matching LSP specification)
const TOKEN_TYPE_CLASS: u32 = 2;
const TOKEN_TYPE_ENUM: u32 = 3;
const TOKEN_TYPE_PARAMETER: u32 = 7;
const TOKEN_TYPE_VARIABLE: u32 = 8;
const TOKEN_TYPE_PROPERTY: u32 = 9;
const TOKEN_TYPE_FUNCTION: u32 = 12;
const TOKEN_TYPE_METHOD: u32 = 13;
const TOKEN_TYPE_KEYWORD: u32 = 15;
const TOKEN_TYPE_COMMENT: u32 = 17;
const TOKEN_TYPE_STRING: u32 = 18;
const TOKEN_TYPE_NUMBER: u32 = 19;
const TOKEN_TYPE_OPERATOR: u32 = 21;

// Squirrel built-in global identifiers (core language only)
// Based on sqbaselib.cpp from the official Squirrel 3 implementation
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
        // Special keywords that are always in scope
        "this",
        "Math",
        // Battle Brothers framework
        "inherit",  // Class inheritance function
    ])
});

/// Represents a scope (function, block, class, etc.) that can contain variable declarations
#[derive(Debug, Clone)]
struct Scope {
    /// Variables declared in this scope: name -> declaration position
    declarations: HashMap<String, Position>,
    /// Variables referenced in this scope
    references: HashSet<String>,
}

impl Scope {
    fn new() -> Self {
        Self {
            declarations: HashMap::new(),
            references: HashSet::new(),
        }
    }

    fn declare(&mut self, name: String, pos: Position) {
        self.declarations.insert(name, pos);
    }

    fn reference(&mut self, name: String) {
        self.references.insert(name);
    }

    fn contains(&self, name: &str) -> bool {
        self.declarations.contains_key(name)
    }

    /// Get unused variables (declared but not referenced)
    fn get_unused(&self) -> Vec<(&String, &Position)> {
        self.declarations
            .iter()
            .filter(|(name, _)| !self.references.contains(*name))
            .collect()
    }
}

/// Type-safe scope stack that prevents accidentally popping the global scope
#[derive(Debug)]
struct ScopeStack {
    /// The global scope (always present)
    global: Scope,
    /// Local scopes (can be pushed and popped)
    scopes: Vec<Scope>,
}

impl ScopeStack {
    fn new() -> Self {
        Self {
            global: Scope::new(),
            scopes: Vec::new(),
        }
    }

    fn push(&mut self) {
        self.scopes.push(Scope::new());
    }

    fn pop(&mut self) -> Option<Scope> {
        self.scopes.pop() // Can't accidentally pop global
    }

    fn current_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap_or(&mut self.global)
    }

    fn contains(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .chain(std::iter::once(&self.global))
            .any(|scope| scope.contains(name))
    }

    /// Record a variable reference in the scope where it's declared
    fn record_reference(&mut self, name: &str) {
        // Search from innermost to outermost scope
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains(name) {
                scope.reference(name.to_string());
                return;
            }
        }
        // Check global scope
        if self.global.contains(name) {
            self.global.reference(name.to_string());
        }
    }
}

/// Semantic analyzer that tracks variable declarations and references
pub struct SemanticAnalyzer<'a> {
    text: &'a str,
    /// Stack of scopes (most recent scope is at the end)
    scopes: ScopeStack,
    /// Diagnostics collected during analysis
    diagnostics: Vec<Diagnostic>,
    /// Optional set of known global identifiers from workspace indexing
    known_globals: Option<&'a HashSet<String>>,
}

impl<'a> SemanticAnalyzer<'a> {
    #[allow(dead_code)] // Used by compute_semantic_diagnostics for tests
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            scopes: ScopeStack::new(),
            diagnostics: Vec::new(),
            known_globals: None,
        }
    }

    pub fn with_globals(text: &'a str, globals: &'a HashSet<String>) -> Self {
        Self {
            text,
            scopes: ScopeStack::new(),
            diagnostics: Vec::new(),
            known_globals: Some(globals),
        }
    }

    /// Push a new scope onto the stack
    fn push_scope(&mut self) {
        self.scopes.push();
    }

    /// Pop the current scope from the stack and check for unused variables
    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            // Check for unused variables in the popped scope
            for (name, pos) in scope.get_unused() {
                self.report_unused(name, *pos);
            }
        }
    }

    /// Declare a variable in the current scope
    fn declare_variable(&mut self, name: &str, pos: Position) {
        self.scopes.current_mut().declare(name.to_string(), pos);
    }

    /// Check if a variable is declared in any scope
    fn is_declared(&self, name: &str) -> bool {
        self.scopes.contains(name)
    }

    /// Add a diagnostic for an undeclared variable
    fn report_undeclared(&mut self, name: &str, range: Range) {
        self.diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("squirrel-semantic".to_string()),
            message: format!("Undeclared variable '{}'", name),
            ..Diagnostic::default()
        });
    }

    /// Add a diagnostic for unused variables
    fn report_unused(&mut self, name: &str, pos: Position) {
        let end_pos = Position {
            line: pos.line,
            character: pos.character + name.len() as u32,
        };
        let range = Range::new(pos, end_pos);

        self.diagnostics.push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::HINT),
            source: Some("squirrel-semantic".to_string()),
            message: format!("Unused variable: `{}`", name),
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Diagnostic::default()
        });
    }

    /// Main analysis entry point
    pub fn analyze(mut self, root: Node) -> Vec<Diagnostic> {
        self.analyze_node(root);
        self.diagnostics
    }

    /// Recursively analyze a node and its children
    fn analyze_node(&mut self, node: Node) {
        let kind = node.kind();

        // Handle declarations that need to be in the parent scope BEFORE pushing new scope
        match kind {
            "function_declaration" | "class_declaration" => {
                // Declare the function/class name in the parent scope before creating its scope
                // The name is the first identifier child (not a named field)
                self.declare_first_identifier(node);
            },
            _ => {},
        }

        // Check if this node creates a new scope
        let creates_scope = matches!(
            kind,
            "function_declaration"
                | "lambda_expression"
                | "block"
                | "if_statement"
                | "while_statement"
                | "do_while_statement"
                | "for_statement"
                | "foreach_statement"
                | "switch_statement"
                | "try_statement"
                | "catch_statement"
                | "class_declaration"
                | "table" // Tables create a scope where slot keys are visible
        );

        if creates_scope {
            self.push_scope();
        }

        // For tables, pre-declare all table slot keys so they're visible to sibling functions
        if kind == "table" {
            self.declare_table_slots(node);
        }

        // Handle other variable declarations
        match kind {
            "local_declaration" | "var_statement" | "const_declaration" => {
                self.handle_declaration(node);
            },
            "update_expression" => {
                self.handle_new_slot_declaration(node);
            },
            "parameter" => {
                self.handle_parameter(node);
            },
            "foreach_statement" => {
                self.handle_foreach(node);
            },
            "catch_statement" => {
                self.handle_catch(node);
            },
            "identifier" => {
                self.handle_identifier(node);
            },
            _ => {},
        }

        // Recursively analyze children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.analyze_node(child);
        }

        if creates_scope {
            self.pop_scope();
        }
    }

    /// Handle variable declarations (local, var, const)
    fn handle_declaration(&mut self, node: Node) {
        // Try named field first, then fall back to first identifier child
        if let Some(identifier) = node.child_by_field_name("name") {
            let name = self.node_text(identifier).to_string();
            let pos = self.position_at(identifier.start_byte());
            self.declare_variable(&name, pos);
        } else {
            self.declare_first_identifier(node);
        }
    }

    /// Handle function parameters
    fn handle_parameter(&mut self, node: Node) {
        self.declare_first_identifier(node);
    }

    /// Handle foreach statements (auto-declares loop variables)
    fn handle_foreach(&mut self, node: Node) {
        // foreach can have: foreach (value in iterable) or foreach (key, value in iterable)
        // Declare all identifier children as loop variables
        for child in node.children(&mut node.walk()) {
            if child.kind() == "identifier" {
                let name = self.node_text(child).to_string();
                let pos = self.position_at(child.start_byte());
                self.declare_variable(&name, pos);
            }
        }
    }

    /// Handle catch statements (declares exception variable)
    fn handle_catch(&mut self, node: Node) {
        // catch (e) { ... } - declares the exception variable (first identifier only)
        self.declare_first_identifier(node);
    }

    /// Pre-declare all table slot keys in the current scope
    /// This makes keys like `m` and function names visible to sibling functions
    fn declare_table_slots(&mut self, table_node: Node) {
        // Find table_slots child, then iterate its table_slot children
        for child in table_node.children(&mut table_node.walk()) {
            if child.kind() == "table_slots" {
                for slot in child.children(&mut child.walk()) {
                    if slot.kind() == "table_slot" {
                        self.declare_table_slot_key(slot);
                    }
                }
            }
        }
    }

    /// Extract and declare the key from a table_slot
    fn declare_table_slot_key(&mut self, slot: Node) {
        for child in slot.children(&mut slot.walk()) {
            match child.kind() {
                "identifier" => {
                    // Simple slot: `m = {}` or `key = value`
                    let name = self.node_text(child).to_string();
                    let pos = self.position_at(child.start_byte());
                    self.declare_variable(&name, pos);
                    return;
                },
                "function_declaration" => {
                    // Function slot: `function foo() {}`
                    // The function name is the identifier child of the function_declaration
                    if let Some(name_node) = child
                        .children(&mut child.walk())
                        .find(|c| c.kind() == "identifier")
                    {
                        let name = self.node_text(name_node).to_string();
                        let pos = self.position_at(name_node.start_byte());
                        self.declare_variable(&name, pos);
                    }
                    return;
                },
                _ => {},
            }
        }
    }

    /// Handle new slot declarations (name <- value)
    fn handle_new_slot_declaration(&mut self, node: Node) {
        // Check if this is a `<-` (new slot) operation
        let mut has_new_slot_op = false;
        let mut first_identifier = None;

        for child in node.children(&mut node.walk()) {
            if child.kind() == "<-" {
                has_new_slot_op = true;
            } else if child.kind() == "identifier" && first_identifier.is_none() {
                first_identifier = Some(child);
            }
        }

        // Only treat as declaration if it's `identifier <- ...`
        if has_new_slot_op && let Some(name_node) = first_identifier {
            let name = self.node_text(name_node).to_string();
            let pos = self.position_at(name_node.start_byte());
            self.declare_variable(&name, pos);
        }
    }

    /// Handle identifier references (check if they're declared)
    fn handle_identifier(&mut self, node: Node) {
        // Skip if this identifier is part of a declaration (it's handled separately)
        if let Some(parent) = node.parent() {
            let parent_kind = parent.kind();

            // Skip identifiers that are the left-hand side of a new slot declaration
            // e.g., `character_trait <- inherit(...)` - skip character_trait
            if parent_kind == "update_expression" {
                // Check if this is the first identifier and there's a <- operator
                let mut is_first_id = false;
                let mut has_new_slot = false;
                for child in parent.children(&mut parent.walk()) {
                    if child.kind() == "identifier" && !is_first_id {
                        is_first_id = child.id() == node.id();
                    }
                    if child.kind() == "<-" {
                        has_new_slot = true;
                    }
                }
                if is_first_id && has_new_slot {
                    return;
                }
            }

            // Skip identifiers that are the first child of these declaration types
            // (they are the name being declared)
            if matches!(
                parent_kind,
                "local_declaration"
                    | "var_statement"
                    | "const_declaration"
                    | "function_declaration"
                    | "class_declaration"
                    | "parameter"
                    | "member_declaration"
                    | "enum_declaration"
                    | "table_slot"
            ) {
                // For these declarations, the first identifier child is the name being declared
                // We should skip checking it as a reference
                if let Some(first_child) = parent
                    .children(&mut parent.walk())
                    .find(|c| c.kind() == "identifier")
                    && first_child.id() == node.id()
                {
                    return;
                }
            }

            // Skip identifiers in member access (e.g., obj.property)
            if parent_kind == "deref_expression" {
                // In a deref expression like obj.property, we want to check obj but not property
                // The property is typically the last identifier child
                let is_not_first_identifier = parent
                    .children(&mut parent.walk())
                    .filter(|c| c.kind() == "identifier")
                    .position(|c| c.id() == node.id())
                    .is_some_and(|pos| pos > 0);

                if is_not_first_identifier {
                    return;
                }

                // Also check for field access patterns
                if let Some(prev) = node.prev_sibling()
                    && (prev.kind() == "." || prev.kind() == "::" || prev.kind() == "->")
                {
                    return;
                }
            }

            // Skip identifiers that are property names in table literals
            // e.g., { id = 3, name = "foo" } - skip "id" and "name"
            if parent_kind == "assignment_expression" {
                // Check if this identifier is the left-hand side of the assignment
                if let Some(left_field) = parent.child_by_field_name("left")
                    && left_field.id() == node.id()
                {
                    // Walk up the tree to check if we're inside a table
                    let mut ancestor = parent.parent();
                    while let Some(current) = ancestor {
                        if current.kind() == "table" {
                            return;
                        }
                        // Stop if we hit a scope boundary
                        if matches!(
                            current.kind(),
                            "function_declaration" | "lambda_expression" | "class_declaration"
                        ) {
                            break;
                        }
                        ancestor = current.parent();
                    }
                }
            }

            // Skip identifiers in global variable syntax (::var)
            // Global variables don't need declaration checking
            if parent_kind == "global_variable" {
                return;
            }
        }

        // This is a variable reference, check if it's declared
        let name = self.node_text(node).to_string();

        // Skip built-in/common global identifiers that are always available
        if self.is_builtin(&name) {
            return;
        }

        if !self.is_declared(&name) {
            let start = self.position_at(node.start_byte());
            let end = self.position_at(node.end_byte());
            let range = Range::new(start, end);
            self.report_undeclared(&name, range);
        } else {
            self.scopes.record_reference(&name);
        }
    }

    /// Check if a name is a built-in or known global identifier
    fn is_builtin(&self, name: &str) -> bool {
        BUILTINS.contains(name)
            || self.known_globals.is_some_and(|g| g.contains(name))
    }

    /// Get the text content of a node
    fn node_text(&self, node: Node) -> &str {
        node.utf8_text(self.text.as_bytes()).unwrap_or("")
    }

    /// Convert byte offset to LSP Position
    fn position_at(&self, byte_offset: usize) -> Position {
        helpers::position_at(self.text, byte_offset)
    }

    /// Find the first identifier child of a node
    fn find_first_identifier<'b>(&self, node: Node<'b>) -> Option<Node<'b>> {
        node.children(&mut node.walk())
            .find(|c| c.kind() == "identifier")
    }

    /// Declare the first identifier child of a node in the current scope
    fn declare_first_identifier(&mut self, node: Node) {
        if let Some(name_node) = self.find_first_identifier(node) {
            let name = self.node_text(name_node).to_string();
            let pos = self.position_at(name_node.start_byte());
            self.declare_variable(&name, pos);
        }
    }
}

/// Compute semantic diagnostics for the given text
#[allow(dead_code)] // Used by tests in code_actions.rs
pub fn compute_semantic_diagnostics(text: &str) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();
    let analyzer = SemanticAnalyzer::new(text);
    Ok(analyzer.analyze(root))
}

/// Compute semantic diagnostics with knowledge of globals from workspace indexing
pub fn compute_semantic_diagnostics_with_globals(
    text: &str,
    globals: &HashSet<String>,
) -> Result<Vec<Diagnostic>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();
    let analyzer = SemanticAnalyzer::with_globals(text, globals);
    Ok(analyzer.analyze(root))
}

pub fn compute_semantic_tokens(text: &str) -> Result<Vec<SemanticToken>, AnalysisError> {
    let tree = helpers::parse_squirrel(text)?;
    let root = tree.root_node();

    let mut tokens: Vec<(usize, usize, u32, u32)> = Vec::new();
    let mut cursor = root.walk();
    let mut visited_children = false;

    // Token modifier bit flags
    const MODIFIER_DECLARATION: u32 = 1 << 0; // 1
    const MODIFIER_READONLY: u32 = 1 << 2; // 4

    loop {
        let node = cursor.node();
        let kind = node.kind();

        // Process leaf nodes (including comments which are marked as extra)
        if node.child_count() == 0 {
            let (token_type, modifiers) = match kind {
                // Variables and identifiers
                "identifier" => {
                    // Check parent context for better classification
                    let parent = node.parent();
                    match parent.map(|p| p.kind()) {
                        Some("function_declaration") => {
                            // Function name declaration
                            (Some(TOKEN_TYPE_FUNCTION), MODIFIER_DECLARATION)
                        },
                        Some("class_declaration") => (Some(TOKEN_TYPE_CLASS), MODIFIER_DECLARATION),
                        Some("enum_declaration") => (Some(TOKEN_TYPE_ENUM), MODIFIER_DECLARATION),
                        Some("const_declaration") => {
                            // Constants are readonly
                            (
                                Some(TOKEN_TYPE_VARIABLE),
                                MODIFIER_DECLARATION | MODIFIER_READONLY,
                            )
                        },
                        Some("local_declaration") => {
                            // Local variable declaration
                            (Some(TOKEN_TYPE_VARIABLE), MODIFIER_DECLARATION)
                        },
                        Some("var_statement") => {
                            // Var statement declaration
                            (Some(TOKEN_TYPE_VARIABLE), MODIFIER_DECLARATION)
                        },
                        Some("parameter") => {
                            // Function parameters - use parameter type
                            (Some(TOKEN_TYPE_PARAMETER), MODIFIER_DECLARATION)
                        },
                        Some("member_declaration") => {
                            // Class member/property
                            (Some(TOKEN_TYPE_PROPERTY), MODIFIER_DECLARATION)
                        },
                        Some("deref_expression") => {
                            // Check if this deref is the function in a call_expression
                            if let Some(grandparent) = parent.and_then(|p| p.parent()) {
                                if grandparent.kind() == "call_expression" {
                                    // Check if parent deref is the 'function' field
                                    if let Some(p) = parent {
                                        if grandparent.child_by_field_name("function") == Some(p) {
                                            (Some(TOKEN_TYPE_METHOD), 0)
                                        } else {
                                            (Some(TOKEN_TYPE_PROPERTY), 0)
                                        }
                                    } else {
                                        (Some(TOKEN_TYPE_PROPERTY), 0)
                                    }
                                } else {
                                    (Some(TOKEN_TYPE_PROPERTY), 0)
                                }
                            } else {
                                (Some(TOKEN_TYPE_PROPERTY), 0)
                            }
                        },
                        Some("call_expression") => {
                            // Function call
                            (Some(TOKEN_TYPE_FUNCTION), 0)
                        },
                        _ => {
                            // Regular variable usage
                            (Some(TOKEN_TYPE_VARIABLE), 0)
                        },
                    }
                },

                // Literals
                "integer" | "float" => (Some(TOKEN_TYPE_NUMBER), 0),
                "string" | "string_content" | "verbatim_string" | "char" | "\"" => {
                    (Some(TOKEN_TYPE_STRING), 0)
                },
                "true" | "false" => (Some(TOKEN_TYPE_NUMBER), 0), // Boolean literals
                "null" => (Some(TOKEN_TYPE_KEYWORD), 0),

                // Comments
                "comment" => (Some(TOKEN_TYPE_COMMENT), 0),

                // Operators
                "=" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "<=>" | "+" | "-" | "*" | "/"
                | "%" | "++" | "--" | "&&" | "||" | "!" | "&" | "|" | "^" | "~" | "<<" | ">>"
                | ">>>" | "+=" | "-=" | "*=" | "/=" | "%=" | "<-" => (Some(TOKEN_TYPE_OPERATOR), 0),

                // Keywords
                "const" | "local" | "var" | "static" | "if" | "else" | "for" | "foreach"
                | "while" | "do" | "switch" | "case" | "default" | "break" | "continue"
                | "return" | "yield" | "try" | "catch" | "throw" | "in" | "instanceof"
                | "typeof" | "delete" | "clone" | "resume" | "extends" | "constructor"
                | "rawcall" | "function" | "class" | "enum" => (Some(TOKEN_TYPE_KEYWORD), 0),

                _ => (None, 0),
            };

            if let Some(token_type) = token_type {
                let start_byte = node.start_byte();
                let end_byte = node.end_byte();
                tokens.push((start_byte, end_byte, token_type, modifiers));
            }
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

    // Sort tokens by position
    tokens.sort_by_key(|(start, _, _, _)| *start);

    // Convert to LSP semantic tokens (delta-encoded)
    let mut semantic_tokens = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;

    for (start_byte, end_byte, token_type, modifiers) in tokens {
        let start_pos = helpers::position_at(text, start_byte);
        let end_pos = helpers::position_at(text, end_byte);

        if start_pos.line == end_pos.line {
            // Single-line token
            let length = end_pos.character - start_pos.character;

            let delta_line = start_pos.line - prev_line;
            let delta_start = if delta_line == 0 {
                start_pos.character - prev_col
            } else {
                start_pos.character
            };

            semantic_tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type,
                token_modifiers_bitset: modifiers,
            });

            prev_line = start_pos.line;
            prev_col = start_pos.character;
        } else {
            // Multi-line token - split into one token per line
            let token_text = &text[start_byte..end_byte];
            let mut current_line = start_pos.line;

            for (i, line_text) in token_text.split('\n').enumerate() {
                let line_start_col = if i == 0 { start_pos.character } else { 0 };
                let line_length = line_text.encode_utf16().count() as u32;

                let delta_line = current_line - prev_line;
                let delta_start = if delta_line == 0 {
                    line_start_col - prev_col
                } else {
                    line_start_col
                };

                semantic_tokens.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length: line_length,
                    token_type,
                    token_modifiers_bitset: modifiers,
                });

                prev_line = current_line;
                prev_col = line_start_col + line_length;
                current_line += 1;
            }
        }
    }

    Ok(semantic_tokens)
}
