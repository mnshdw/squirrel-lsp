//! Workspace indexing for Squirrel files.
//!
//! The workspace is indexed by script path (e.g., "statistics/statistics_manager"),
//! making lookups trivial for hook validation and inheritance resolution.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::bb_support::{find_inherit_calls, get_node_text};
use crate::errors::AnalysisError;
use crate::helpers;

/// Information about a class member (method or field)
#[derive(Debug, Clone)]
pub struct MemberInfo {
    pub name: String,
    pub member_type: MemberType,
    pub line: u32,
    pub column: u32,
}

/// The type of a class member.
///
/// Currently only methods are tracked, but this enum exists to support
/// future extensions like fields and properties.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MemberType {
    /// A method (function defined in a table/class)
    Method,
}

/// A file entry in the workspace
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Actual file path on disk
    pub file_path: PathBuf,
    /// Script path (e.g., "entity/tactical/actor")
    pub script_path: String,
    /// Name of the main definition (usually matches file stem)
    pub name: String,
    /// For classes: the parent script path (e.g., "entity/tactical/actor")
    pub parent_path: Option<String>,
    /// Resolved parent script path (normalized, after building graph)
    pub parent: Option<String>,
    /// Direct children script paths
    pub children: Vec<String>,
    /// Members (methods) defined in this file
    pub members: Vec<MemberInfo>,
}

/// The workspace indexed by script path.
///
/// Script paths are relative to `scripts/` and without the `.nut` extension.
/// E.g., "statistics/statistics_manager" for `scripts/statistics/statistics_manager.nut`
#[derive(Debug, Default)]
pub struct Workspace {
    /// Script path -> file entry
    files: HashMap<String, FileEntry>,
    /// Global identifiers defined across all files
    globals: HashSet<String>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a file entry by script path
    pub fn get(&self, script_path: &str) -> Option<&FileEntry> {
        // Try exact match first
        if let Some(entry) = self.files.get(script_path) {
            return Some(entry);
        }

        // Try with/without "scripts/" prefix
        let normalized = script_path
            .trim_start_matches("scripts/")
            .trim_end_matches(".nut");

        self.files.get(normalized)
    }

    /// Get a mutable file entry by script path
    fn get_mut(&mut self, script_path: &str) -> Option<&mut FileEntry> {
        let normalized = script_path
            .trim_start_matches("scripts/")
            .trim_end_matches(".nut");

        self.files.get_mut(normalized)
    }

    /// Check if a script path exists in the workspace
    pub fn contains(&self, script_path: &str) -> bool {
        self.get(script_path).is_some()
    }

    /// Get all files in the workspace
    pub fn files(&self) -> &HashMap<String, FileEntry> {
        &self.files
    }

    /// Register a global identifier
    pub fn register_global(&mut self, name: String) {
        self.globals.insert(name);
    }

    /// Get all registered globals
    pub fn globals(&self) -> &HashSet<String> {
        &self.globals
    }

    /// Get all members of a file (including inherited members)
    pub fn get_all_members(&self, script_path: &str) -> Vec<MemberInfo> {
        let mut members = Vec::new();
        let mut member_map: HashMap<String, MemberInfo> = HashMap::new();

        // Collect the file and all ancestors
        let mut paths_to_check = vec![script_path.to_string()];
        paths_to_check.extend(
            self.get_ancestors(script_path)
                .into_iter()
                .map(|e| self.get_script_path(e)),
        );

        // Walk from parent to child, so child members override parent
        for path in paths_to_check.iter().rev() {
            if let Some(entry) = self.get(path) {
                for member in &entry.members {
                    member_map.insert(member.name.clone(), member.clone());
                }
            }
        }

        members.extend(member_map.into_values());
        members
    }

    /// Check if a file has a specific method (including inherited)
    pub fn has_method(&self, script_path: &str, method_name: &str) -> bool {
        let members = self.get_all_members(script_path);
        members
            .iter()
            .any(|m| m.name == method_name && m.member_type == MemberType::Method)
    }

