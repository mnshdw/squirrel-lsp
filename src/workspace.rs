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

/// An identifier defined nowhere in the workspace is assumed to be declared by the host env
/// once it is referenced at least this many times. A typo is written once (usually), but a binding
/// like like `require` or `Const` gets used many times.
const HOST_GLOBAL_MIN_REFERENCES: usize = 2;

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
    Method,
    Field,
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
    pub line: u32,
    pub column: u32,
}

/// The workspace indexed by script path.
///
/// Script paths are relative to the workspace folder and without the `.nut` extension.
/// Eg. `Legends-public/scripts/statistics/statistics_manager`.
#[derive(Debug, Default)]
pub struct Workspace {
    /// Script path -> file entry
    files: HashMap<String, FileEntry>,
    /// Trailing path suffix -> file keys having that suffix
    suffix_index: HashMap<String, Vec<String>>,
    /// Class/table name -> file keys defining it
    name_index: HashMap<String, Vec<String>>,
    /// Global identifiers defined across all files
    globals: HashSet<String>,
    /// File key -> identifiers that file references but nothing defines
    unresolved: HashMap<String, HashMap<String, usize>>,
    /// Identifier -> how many times the whole workspace references it unresolved
    unresolved_totals: HashMap<String, usize>,
    /// `globals`, plus the bindings inferred from `unresolved_totals`
    known_globals: HashSet<String>,
    /// Workspace folders, used to make file paths relative before keying them
    folders: Vec<PathBuf>,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a workspace whose file keys are relative to `folders`.
    pub fn with_folders(folders: Vec<PathBuf>) -> Self {
        Self {
            folders,
            ..Self::default()
        }
    }

    /// Resolve a path as written in source to every file it could refer to.
    pub fn resolve_all(&self, script_path: &str) -> Vec<&FileEntry> {
        let key = suffix_key(script_path);

        self.suffix_index
            .get(&key)
            .map(|keys| keys.iter().filter_map(|k| self.files.get(k)).collect())
            .unwrap_or_default()
    }

    /// Get a file entry by the path used to refer to it in source.
    ///
    /// When several files match (the same script provided by several mods), the shallowest one wins.
    pub fn get(&self, script_path: &str) -> Option<&FileEntry> {
        self.resolve_all(script_path)
            .into_iter()
            .min_by_key(|e| (path_components(&e.script_path).len(), e.script_path.clone()))
    }

    pub fn script_path_for(&self, file_path: &Path) -> String {
        extract_script_path(file_path, &self.folders)
    }

    /// Check if a script path exists in the workspace
    pub fn contains(&self, script_path: &str) -> bool {
        !self.resolve_all(script_path).is_empty()
    }

    /// Get all files in the workspace
    pub fn files(&self) -> &HashMap<String, FileEntry> {
        &self.files
    }

    /// Register a global identifier
    pub fn register_global(&mut self, name: String) {
        self.globals.insert(name);
    }

    /// Identifiers actually defined at the root table somewhere in the workspace.
    pub fn globals(&self) -> &HashSet<String> {
        &self.globals
    }

    /// Every identifier a file may refer to without declaring it: the ones defined in the workspace,
    /// plus the env bindings inferred from usage.
    ///
    /// This is what the resolver checks against, so it is what decides whether an identifier is
    /// reported as undeclared or not.
    pub fn known_globals(&self) -> &HashSet<String> {
        &self.known_globals
    }

    /// Record the identifiers `file_path` references but that nothing defines.
    ///
    /// Feeding these back is what lets the LSP try to "guess" the env API: Squirrel is always
    /// embedded in an application that binds its own names into the root table, and those names
    /// appear in no `.nut` file. They are hard to distinguish from a typo when looked at one
    /// reference at a time, but it is easier accross the whole worksapce: a typo is (usually)
    /// written once but an API is used multiple times.
    pub fn set_unresolved(&mut self, file_path: &Path, names: &[String]) {
        let key = self.script_path_for(file_path);
        if key.is_empty() {
            return;
        }

        // Withdraw whatever this file contributed previously, so re-indexing a file on does not
        // change the totals.
        if let Some(previous) = self.unresolved.remove(&key) {
            for (name, count) in previous {
                if let Some(total) = self.unresolved_totals.get_mut(&name) {
                    *total = total.saturating_sub(count);
                    if *total == 0 {
                        self.unresolved_totals.remove(&name);
                    }
                }
            }
        }

        let mut counts: HashMap<String, usize> = HashMap::new();
        for name in names {
            *counts.entry(name.clone()).or_default() += 1;
        }
        for (name, count) in &counts {
            *self.unresolved_totals.entry(name.clone()).or_default() += count;
        }

        self.unresolved.insert(key, counts);
    }

