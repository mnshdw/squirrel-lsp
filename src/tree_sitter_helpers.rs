use tree_sitter::Node;

/// Represents an inherit() call in Squirrel code
/// Pattern: identifier <- inherit("path/to/parent", { body })
#[derive(Debug, Clone)]
pub struct InheritCall<'tree> {
    /// The full assignment node (identifier <- inherit(...))
    pub node: Node<'tree>,
    /// The class name being defined (left side of <-)
    pub class_name: String,
    /// The parent class path (first argument to inherit)
    pub parent_path: String,
    /// Location of the parent path string literal
    pub parent_path_node: Node<'tree>,
    /// The class body (second argument to inherit)
    pub class_body: Node<'tree>,
}

/// Represents a hook call in Squirrel code
/// Patterns:
/// - ::mods_hookExactClass("path", function(o) {...})
/// - ::mods_hookBaseClass("path", function(o) {...})
/// - ::mods_hookDescendants("path", function(o) {...})
/// - ::ModHook.hookTree("path", function(q) {...})
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookType {
    Exact,
    Base,
    Descendants,
    Tree,
}

#[derive(Debug, Clone)]
pub struct HookCall<'tree> {
    /// The full hook call expression node
    pub node: Node<'tree>,
    /// Type of hook (exact, base, descendants, tree)
    pub hook_type: HookType,
    /// The target class path (first argument)
    pub target_path: String,
    /// Location of the target path string literal
    pub target_path_node: Node<'tree>,
    /// The hook function (second argument)
    pub hook_function: Node<'tree>,
}

/// Represents a member access pattern (o.method or o.m.field)
#[derive(Debug, Clone)]
pub struct MemberAccess<'tree> {
    /// The base object being accessed (e.g., "o")
    pub base: String,
    /// The member name being accessed (e.g., "method" or "field")
    pub member_name: String,
    /// Location of the member name
    pub member_node: Node<'tree>,
}

/// Get the text content of a node
pub fn get_node_text<'a>(node: Node, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

/// Extract a string literal's content (without quotes)
pub fn extract_string_literal(node: Node, text: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }

    // Get the string content (child of string node)
    for child in node.children(&mut node.walk()) {
        if child.kind() == "string_content" {
            return Some(get_node_text(child, text).to_string());
        }
    }

    // Fallback: remove quotes from the string node text
    let s = get_node_text(node, text);
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        return Some(s[1..s.len() - 1].to_string());
    }

    None
}