    /// Find where a method is defined, searching current class and ancestors.
    /// Returns (file_path, line, column) if found.
    pub fn find_method_definition(
        &self,
        script_path: &str,
        method_name: &str,
    ) -> Option<(&PathBuf, u32, u32)> {
        // First check the current class
        if let Some(entry) = self.get(script_path)
            && let Some(member) = entry
                .members
                .iter()
                .find(|m| m.name == method_name && m.member_type == MemberType::Method)
        {
            return Some((&entry.file_path, member.line, member.column));
        }

        // Then check ancestors
        for ancestor in self.get_ancestors(script_path) {
            if let Some(member) = ancestor
                .members
                .iter()
                .find(|m| m.name == method_name && m.member_type == MemberType::Method)
            {
                return Some((&ancestor.file_path, member.line, member.column));
            }
        }

        None
    }

    /// Find a method definition by name across all files in workspace
    pub fn find_method_anywhere(&self, method_name: &str) -> Vec<(&PathBuf, u32, u32, &str)> {
        let mut results = Vec::new();
        for (script_path, entry) in &self.files {
            for member in &entry.members {
                if member.name == method_name && member.member_type == MemberType::Method {
                    results.push((
                        &entry.file_path,
                        member.line,
                        member.column,
                        script_path.as_str(),
                    ));
                }
            }
        }
        results
    }