    /// Recompute the set of identifiers the resolver considers as known.
    pub fn infer_host_globals(&mut self) {
        self.known_globals = self.globals.clone();

        self.known_globals.extend(
            self.unresolved_totals
                .iter()
                .filter(|(_, total)| **total >= HOST_GLOBAL_MIN_REFERENCES)
                .map(|(name, _)| name.clone()),
        );
    }

    /// Get all members of a script, including inherited ones.
    ///
    /// When several mods provide the same script, their members are merged: at runtime they all
    /// write into one class, so a method added by any of them exists.
    pub fn get_all_members(&self, script_path: &str) -> Vec<MemberInfo> {
        let mut member_map: HashMap<String, MemberInfo> = HashMap::new();

        for entry in self.resolve_all(script_path) {
            // Walk from the furthest ancestor down, so a child overrides its parent.
            for ancestor in self.get_ancestors(&entry.script_path).into_iter().rev() {
                for member in &ancestor.members {
                    member_map.insert(member.name.clone(), member.clone());
                }
            }
            for member in &entry.members {
                member_map.insert(member.name.clone(), member.clone());
            }
        }

        member_map.into_values().collect()
    }

    pub fn has_member(&self, script_path: &str, member_name: &str) -> bool {
        let members = self.get_all_members(script_path);
        members.iter().any(|m| m.name == member_name)
    }

    /// Every direct child of a script, across all the copies of it in the workspace.
    pub fn children_of(&self, script_path: &str) -> HashSet<String> {
        self.resolve_all(script_path)
            .iter()
            .flat_map(|entry| entry.children.iter().cloned())
            .collect()
    }

