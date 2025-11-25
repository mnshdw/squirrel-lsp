use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::Node;

use crate::errors::AnalysisError;
use crate::helpers;
use crate::tree_sitter_helpers::{find_inherit_calls, get_node_text};

/// Information about a class member (method or field)
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub name: String,
    pub member_type: MemberType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberType {
    Method,
}

/// Information about a class
#[derive(Debug, Clone)]
pub struct ClassInfo {
    /// Class name (e.g., "barbarian_thrall")
    pub name: String,
    /// Parent class path (e.g., "scripts/entity/tactical/human")
    pub parent_path: Option<String>,
    /// Resolved parent class reference
    pub parent: Option<String>,
    /// Direct children classes
    pub children: Vec<String>,
    /// Members (methods and fields)
    pub members: Vec<MemberInfo>,
}

/// Global registry of all classes across all mods
#[derive(Debug, Default)]
pub struct ClassRegistry {
    /// Map of class name -> class info
    classes: HashMap<String, ClassInfo>,
    /// Map of class path -> class name (for resolving inherit paths)
    /// E.g., "scripts/entity/tactical/actor" -> "actor"
    pub path_to_class: HashMap<String, String>,
    /// Set of global identifiers (variables/functions defined at script level)
    globals: HashSet<String>,
}