    /// Get all ancestors of a file (walking up the inheritance chain)
    pub fn get_ancestors(&self, script_path: &str) -> Vec<&FileEntry> {
        let mut ancestors = Vec::new();
        let mut current_path = script_path.to_string();
        let mut visited = HashSet::new();

        while let Some(entry) = self.get(&current_path) {
            if let Some(parent_path) = &entry.parent {
                if visited.contains(parent_path) {
                    break; // Prevent infinite loop on circular inheritance
                }
                visited.insert(parent_path.clone());

                if let Some(parent_entry) = self.get(parent_path) {
                    ancestors.push(parent_entry);
                    current_path = parent_path.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        ancestors
    }

    /// Get script path for a file entry
    fn get_script_path(&self, entry: &FileEntry) -> String {
        entry.script_path.clone()
    }

    /// Index a single file into the workspace
    pub fn index_file(&mut self, file_path: &Path, content: &str) -> Result<(), AnalysisError> {
        let script_path = extract_script_path(file_path);
        if script_path.is_empty() {
            return Ok(()); // Skip files not under scripts/
        }

        let tree = helpers::parse_squirrel(content)?;
        let root = tree.root_node();

        // Try to find inherit() calls first (class definitions)
        let inherits = find_inherit_calls(root, content);

        if let Some(inherit_call) = inherits.into_iter().next() {
            // This is a class file
            let parent_path = normalize_script_path(&inherit_call.parent_path);

            let entry = FileEntry {
                file_path: file_path.to_path_buf(),
                script_path: script_path.clone(),
                name: inherit_call.class_name,
                parent_path: Some(parent_path),
                parent: None, // Resolved later
                children: Vec::new(),
                members: extract_members_from_table(inherit_call.class_body, content),
            };

            self.files.insert(script_path, entry);
        } else {
            // Look for global table definition matching file name
            let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

            if let Some((name, table_node)) = find_global_table(root, content, file_stem) {
                let entry = FileEntry {
                    file_path: file_path.to_path_buf(),
                    script_path: script_path.clone(),
                    name,
                    parent_path: None,
                    parent: None,
                    children: Vec::new(),
                    members: extract_members_from_table(table_node, content),
                };

                self.files.insert(script_path, entry);
            }
        }

        // Extract global definitions
        self.extract_globals(root, content);

        Ok(())
    }

    /// Build inheritance relationships after all files are indexed
    pub fn build_inheritance_graph(&mut self) {
        let script_paths: Vec<String> = self.files.keys().cloned().collect();

        for script_path in script_paths {
            if let Some(entry) = self.files.get(&script_path)
                && let Some(parent_path) = entry.parent_path.clone()
            {
                // Normalize and resolve parent
                let normalized_parent = normalize_script_path(&parent_path);

                if self.contains(&normalized_parent) {
                    // Update parent reference
                    if let Some(entry_mut) = self.files.get_mut(&script_path) {
                        entry_mut.parent = Some(normalized_parent.clone());
                    }

                    // Add child to parent
                    if let Some(parent_mut) = self.get_mut(&normalized_parent)
                        && !parent_mut.children.contains(&script_path)
                    {
                        parent_mut.children.push(script_path.clone());
                    }
                }
            }
        }
    }

    /// Extract global variable definitions from a file
    fn extract_globals(&mut self, root: Node, text: &str) {
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

                if has_new_slot && let Some(name) = global_name {
                    self.register_global(name);
                }
            }
        }
    }

    /// Find similar script paths for "did you mean?" suggestions
    pub fn find_similar_paths(&self, target: &str) -> Vec<String> {
        let mut candidates: Vec<(String, usize)> = self
            .files
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
            .filter(|(_, dist)| *dist < target.len() / 2)
            .map(|(path, _)| path)
            .collect()
    }

    /// Find similar method names in a file
    pub fn find_similar_methods(&self, script_path: &str, target: &str) -> Vec<String> {
        let members = self.get_all_members(script_path);
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

/// Extract script path from a file path.
/// E.g., "/path/to/scripts/statistics/statistics_manager.nut" -> "statistics/statistics_manager"
fn extract_script_path(file_path: &Path) -> String {
    let path_str = file_path.to_string_lossy();

    if let Some(scripts_idx) = path_str.find("scripts/") {
        let after_scripts = &path_str[scripts_idx + 8..]; // len("scripts/") = 8
        return after_scripts.trim_end_matches(".nut").to_string();
    }

    // Not under scripts/, return empty
    String::new()
}

/// Normalize a script path (remove "scripts/" prefix and ".nut" suffix)
fn normalize_script_path(path: &str) -> String {
    path.trim_start_matches("scripts/")
        .trim_end_matches(".nut")
        .to_string()
}

/// Find a global table definition that matches the file name.
/// Also searches inside ERROR nodes for partial parse results.
fn find_global_table<'tree>(
    root: Node<'tree>,
    text: &str,
    file_stem: &str,
) -> Option<(String, Node<'tree>)> {
    fn search_node<'tree>(
        node: Node<'tree>,
        text: &str,
        file_stem: &str,
    ) -> Option<(String, Node<'tree>)> {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "update_expression" {
                let mut has_new_slot = false;
                let mut identifier_name = None;
                let mut table_node = None;

                for n in child.children(&mut child.walk()) {
                    match n.kind() {
                        "<-" => has_new_slot = true,
                        "identifier" if identifier_name.is_none() => {
                            identifier_name = Some(get_node_text(n, text).to_string());
                        },
                        "table" => table_node = Some(n),
                        _ => {},
                    }
                }

                if has_new_slot
                    && let Some(name) = identifier_name
                    && let Some(table) = table_node
                    && name == file_stem
                {
                    return Some((name, table));
                }
            } else if child.kind() == "ERROR" {
                // Search inside ERROR nodes for partial parse results (BB syntax extensions)
                if let Some(result) = search_node(child, text, file_stem) {
                    return Some(result);
                }
            }
        }
        None
    }

    // Also check if root itself contains the pattern (for ERROR root nodes)
    if root.kind() == "ERROR" {
        // Look for identifier <- table pattern directly in ERROR children
        let mut has_new_slot = false;
        let mut identifier_name = None;
        let mut table_node = None;

        for child in root.children(&mut root.walk()) {
            match child.kind() {
                "<-" => has_new_slot = true,
                "identifier" if identifier_name.is_none() => {
                    identifier_name = Some(get_node_text(child, text).to_string());
                },
                "table" | "{" => {
                    // When parsing fails, the table might just be "{"
                    if table_node.is_none() {
                        table_node = Some(child);
                    }
                },
                _ => {},
            }
        }

        if has_new_slot
            && let Some(name) = identifier_name
            && table_node.is_some()
            && name == file_stem
        {
            // For ERROR nodes, we can't extract members properly, but we can at least index the file
            return Some((name, root));
        }
    }

    search_node(root, text, file_stem)
}