    /// Find where a method is defined, searching current class and ancestors.
    /// Returns (file_path, line, column) if found.
    pub fn find_method_definition(
        &self,
        script_path: &str,
        method_name: &str,
    ) -> Option<(&PathBuf, u32, u32)> {
        let is_method =
            |m: &&MemberInfo| m.name == method_name && m.member_type == MemberType::Method;

        for entry in self.resolve_all(script_path) {
            if let Some(member) = entry.members.iter().find(is_method) {
                return Some((&entry.file_path, member.line, member.column));
            }

            for ancestor in self.get_ancestors(&entry.script_path) {
                if let Some(member) = ancestor.members.iter().find(is_method) {
                    return Some((&ancestor.file_path, member.line, member.column));
                }
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

        results.sort_by(|a, b| a.3.cmp(b.3).then_with(|| a.1.cmp(&b.1)));
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
                    current_path = parent_entry.script_path.clone();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        ancestors
    }

    /// Index a single file into the workspace
    pub fn index_file(&mut self, file_path: &Path, content: &str) -> Result<(), AnalysisError> {
        let script_path = self.script_path_for(file_path);
        if script_path.is_empty() {
            return Ok(());
        }

        let tree = helpers::parse_squirrel(content)?;
        let root = tree.root_node();

        // Try to find inherit() calls first (class definitions)
        let inherits = find_inherit_calls(root, content);

        if let Some(inherit_call) = inherits.into_iter().next() {
            // This is a class file. The parent is kept exactly as written in source and
            // resolved later, once every file has been indexed.
            let (line, column) = node_position(inherit_call.class_name_node);

            let entry = FileEntry {
                file_path: file_path.to_path_buf(),
                script_path: script_path.clone(),
                name: inherit_call.class_name,
                parent_path: Some(inherit_call.parent_path),
                parent: None, // Resolved later
                children: Vec::new(),
                members: extract_members_from_table(inherit_call.class_body, content),
                line,
                column,
            };

            self.insert_file(entry);
        } else {
            // Look for global table definition matching file name
            let file_stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

            if let Some((name, name_node, table_node)) = find_global_table(root, content, file_stem)
            {
                let (line, column) = node_position(name_node);

                let entry = FileEntry {
                    file_path: file_path.to_path_buf(),
                    script_path: script_path.clone(),
                    name,
                    parent_path: None,
                    parent: None,
                    children: Vec::new(),
                    members: extract_members_from_table(table_node, content),
                    line,
                    column,
                };

                self.insert_file(entry);
            }
        }

        // Extract global definitions
        self.extract_globals(root, content);

        Ok(())
    }

    /// Insert a file entry and make it findable by every trailing part of its path.
    fn insert_file(&mut self, entry: FileEntry) {
        let key = entry.script_path.clone();

        for suffix in path_suffixes(&key) {
            let keys = self.suffix_index.entry(suffix).or_default();
            if !keys.contains(&key) {
                keys.push(key.clone());
            }
        }

        let by_name = self.name_index.entry(entry.name.clone()).or_default();
        if !by_name.contains(&key) {
            by_name.push(key.clone());
        }

        self.files.insert(key, entry);
    }

    /// Find the file(s) defining a class or table by its name, e.g. `rotu_mod_aura_abstract`.
    ///
    /// This is how a bare identifier in source is resolved: BB writes
    /// `rotu_mod_aura_abstract.create()`, referring to the class by name rather than by path.
    pub fn find_by_name(&self, name: &str) -> Vec<&FileEntry> {
        let mut entries: Vec<&FileEntry> = self
            .name_index
            .get(name)
            .map(|keys| keys.iter().filter_map(|k| self.files.get(k)).collect())
            .unwrap_or_default();

        entries.sort_by(|a, b| a.script_path.cmp(&b.script_path));
        entries
    }

    /// Build inheritance relationships after all files are indexed
    pub fn build_inheritance_graph(&mut self) {
        let script_paths: Vec<String> = self.files.keys().cloned().collect();

        for script_path in script_paths {
            let Some(parent_path) = self
                .files
                .get(&script_path)
                .and_then(|e| e.parent_path.clone())
            else {
                continue;
            };

            // A parent path may resolve to several files when mods overlay a script.
            // Record the child against all of them, so "does this class have descendants" stays
            // true independently of which copy is being asked about.
            let parent_keys: Vec<String> = self
                .resolve_all(&parent_path)
                .iter()
                .map(|e| e.script_path.clone())
                .collect();

            let Some(primary) = parent_keys.iter().min() else {
                continue;
            };

            if let Some(entry) = self.files.get_mut(&script_path) {
                entry.parent = Some(primary.clone());
            }

            for parent_key in &parent_keys {
                if *parent_key == script_path {
                    continue; // A file cannot be its own parent.
                }
                if let Some(parent) = self.files.get_mut(parent_key)
                    && !parent.children.contains(&script_path)
                {
                    parent.children.push(script_path.clone());
                }
            }
        }
    }

    /// Extract global variable definitions from a file
    fn extract_globals(&mut self, root: Node, text: &str) {
        for child in root.children(&mut root.walk()) {
            let name = match child.kind() {
                "update_expression" => new_slot_name(child, text),
                "class_declaration"
                | "function_declaration"
                | "enum_declaration"
                | "const_declaration" => first_identifier_name(child, text),
                _ => None,
            };

            if let Some(name) = name {
                self.register_global(name);
            }
        }
    }

    /// Find similar script paths for "did you mean?" suggestions.
    pub fn find_similar_paths(&self, target: &str) -> Vec<String> {
        let target = suffix_key(target);
        let max_distance = target.len() / 2;
        if max_distance == 0 {
            return Vec::new();
        }

        let depth = count_components(&target);

        let mut candidates: Vec<(usize, &str)> = self
            .suffix_index
            .keys()
            .filter(|suffix| {
                count_components(suffix) == depth
                    && suffix.len().abs_diff(target.len()) < max_distance
            })
            .map(|suffix| (levenshtein_distance(&target, suffix), suffix.as_str()))
            .filter(|(distance, _)| *distance < max_distance)
            .collect();

        candidates.sort_unstable();

        candidates
            .into_iter()
            .take(3)
            .map(|(_, path)| path.to_string())
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
///
/// Only called on children of the script root, where `this` *is* the root table:
///   `foo <- ...`        -> foo
///   `::foo <- ...`      -> foo
///   `this.foo <- ...`   -> foo
///   `some.thing <- ...` -> None  (writes into `some`, not the root table)
///   `this.foo.bar <- .` -> None  (writes into `this.foo`)
fn new_slot_name(node: Node, text: &str) -> Option<String> {
    let children: Vec<Node> = node.children(&mut node.walk()).collect();

    // A new slot, not a plain assignment or compound update.
    if !children.iter().any(|c| c.kind() == "<-") {
        return None;
    }

    let target = *children.first()?;

    match target.kind() {
        "identifier" => Some(get_node_text(target, text).to_string()),
        "global_variable" => direct_identifiers(target)
            .first()
            .map(|c| get_node_text(*c, text).to_string()),
        "deref_expression" => {
            let identifiers = direct_identifiers(target);
            // Exactly `<base>.<member>`, and the base must be the root table.
            match identifiers.as_slice() {
                [base, member] if get_node_text(*base, text) == "this" => {
                    Some(get_node_text(*member, text).to_string())
                },
                _ => None,
            }
        },
        _ => None,
    }
}

/// Direct `identifier` children of a node.
fn direct_identifiers<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    node.children(&mut node.walk())
        .filter(|c| c.kind() == "identifier")
        .collect()
}

/// Name of a declaration: its first identifier child (`class Foo extends Bar` -> `Foo`).
fn first_identifier_name(node: Node, text: &str) -> Option<String> {
    node.children(&mut node.walk())
        .find(|c| c.kind() == "identifier")
        .map(|c| get_node_text(c, text).to_string())
}

fn count_components(path: &str) -> usize {
    path.split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != ".")
        .count()
}

/// Split a path into non-empty components, normalizing separators.
fn path_components(path: &str) -> Vec<&str> {
    path.split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != ".")
        .collect()
}

fn strip_nut_extension(path: &str) -> &str {
    path.strip_suffix(".nut").unwrap_or(path)
}

/// Compute the key for a file on disk: its path relative to the workspace folder containing it,
/// without the `.nut` extension.
fn extract_script_path(file_path: &Path, folders: &[PathBuf]) -> String {
    let path_str = file_path.to_string_lossy().replace('\\', "/");

    // Relative to the innermost (longest-matching) workspace folder.
    let mut relative = path_str.as_str();
    let mut matched_len = 0;
    for folder in folders {
        let folder = folder.to_string_lossy().replace('\\', "/");
        let folder = folder.trim_end_matches('/');
        if folder.is_empty() || folder.len() < matched_len {
            continue;
        }

        if let Some(rest) = path_str.strip_prefix(folder)
            && (rest.is_empty() || rest.starts_with('/'))
        {
            matched_len = folder.len();
            relative = rest;
        }
    }

    suffix_key(relative)
}

/// Every trailing run of components of a path, longest first.
fn path_suffixes(key: &str) -> Vec<String> {
    let components = path_components(key);

    (0..components.len())
        .map(|i| components[i..].join("/"))
        .collect()
}

fn suffix_key(path: &str) -> String {
    strip_nut_extension(&path_components(path).join("/")).to_string()
}

/// Find a global table definition that matches the file name.
/// Also searches inside ERROR nodes for partial parse results.
fn find_global_table<'tree>(
    root: Node<'tree>,
    text: &str,
    file_stem: &str,
) -> Option<(String, Option<Node<'tree>>, Node<'tree>)> {
    fn search_node<'tree>(
        node: Node<'tree>,
        text: &str,
        file_stem: &str,
    ) -> Option<(String, Option<Node<'tree>>, Node<'tree>)> {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "update_expression" {
                let mut has_new_slot = false;
                let mut identifier_name = None;
                let mut identifier_node = None;
                let mut table_node = None;

                for n in child.children(&mut child.walk()) {
                    match n.kind() {
                        "<-" => has_new_slot = true,
                        "identifier" | "deref_expression" if identifier_name.is_none() => {
                            identifier_name = helpers::extract_identifier_name(n, text);
                            identifier_node = helpers::find_last_identifier(n);
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
                    return Some((name, identifier_node, table));
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
        let mut identifier_node = None;
        let mut table_node = None;

        for child in root.children(&mut root.walk()) {
            match child.kind() {
                "<-" => has_new_slot = true,
                "identifier" if identifier_name.is_none() => {
                    identifier_name = Some(get_node_text(child, text).to_string());
                    identifier_node = Some(child);
                },
                // When parsing fails, the table might just be "{"
                "table" | "{" if table_node.is_none() => {
                    table_node = Some(child);
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
            return Some((name, identifier_node, root));
        }
    }

    search_node(root, text, file_stem)
}

/// Line/column of a node, defaulting to the start of the file.
fn node_position(node: Option<Node>) -> (u32, u32) {
    node.map_or((0, 0), |n| {
        (
            n.start_position().row as u32,
            n.start_position().column as u32,
        )
    })
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
                let mut key_node = None;
                let mut value_node = None;
                let mut has_function_decl = false;

                for slot_child in child.children(&mut child.walk()) {
                    match slot_child.kind() {
                        "identifier" if key_node.is_none() => {
                            key_node = Some(slot_child);
                        },
                        "function_declaration" => {
                            has_function_decl = true;
                            // Extract function name
                            if let Some(name_node) = slot_child.child_by_field_name("name") {
                                let start = name_node.start_position();
                                members.push(MemberInfo {
                                    name: get_node_text(name_node, text).to_string(),
                                    member_type: MemberType::Method,
                                    line: start.row as u32,
                                    column: start.column as u32,
                                });
                            } else {
                                // Fallback: look for identifier in function_declaration
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
                        },
                        "lambda_expression" | "anonymous_function" => {
                            value_node = Some(slot_child);
                        },
                        "table" | "array" | "string" | "integer" | "float" | "bool" => {
                            value_node = Some(slot_child);
                        },
                        _ => {},
                    }
                }

                if !has_function_decl && let Some(key) = key_node {
                    let is_function = value_node.is_some_and(|v| {
                        matches!(v.kind(), "lambda_expression" | "anonymous_function")
                    });

                    let start = key.start_position();
                    members.push(MemberInfo {
                        name: get_node_text(key, text).to_string(),
                        member_type: if is_function {
                            MemberType::Method
                        } else {
                            MemberType::Field
                        },
                        line: start.row as u32,
                        column: start.column as u32,
                    });
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
    let s2_chars: Vec<char> = s2.chars().collect();

    // Two rows are enough: each cell only looks at the row above and the cell to its
    // left. The full matrix was allocating a Vec per row, which is what made this
    // expensive when scoring thousands of candidates.
    let mut previous: Vec<usize> = (0..=s2_chars.len()).collect();
    let mut current = vec![0; s2_chars.len() + 1];

    for (i, c1) in s1.chars().enumerate() {
        current[0] = i + 1;

        for (j, c2) in s2_chars.iter().enumerate() {
            let cost = usize::from(c1 != *c2);
            current[j + 1] = (previous[j + 1] + 1)
                .min(current[j] + 1)
                .min(previous[j] + cost);
        }

        std::mem::swap(&mut previous, &mut current);
    }

    previous[s2_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        Workspace::with_folders(vec![PathBuf::from("/ws")])
    }

    #[test]
    fn test_script_path_is_relative_to_the_workspace_folder() {
        let workspace = ws();
        assert_eq!(
            workspace.script_path_for(Path::new("/ws/legends/scripts/entity/actor.nut")),
            "legends/scripts/entity/actor"
        );
        // No directory convention is assumed: a project with no scripts/ dir is keyed
        // the same way, rather than being skipped.
        assert_eq!(
            workspace.script_path_for(Path::new("/ws/ui/font.nut")),
            "ui/font"
        );
    }

    #[test]
    fn test_path_suffixes() {
        assert_eq!(
            path_suffixes("legends/scripts/entity/actor"),
            vec![
                "legends/scripts/entity/actor",
                "scripts/entity/actor",
                "entity/actor",
                "actor",
            ]
        );
    }

    #[test]
    fn test_resolve_by_partial_path() {
        let mut workspace = ws();
        workspace
            .index_file(
                Path::new("/ws/legends/scripts/entity/tactical/actor.nut"),
                r#"this.actor <- this.inherit("scripts/entity/base", {});"#,
            )
            .unwrap();

        // Every way source refers to this file resolves to it.
        for path in [
            "scripts/entity/tactical/actor",
            "entity/tactical/actor",
            "entity/tactical/actor.nut",
            "legends/scripts/entity/tactical/actor",
        ] {
            assert!(workspace.contains(path), "'{path}' should resolve");
        }

        // A path that is not a trailing run of components does not.
        assert!(!workspace.contains("tactical/aktor"));
        assert!(!workspace.contains("entity/actor"));
    }

    #[test]
    fn test_mods_overlaying_a_script_all_resolve() {
        let mut workspace = ws();
        for mod_name in ["vanilla", "legends", "rotu"] {
            workspace
                .index_file(
                    Path::new(&format!("/ws/{mod_name}/scripts/entity/actor.nut")),
                    r#"this.actor <- this.inherit("scripts/entity/base", {});"#,
                )
                .unwrap();
        }

        assert_eq!(workspace.resolve_all("entity/actor").len(), 3);
        // get() is stable rather than insertion-order-dependent.
        assert_eq!(
            workspace.get("entity/actor").unwrap().script_path,
            "legends/scripts/entity/actor"
        );
    }

    #[test]
    fn test_extract_globals_covers_all_top_level_declarations() {
        let mut workspace = ws();
        let code = r#"
            Font <- {};
            class Sprite {}
            function require(_path) {}
            enum Align { Left, Right }
            const VERSION = "1.0";
            local hidden = 1;
        "#;
        workspace
            .index_file(Path::new("/ws/ui/font.nut"), code)
            .unwrap();

        let globals = workspace.globals();
        for expected in ["Font", "Sprite", "require", "Align", "VERSION"] {
            assert!(globals.contains(expected), "missing global '{expected}'");
        }
        assert!(
            !globals.contains("hidden"),
            "top-level 'local' is file-scoped, not a global"
        );
    }

    #[test]
    fn test_extract_globals_new_slot_shapes() {
        let mut workspace = ws();
        let code = r#"
            plain <- 1;
            ::explicit <- 2;
            this.actor <- this.inherit("scripts/entity/base", {});
            some.thing <- 3;
            this.nested.deep <- 4;
        "#;
        workspace
            .index_file(Path::new("/ws/scripts/entity/actor.nut"), code)
            .unwrap();

        let globals = workspace.globals();
        for expected in ["plain", "explicit", "actor"] {
            assert!(globals.contains(expected), "missing global '{expected}'");
        }
        // These write into another table, not the root table.
        for unexpected in ["thing", "deep", "nested", "some"] {
            assert!(
                !globals.contains(unexpected),
                "'{unexpected}' is not a root-table slot"
            );
        }
    }

    #[test]
    fn test_host_globals_are_inferred_from_repeated_use() {
        let mut workspace = ws();

        workspace.set_unresolved(Path::new("/ws/a.nut"), &["Font".to_string()]);
        workspace.set_unresolved(
            Path::new("/ws/b.nut"),
            &["Font".to_string(), "typo".to_string()],
        );
        workspace.infer_host_globals();

        // Used twice, defined nowhere: the host provides it.
        assert!(workspace.known_globals().contains("Font"));
        // Used once, defined nowhere: still a typo.
        assert!(!workspace.known_globals().contains("typo"));
    }

    #[test]
    fn test_reindexing_a_file_does_not_inflate_reference_counts() {
        let mut workspace = ws();

        // The same file, re-indexed on every keystroke, must not look like two files.
        workspace.set_unresolved(Path::new("/ws/a.nut"), &["Font".to_string()]);
        workspace.set_unresolved(Path::new("/ws/a.nut"), &["Font".to_string()]);
        workspace.infer_host_globals();

        assert!(
            !workspace.known_globals().contains("Font"),
            "one reference in one file, however many times that file was re-indexed"
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
        assert!(method_names.contains(&"m"));

        // Verify types are correct
        let m_field = entry.members.iter().find(|m| m.name == "m").unwrap();
        assert_eq!(m_field.member_type, MemberType::Field, "m should be Field");

        let get_flags = entry.members.iter().find(|m| m.name == "getFlags").unwrap();
        assert_eq!(
            get_flags.member_type,
            MemberType::Method,
            "getFlags should be Method"
        );
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
        let actor = workspace.get("entity/tactical/actor").unwrap();

        assert_eq!(human.parent.as_ref(), Some(&actor.script_path));
        assert!(actor.children.contains(&human.script_path));
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
        assert!(workspace.has_member("entity/tactical/human", "onTurnStart"));

        // human should inherit onDeath from actor
        assert!(workspace.has_member("entity/tactical/human", "onDeath"));

        // actor should have onDeath
        assert!(workspace.has_member("entity/tactical/actor", "onDeath"));

        // actor should NOT have onTurnStart
        assert!(!workspace.has_member("entity/tactical/actor", "onTurnStart"));
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
        let code = r#"
            this.skill <- {
            };
        "#;
        let mut workspace = Workspace::new();

        workspace
            .index_file(Path::new("scripts/skills/skill"), code)
            .unwrap();

        let entry = workspace.get("skills/skill");
        assert!(
            entry.is_some(),
            "Should index real skill.nut as 'skills/skill'. Files: {:?}",
            workspace.files().keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_real_skill_file_members() {
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

        let entry = workspace.get("skills/skill").expect("Should find entry");
        eprintln!("Entry name: {}", entry.name);
        eprintln!("Members count: {}", entry.members.len());
        for m in &entry.members {
            eprintln!("  - {} ({:?})", m.name, m.member_type);
        }

        // Should have m field
        assert!(
            entry.members.iter().any(|m| m.name == "m"),
            "Should have m field"
        );
        // Should have getContainer method
        assert!(
            entry.members.iter().any(|m| m.name == "getContainer"),
            "Should have getContainer"
        );
    }
}