impl ClassRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a class in the registry
    pub fn register_class(&mut self, class_info: ClassInfo) {
        self.classes.insert(class_info.name.clone(), class_info);
    }

    /// Register a global identifier
    pub fn register_global(&mut self, name: String) {
        self.globals.insert(name);
    }

    /// Check if an identifier is a known global
    #[allow(dead_code)] // May be useful for future tests
    pub fn is_global(&self, name: &str) -> bool {
        self.globals.contains(name)
    }

    /// Get all registered globals
    pub fn globals(&self) -> &HashSet<String> {
        &self.globals
    }

    /// Get class by name
    #[allow(dead_code)] // Used by tests
    pub fn get_class(&self, name: &str) -> Option<&ClassInfo> {
        self.classes.get(name)
    }

    /// Get class by path (e.g., "scripts/entity/tactical/actor")
    pub fn get_class_by_path(&self, path: &str) -> Option<&ClassInfo> {
        // Try with and without "scripts/" prefix
        let normalized = path.trim_end_matches(".nut");

        if let Some(class_name) = self.path_to_class.get(normalized) {
            return self.classes.get(class_name);
        }

        // Try adding "scripts/" prefix if not present
        if !normalized.starts_with("scripts/") {
            let with_scripts = format!("scripts/{}", normalized);
            if let Some(class_name) = self.path_to_class.get(&with_scripts) {
                return self.classes.get(class_name);
            }
        }

        // Try without "scripts/" prefix
        if let Some(stripped) = normalized.strip_prefix("scripts/")
            && let Some(class_name) = self.path_to_class.get(stripped)
        {
            return self.classes.get(class_name);
        }

        // Fallback: try to find by file path match
        for class in self.classes.values() {
            if let Some(parent) = &class.parent_path {
                let parent_normalized = parent.trim_end_matches(".nut");
                if parent_normalized == normalized
                    || parent_normalized.ends_with(normalized)
                    || normalized.ends_with(parent_normalized)
                {
                    return Some(class);
                }
            }
        }

        None
    }

    /// Get all descendants of a class
    #[allow(dead_code)] // Used by tests
    pub fn get_descendants(&self, class_name: &str) -> Vec<&ClassInfo> {
        let mut descendants = Vec::new();
        let mut queue = vec![class_name];
        let mut visited = HashSet::new();

        while let Some(name) = queue.pop() {
            if visited.contains(name) {
                continue;
            }
            visited.insert(name);

            if let Some(class) = self.classes.get(name) {
                for child_name in &class.children {
                    descendants.push(self.classes.get(child_name).unwrap());
                    queue.push(child_name);
                }
            }
        }

        descendants
    }

    /// Get all ancestors of a class
    pub fn get_ancestors(&self, class_name: &str) -> Vec<&ClassInfo> {
        let mut ancestors = Vec::new();
        let mut current = class_name;
        let mut visited = HashSet::new();

        while let Some(class) = self.classes.get(current) {
            if let Some(parent_name) = &class.parent {
                // Prevent infinite loop on circular inheritance
                if visited.contains(parent_name.as_str()) {
                    break;
                }
                visited.insert(parent_name.clone());

                if let Some(parent) = self.classes.get(parent_name) {
                    ancestors.push(parent);
                    current = &parent.name;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        ancestors
    }

    /// Check if one class is a descendant of another
    #[allow(dead_code)] // Used by tests
    pub fn is_descendant_of(&self, child: &str, parent: &str) -> bool {
        let ancestors = self.get_ancestors(child);
        ancestors.iter().any(|a| a.name == parent)
    }

    /// Get all members of a class (including inherited)
    pub fn get_all_members(&self, class_name: &str) -> Vec<MemberInfo> {
        let mut members = Vec::new();
        let mut member_map: HashMap<String, MemberInfo> = HashMap::new();

        // Collect ancestors (including self)
        let mut classes_to_check = vec![class_name.to_string()];
        classes_to_check.extend(
            self.get_ancestors(class_name)
                .into_iter()
                .map(|c| c.name.clone()),
        );

        // Walk from parent to child, so child members override parent
        for class_name in classes_to_check.iter().rev() {
            if let Some(class) = self.classes.get(class_name) {
                for member in &class.members {
                    member_map.insert(member.name.clone(), member.clone());
                }
            }
        }

        members.extend(member_map.into_values());
        members
    }

    /// Check if a class has a specific method (including inherited)
    pub fn has_method(&self, class_name: &str, method_name: &str) -> bool {
        let members = self.get_all_members(class_name);
        members
            .iter()
            .any(|m| m.name == method_name && m.member_type == MemberType::Method)
    }

    /// Build inheritance relationships after all classes are registered
    pub fn build_inheritance_graph(&mut self) {
        // First pass: resolve parent references
        let class_names: Vec<String> = self.classes.keys().cloned().collect();

        for class_name in class_names {
            if let Some(class) = self.classes.get(&class_name)
                && let Some(parent_path) = &class.parent_path
            {
                // Try to find parent by path
                let parent_class = self.get_class_by_path(parent_path);

                if let Some(parent) = parent_class {
                    let parent_name = parent.name.clone();

                    // Update parent reference
                    if let Some(class_mut) = self.classes.get_mut(&class_name) {
                        class_mut.parent = Some(parent_name.clone());
                    }

                    // Add child to parent
                    if let Some(parent_mut) = self.classes.get_mut(&parent_name)
                        && !parent_mut.children.contains(&class_name)
                    {
                        parent_mut.children.push(class_name.clone());
                    }
                }
            }
        }
    }

    /// Index a single file
    pub fn index_file(&mut self, file_path: &str, text: &str) -> Result<(), AnalysisError> {
        let tree = helpers::parse_squirrel(text)?;
        let root = tree.root_node();

        // Find all inherit calls (class definitions)
        let inherits = find_inherit_calls(root, text);

        for inherit_call in inherits {
            let class_info = ClassInfo {
                name: inherit_call.class_name.clone(),
                parent_path: Some(inherit_call.parent_path.clone()),
                parent: None, // Will be resolved later
                children: Vec::new(),
                members: extract_members_from_node(inherit_call.class_body, text),
            };

            self.register_class(class_info);

            // Also register path mapping
            let path_key = extract_path_from_file_path(file_path);
            self.path_to_class.insert(path_key, inherit_call.class_name);
        }

        // Extract global definitions (top-level `identifier <- ...`)
        self.extract_globals(root, text);

        Ok(())
    }

    /// Extract global variable/function definitions from top-level statements
    fn extract_globals(&mut self, root: Node, text: &str) {
        // Look for direct children of the script node that are update_expression with <-
        for child in root.children(&mut root.walk()) {
            if child.kind() == "update_expression" {
                let mut has_new_slot = false;
                let mut global_name = None;

                for node in child.children(&mut child.walk()) {
                    if node.kind() == "<-" {
                        has_new_slot = true;
                    } else if node.kind() == "identifier" && global_name.is_none() {
                        global_name = Some(get_node_text(node, text).to_string());
                    }
                }

                if has_new_slot {
                    if let Some(name) = global_name {
                        self.register_global(name);
                    }
                }
            }
        }
    }

    /// Find similar class paths (for "did you mean?" suggestions)
    pub fn find_similar_paths(&self, target: &str) -> Vec<String> {
        let mut candidates: Vec<(String, usize)> = self
            .path_to_class
            .keys()
            .map(|path| {
                let distance = levenshtein_distance(target, path);
                (path.clone(), distance)
            })
            .collect();

        candidates.sort_by_key(|(_, dist)| *dist);

        candidates
            .into_iter()
            .take(3)
            .filter(|(_, dist)| *dist < target.len() / 2) // Only suggest if reasonably close
            .map(|(path, _)| path)
            .collect()
    }

    /// Find similar method names in a class
    pub fn find_similar_methods(&self, class_name: &str, target: &str) -> Vec<String> {
        let members = self.get_all_members(class_name);
        let methods: Vec<String> = members
            .iter()
            .filter(|m| m.member_type == MemberType::Method)
            .map(|m| m.name.clone())
            .collect();

        let mut candidates: Vec<(String, usize)> = methods
            .iter()
            .map(|name| {
                let distance = levenshtein_distance(target, name);
                (name.clone(), distance)
            })
            .collect();

        candidates.sort_by_key(|(_, dist)| *dist);

        candidates
            .into_iter()
            .take(3)
            .filter(|(_, dist)| *dist < target.len() / 2)
            .map(|(name, _)| name)
            .collect()
    }
}

/// Extract method names from a class body node
fn extract_members_from_node(node: Node, text: &str) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                // Extract function name
                if let Some(name_node) = child.child_by_field_name("name") {
                    members.push(MemberInfo {
                        name: get_node_text(name_node, text).to_string(),
                        member_type: MemberType::Method,
                    });
                } else {
                    // Try to find first identifier
                    for c in child.children(&mut child.walk()) {
                        if c.kind() == "identifier" {
                            members.push(MemberInfo {
                                name: get_node_text(c, text).to_string(),
                                member_type: MemberType::Method,
                            });
                            break;
                        }
                    }
                }
            },
            "table_slot" => {
                // Check if value is a function (lambda or function declaration)
                if let Some(key) = child.child_by_field_name("key") {
                    let is_function = child.child_by_field_name("value").is_some_and(|v| {
                        v.kind() == "lambda_expression" || v.kind() == "function_declaration"
                    });

                    if is_function {
                        members.push(MemberInfo {
                            name: get_node_text(key, text).to_string(),
                            member_type: MemberType::Method,
                        });
                    }
                }
            },
            _ => {
                members.extend(extract_members_from_node(child, text));
            },
        }
    }

    members
}