/// Extract members from a table node
fn extract_members_from_table(node: Node, text: &str) -> Vec<MemberInfo> {
    let mut members = Vec::new();

    for child in node.children(&mut node.walk()) {
        match child.kind() {
            "function_declaration" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let start = name_node.start_position();
                    members.push(MemberInfo {
                        name: get_node_text(name_node, text).to_string(),
                        member_type: MemberType::Method,
                        line: start.row as u32,
                        column: start.column as u32,
                    });
                } else {
                    for c in child.children(&mut child.walk()) {
                        if c.kind() == "identifier" {
                            let start = c.start_position();
                            members.push(MemberInfo {
                                name: get_node_text(c, text).to_string(),
                                member_type: MemberType::Method,
                                line: start.row as u32,
                                column: start.column as u32,
                            });
                            break;
                        }
                    }
                }
            },
            "table_slot" => {
                // Check for `key = function() {}` pattern
                if let Some(key) = child.child_by_field_name("key") {
                    let is_function = child.child_by_field_name("value").is_some_and(|v| {
                        v.kind() == "lambda_expression" || v.kind() == "function_declaration"
                    });

                    if is_function {
                        let start = key.start_position();
                        members.push(MemberInfo {
                            name: get_node_text(key, text).to_string(),
                            member_type: MemberType::Method,
                            line: start.row as u32,
                            column: start.column as u32,
                        });
                    }
                } else {
                    // Handle `function name() {}` syntax inside tables
                    for slot_child in child.children(&mut child.walk()) {
                        if slot_child.kind() == "function_declaration" {
                            if let Some(name_node) = slot_child.child_by_field_name("name") {
                                let start = name_node.start_position();
                                members.push(MemberInfo {
                                    name: get_node_text(name_node, text).to_string(),
                                    member_type: MemberType::Method,
                                    line: start.row as u32,
                                    column: start.column as u32,
                                });
                            } else {
                                for c in slot_child.children(&mut slot_child.walk()) {
                                    if c.kind() == "identifier" {
                                        let start = c.start_position();
                                        members.push(MemberInfo {
                                            name: get_node_text(c, text).to_string(),
                                            member_type: MemberType::Method,
                                            line: start.row as u32,
                                            column: start.column as u32,
                                        });
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            },
            _ => {
                members.extend(extract_members_from_table(child, text));
            },
        }
    }

    members
}

/// Simple Levenshtein distance for suggestions
fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();
    let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

    // Initialize first column
    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    // Initialize first row
    for (j, cell) in matrix[0].iter_mut().enumerate() {
        *cell = j;
    }

    for (i, c1) in s1.chars().enumerate() {
        for (j, c2) in s2.chars().enumerate() {
            let cost = usize::from(c1 != c2);
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[len1][len2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_script_path() {
        assert_eq!(
            extract_script_path(Path::new(
                "/path/to/scripts/statistics/statistics_manager.nut"
            )),
            "statistics/statistics_manager"
        );
        assert_eq!(
            extract_script_path(Path::new("scripts/entity/tactical/actor.nut")),
            "entity/tactical/actor"
        );
        assert_eq!(extract_script_path(Path::new("/some/other/path.nut")), "");
    }

    #[test]
    fn test_normalize_script_path() {
        assert_eq!(
            normalize_script_path("scripts/entity/tactical/actor"),
            "entity/tactical/actor"
        );
        assert_eq!(
            normalize_script_path("entity/tactical/actor.nut"),
            "entity/tactical/actor"
        );
        assert_eq!(
            normalize_script_path("scripts/entity/tactical/actor.nut"),
            "entity/tactical/actor"
        );
    }

    #[test]
    fn test_index_global_table() {
        let mut workspace = Workspace::new();
        let content = r#"
statistics_manager <-
{
    m = { Flags = null }

    function getFlags() { return m.Flags; }
    function onSerialize(_out) { m.Flags.onSerialize(_out); }
}
"#;

        workspace
            .index_file(
                Path::new("/path/to/scripts/statistics/statistics_manager.nut"),
                content,
            )
            .expect("Should parse");

        // Should be findable by script path
        let entry = workspace.get("statistics/statistics_manager");
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.name, "statistics_manager");

        // Methods should be extracted
        let method_names: Vec<&str> = entry.members.iter().map(|m| m.name.as_str()).collect();
        assert!(method_names.contains(&"getFlags"));
        assert!(method_names.contains(&"onSerialize"));
    }

    #[test]
    fn test_index_class_with_inherit() {
        let mut workspace = Workspace::new();

        // First index the parent
        let actor_content = r#"
this.actor <- this.inherit("scripts/entity/tactical/base", {
    function onDeath() {}
    function setFatigue(_f) {}
});
"#;
        workspace
            .index_file(
                Path::new("/path/to/scripts/entity/tactical/actor.nut"),
                actor_content,
            )
            .expect("Should parse");

        // Then index the child
        let human_content = r#"
this.human <- this.inherit("scripts/entity/tactical/actor", {
    function onTurnStart() {}
});
"#;
        workspace
            .index_file(
                Path::new("/path/to/scripts/entity/tactical/human.nut"),
                human_content,
            )
            .expect("Should parse");

        // Build inheritance graph
        workspace.build_inheritance_graph();

        // Check parent-child relationships
        let human = workspace.get("entity/tactical/human").unwrap();
        assert_eq!(human.parent, Some("entity/tactical/actor".to_string()));

        let actor = workspace.get("entity/tactical/actor").unwrap();
        assert!(
            actor
                .children
                .contains(&"entity/tactical/human".to_string())
        );
    }

    #[test]
    fn test_has_method_with_inheritance() {
        let mut workspace = Workspace::new();

        let actor_content = r#"
this.actor <- this.inherit("scripts/entity/tactical/base", {
    function onDeath() {}
});
"#;
        workspace
            .index_file(
                Path::new("/path/to/scripts/entity/tactical/actor.nut"),
                actor_content,
            )
            .unwrap();

        let human_content = r#"
this.human <- this.inherit("scripts/entity/tactical/actor", {
    function onTurnStart() {}
});
"#;
        workspace
            .index_file(
                Path::new("/path/to/scripts/entity/tactical/human.nut"),
                human_content,
            )
            .unwrap();

        workspace.build_inheritance_graph();

        // human should have onTurnStart directly
        assert!(workspace.has_method("entity/tactical/human", "onTurnStart"));

        // human should inherit onDeath from actor
        assert!(workspace.has_method("entity/tactical/human", "onDeath"));

        // actor should have onDeath
        assert!(workspace.has_method("entity/tactical/actor", "onDeath"));

        // actor should NOT have onTurnStart
        assert!(!workspace.has_method("entity/tactical/actor", "onTurnStart"));
    }

    #[test]
    fn test_index_multiline_global_table() {
        // Test the pattern used in base_bb/scripts/skills/skill.nut
        let mut workspace = Workspace::new();

        let content = r#"/*
 * Comment header
 */

skill <-
{
    m =
    {
        ID = ""
    },

    function getContainer() {
        return m.Container;
    }
}
"#;
        workspace
            .index_file(Path::new("/path/to/scripts/skills/skill.nut"), content)
            .expect("Should parse");

        let entry = workspace.get("skills/skill");
        assert!(
            entry.is_some(),
            "Should index multiline global table 'skill'"
        );

        let entry = entry.unwrap();
        assert_eq!(entry.name, "skill");
    }

    #[test]
    fn test_index_real_skill_nut() {
        // Test with the actual base_bb skill.nut file if it exists
        let file_path = Path::new("/home/antoine/bb-ws/base_bb/scripts/skills/skill.nut");
        if !file_path.exists() {
            eprintln!("Skipping test: {:?} not found", file_path);
            return;
        }

        let mut workspace = Workspace::new();
        let content = std::fs::read_to_string(file_path).expect("Should read file");

        workspace
            .index_file(file_path, &content)
            .expect("Should parse");

        let entry = workspace.get("skills/skill");
        assert!(
            entry.is_some(),
            "Should index real skill.nut as 'skills/skill'. Files: {:?}",
            workspace.files().keys().collect::<Vec<_>>()
        );
    }
}
