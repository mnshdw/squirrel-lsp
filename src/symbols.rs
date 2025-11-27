//! Symbol map for Squirrel semantic analysis.
//!
//! Squirrel tables are fundamentally maps of slot_name → value.
//! This module models that structure for symbol resolution.
//!
//! # Resolution order
//!
//! When resolving an identifier inside a table method:
//! 1. Local scope (function parameters, local variables)
//! 2. Sibling slots in the same table
//! 3. Parent table slots (via inheritance chain)
//! 4. Global scope

use std::collections::HashMap;
use tower_lsp::lsp_types::Position;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub defined_at: Position,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SymbolKind {
    Variable,
    Function {
        params: Vec<String>,
    },
    Table {
        parent: Option<String>,
        slots: SymbolMap,
    },
}

pub type SymbolMap = HashMap<String, Symbol>;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Table {
    pub name: String,
    pub path: String,
    pub parent: Option<String>,
    pub slots: SymbolMap,
    pub defined_at: Position,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct FileSymbols {
    pub path: String,
    pub symbols: SymbolMap,
    pub main_table: Option<Table>,
}

/// Extract script path from a file path
/// e.g., "/path/to/scripts/skills/skill.nut" → "skills/skill"
pub fn extract_script_path(file_path: &str) -> String {
    if let Some(idx) = file_path.find("scripts/") {
        let after_scripts = &file_path[idx + 8..]; // len("scripts/") = 8
        return after_scripts.trim_end_matches(".nut").to_string();
    }

    std::path::Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_script_path() {
        assert_eq!(
            extract_script_path("/home/user/mods/scripts/skills/skill.nut"),
            "skills/skill"
        );
        assert_eq!(
            extract_script_path("/path/scripts/entity/tactical/enemies/legend_stollwurm.nut"),
            "entity/tactical/enemies/legend_stollwurm"
        );
        assert_eq!(
            extract_script_path("scripts/items/weapons/sword.nut"),
            "items/weapons/sword"
        );
    }
}