/// Find all inherit() calls in the AST
pub fn find_inherit_calls<'tree>(root: Node<'tree>, text: &str) -> Vec<InheritCall<'tree>> {
    let mut calls = Vec::new();
    let mut cursor = root.walk();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        // Look for update_expression with <- operator
        // Pattern: identifier <- inherit("path", {...})
        if node.kind() == "update_expression" {
            // Check if operator is <-
            let mut has_new_slot_op = false;
            let mut class_name = String::new();
            let mut call_expr = None;

            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" && class_name.is_empty() {
                    class_name = get_node_text(child, text).to_string();
                } else if child.kind() == "<-" {
                    has_new_slot_op = true;
                } else if child.kind() == "call_expression" {
                    call_expr = Some(child);
                }
            }

            if has_new_slot_op
                && !class_name.is_empty()
                && let Some(right) = call_expr
            {
                // Check if function name is "inherit"
                // The function might be an identifier or inside the call_expression
                let mut func_name = String::new();
                for child in right.children(&mut right.walk()) {
                    if child.kind() == "identifier" {
                        func_name = get_node_text(child, text).to_string();
                        break;
                    }
                }

                if func_name == "inherit" {
                    // Extract arguments from call_args node
                    for child in right.children(&mut right.walk()) {
                        if child.kind() == "call_args" {
                            let arg_children: Vec<_> = child
                                .children(&mut child.walk())
                                .filter(|c| c.kind() != "," && !c.kind().is_empty())
                                .collect();

                            if arg_children.len() >= 2 {
                                // First argument: parent path (string)
                                let parent_path_node = arg_children[0];
                                if let Some(parent_path) =
                                    extract_string_literal(parent_path_node, text)
                                {
                                    // Second argument: class body (table/object)
                                    let class_body = arg_children[1];

                                    calls.push(InheritCall {
                                        node,
                                        class_name: class_name.clone(),
                                        parent_path,
                                        parent_path_node,
                                        class_body,
                                    });
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }

        // Add children to stack for processing
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    // Sort by source position to maintain order
    calls.sort_by_key(|call| call.node.start_byte());
    calls
}

/// Find all hook calls in the AST
pub fn find_hook_calls<'tree>(root: Node<'tree>, text: &str) -> Vec<HookCall<'tree>> {
    let mut calls = Vec::new();
    let mut cursor = root.walk();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        // Look for call_expression nodes
        if node.kind() == "call_expression" {
            // Get the function name - it might be in a global_variable or identifier node
            let mut func_text = String::new();

            for child in node.children(&mut cursor) {
                if child.kind() == "global_variable" {
                    // Extract identifier from global_variable (::identifier)
                    for gc in child.children(&mut child.walk()) {
                        if gc.kind() == "identifier" {
                            func_text = get_node_text(gc, text).to_string();
                            break;
                        }
                    }
                    break;
                } else if child.kind() == "identifier" {
                    func_text = get_node_text(child, text).to_string();
                    break;
                } else if child.kind() == "deref_expression" {
                    // For ModHook.hookTree pattern
                    func_text = get_node_text(child, text).to_string();
                    break;
                }
            }

            // Determine hook type
            let hook_type = if func_text.contains("mods_hookExactClass")
                || func_text == "mods_hookExactClass"
            {
                Some(HookType::Exact)
            } else if func_text.contains("mods_hookBaseClass") || func_text == "mods_hookBaseClass"
            {
                Some(HookType::Base)
            } else if func_text.contains("mods_hookDescendants")
                || func_text == "mods_hookDescendants"
            {
                Some(HookType::Descendants)
            } else if func_text.contains("hookTree") {
                Some(HookType::Tree)
            } else {
                None
            };

            if let Some(hook_type) = hook_type {
                // Extract arguments from call_args node
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "call_args" {
                        let arg_children: Vec<_> = child
                            .children(&mut child.walk())
                            .filter(|c| c.kind() != "," && !c.kind().is_empty())
                            .collect();

                        if arg_children.len() >= 2 {
                            // First argument: target path (string)
                            let target_path_node = arg_children[0];
                            if let Some(target_path) =
                                extract_string_literal(target_path_node, text)
                            {
                                // Second argument: hook function
                                let hook_function = arg_children[1];

                                calls.push(HookCall {
                                    node,
                                    hook_type,
                                    target_path,
                                    target_path_node,
                                    hook_function,
                                });
                            }
                        }
                        break;
                    }
                }
            }
        }

        // Add children to stack for processing
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }

    // Sort by source position to maintain order
    calls.sort_by_key(|call| call.node.start_byte());
    calls
}

