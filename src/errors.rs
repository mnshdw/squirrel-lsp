use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalysisError {
    #[error("failed to configure squirrel parser: {0}")]
    Language(#[from] tree_sitter::LanguageError),
    #[error("failed to parse squirrel source")]
    ParseFailed,
    #[error("encountered invalid utf-8 in source text")]
    InvalidUtf8,
}

impl From<String> for AnalysisError {
    fn from(s: String) -> Self {
        if s.contains("parse") || s.contains("Parse") {
            Self::ParseFailed
        } else if s.contains("utf") || s.contains("UTF") {
            Self::InvalidUtf8
        } else {
            Self::ParseFailed
        }
    }
}