/// Extract a normalized class path from a file path
/// E.g., "/path/to/scripts/entity/tactical/actor.nut" -> "entity/tactical/actor"
fn extract_path_from_file_path(file_path: &str) -> String {
    let path = Path::new(file_path);

    // Try to find "scripts/" in the path and take everything after it
    let path_str = path.to_str().unwrap_or("");

    if let Some(scripts_idx) = path_str.find("scripts/") {
        let after_scripts = &path_str[scripts_idx + 8..]; // len("scripts/") = 8
        return after_scripts.trim_end_matches(".nut").to_string();
    }

    // Fallback: just use the file stem
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Simple Levenshtein distance for suggestions
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    #[allow(clippy::needless_range_loop, reason = "Clarity")]
    for i in 0..=len1 {
        matrix[i][0] = i;
    }
    #[allow(clippy::needless_range_loop, reason = "Clarity")]
    for j in 0..=len2 {
        matrix[0][j] = j;
    }

    for (i, c1) in s1.chars().enumerate() {
        for (j, c2) in s2.chars().enumerate() {
            let cost = if c1 == c2 { 0 } else { 1 };
            matrix[i + 1][j + 1] = std::cmp::min(
                std::cmp::min(matrix[i][j + 1] + 1, matrix[i + 1][j] + 1),
                matrix[i][j] + cost,
            );
        }
    }

    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_get_class() {
        let mut registry = ClassRegistry::new();

        let class_info = ClassInfo {
            name: "actor".to_string(),
            parent_path: None,
            parent: None,
            children: Vec::new(),
            members: Vec::new(),
        };

        registry.register_class(class_info);

        assert!(registry.get_class("actor").is_some());
    }

    #[test]
    fn test_inheritance_graph() {
        let mut registry = ClassRegistry::new();

        // Register actor (base class)
        registry.register_class(ClassInfo {
            name: "actor".to_string(),
            parent_path: None,
            parent: None,
            children: Vec::new(),
            members: Vec::new(),
        });

        // Register path mapping
        registry
            .path_to_class
            .insert("entity/tactical/actor".to_string(), "actor".to_string());

        // Register human (inherits from actor)
        registry.register_class(ClassInfo {
            name: "human".to_string(),
            parent_path: Some("scripts/entity/tactical/actor".to_string()),
            parent: None,
            children: Vec::new(),
            members: Vec::new(),
        });

        // Build inheritance graph
        registry.build_inheritance_graph();

        // Check parent reference
        let human = registry.get_class("human").unwrap();
        assert_eq!(human.parent, Some("actor".to_string()));

        // Check children reference
        let actor = registry.get_class("actor").unwrap();
        assert!(actor.children.contains(&"human".to_string()));

        // Check is_descendant_of
        assert!(registry.is_descendant_of("human", "actor"));
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("actor", "actor"), 0);
        assert_eq!(levenshtein_distance("actor", "aktor"), 1);
        assert_eq!(levenshtein_distance("onDeath", "onDeth"), 1);
        assert_eq!(levenshtein_distance("abc", "def"), 3);
    }

    #[test]
    fn test_extract_path_from_file_path() {
        assert_eq!(
            extract_path_from_file_path("/path/to/scripts/entity/tactical/actor.nut"),
            "entity/tactical/actor"
        );
        assert_eq!(
            extract_path_from_file_path("scripts/items/weapons/sword.nut"),
            "items/weapons/sword"
        );
    }

    #[test]
    fn test_extract_globals() {
        let mut registry = ClassRegistry::new();
        let code = r#"
// Global definitions
inherit <- function(path, body) { /* ... */ }
myGlobal <- 42
anotherGlobal <- function(x) { return x + 1; }

// Class definition (should not be treated as global)
my_class <- inherit("scripts/base", {
    function foo() {
        return 1;
    }
});
"#;

        registry
            .index_file("/fake/path/test.nut", code)
            .expect("Should parse");

        // These should be registered as globals
        assert!(registry.is_global("inherit"));
        assert!(registry.is_global("myGlobal"));
        assert!(registry.is_global("anotherGlobal"));
        assert!(registry.is_global("my_class"));

        // These should not be globals
        assert!(!registry.is_global("foo"));
        assert!(!registry.is_global("path"));
    }
}