/// Find all member access patterns in a node (e.g., o.method, o.m.field)
pub fn find_member_accesses<'tree>(node: Node<'tree>, text: &str) -> Vec<MemberAccess<'tree>> {
    let mut accesses = Vec::new();
    let mut cursor = node.walk();
    let mut stack = vec![node];

    while let Some(current) = stack.pop() {
        // Look for deref_expression (member access with .)
        if current.kind() == "deref_expression" {
            // Pattern: base.member
            // The deref_expression has multiple children, we need to find the identifiers
            let children: Vec<_> = current.children(&mut cursor).collect();

            // Typically: identifier, ".", identifier
            // Or: identifier, ".", identifier, ".", identifier (chained access)

            // Extract base (first identifier before first ".")
            let mut base_opt = None;
            let mut member_opt = None;

            for child in children.iter() {
                if child.kind() == "identifier" && base_opt.is_none() {
                    base_opt = Some(get_node_text(*child, text).to_string());
                } else if child.kind() == "identifier" && base_opt.is_some() {
                    // This is a member access
                    member_opt = Some((get_node_text(*child, text).to_string(), *child));
                    break;
                } else if child.kind() == "deref_expression" {
                    // Nested deref, handle recursively
                    stack.push(*child);
                }
            }

            if let (Some(base), Some((member_name, member_node))) = (base_opt, member_opt) {
                accesses.push(MemberAccess {
                    base,
                    member_name,
                    member_node,
                });
            }
        }

        // Add children to stack for processing (excluding already processed derefs)
        for child in current.children(&mut cursor) {
            if child.kind() != "deref_expression" || current.kind() != "deref_expression" {
                stack.push(child);
            }
        }
    }

    accesses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::parse_squirrel;

    #[test]
    fn test_extract_string_literal() {
        let code = r#"
            local s1 = "hello";
            local s2 = "scripts/entity/tactical/actor";
        "#;

        let tree = parse_squirrel(code).unwrap();
        let root = tree.root_node();

        // Find string nodes
        let mut strings = Vec::new();

        fn collect_strings<'tree>(node: Node<'tree>, strings: &mut Vec<Node<'tree>>) {
            if node.kind() == "string" {
                strings.push(node);
            }
            for child in node.children(&mut node.walk()) {
                collect_strings(child, strings);
            }
        }

        collect_strings(root, &mut strings);

        assert_eq!(strings.len(), 2);
        assert_eq!(
            extract_string_literal(strings[0], code),
            Some("hello".to_string())
        );
        assert_eq!(
            extract_string_literal(strings[1], code),
            Some("scripts/entity/tactical/actor".to_string())
        );
    }

    #[test]
    fn test_find_inherit_calls() {
        let code = r#"
            barbarian_thrall <- inherit("scripts/entity/tactical/human", {
                m = {},
                function create() {
                    human.create();
                }
            });
        "#;

        let tree = parse_squirrel(code).unwrap();
        let root = tree.root_node();

        let inherits = find_inherit_calls(root, code);

        assert_eq!(inherits.len(), 1);
        assert_eq!(inherits[0].class_name, "barbarian_thrall");
        assert_eq!(inherits[0].parent_path, "scripts/entity/tactical/human");
    }

    #[test]
    fn test_find_hook_calls() {
        let code = r#"
            ::mods_hookExactClass("entity/tactical/actor", function(o) {
                local onDeath = o.onDeath;
            });

            ::mods_hookBaseClass("skills/skill", function(o) {
                while(!("m" in o && "ID" in o.m)) o=o[o.SuperName];
            });

            ::ModHook.hookTree("scripts/items/shields/shield", function(q) {
                q.onShieldHit = @(__original) function(_attacker, _skill) {
                    __original(_attacker, _skill);
                };
            });
        "#;

        let tree = parse_squirrel(code).unwrap();
        let root = tree.root_node();

        let hooks = find_hook_calls(root, code);

        assert_eq!(hooks.len(), 3);

        assert_eq!(hooks[0].hook_type, HookType::Exact);
        assert_eq!(hooks[0].target_path, "entity/tactical/actor");

        assert_eq!(hooks[1].hook_type, HookType::Base);
        assert_eq!(hooks[1].target_path, "skills/skill");

        assert_eq!(hooks[2].hook_type, HookType::Tree);
        assert_eq!(hooks[2].target_path, "scripts/items/shields/shield");
    }

    #[test]
    fn test_find_member_accesses() {
        let code = r#"
            function test(o) {
                local onDeath = o.onDeath;
                local id = o.m.ID;
                o.setFatigue(100);
            }
        "#;

        let tree = parse_squirrel(code).unwrap();
        let root = tree.root_node();

        let accesses = find_member_accesses(root, code);

        // Should find: o.onDeath, o.m, m.ID, o.setFatigue
        assert!(accesses.len() >= 3);

        // Check that we found some expected accesses
        let member_names: Vec<_> = accesses.iter().map(|a| a.member_name.as_str()).collect();
        assert!(member_names.contains(&"onDeath"));
        assert!(member_names.contains(&"setFatigue"));
    }
}
