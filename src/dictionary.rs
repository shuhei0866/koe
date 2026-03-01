use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// A single dictionary file's content.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct DictionaryFile {
    /// Term corrections: misrecognized → correct.
    #[serde(default)]
    pub terms: HashMap<String, String>,
    /// Extra context hints for the AI processor.
    #[serde(default)]
    pub context_hints: Option<ContextHints>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ContextHints {
    pub domain: Option<String>,
    pub notes: Option<String>,
}

/// Merged dictionary from all loaded files.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    pub terms: HashMap<String, String>,
    pub domains: Vec<String>,
    pub notes: Vec<String>,
}

impl Dictionary {
    /// Load and merge all dictionary files from the given paths.
    pub fn load(paths: &[impl AsRef<Path>]) -> Result<Self> {
        let mut merged = Dictionary::default();

        for path in paths {
            let path = path.as_ref();
            if !path.exists() {
                tracing::warn!("Dictionary file not found, skipping: {}", path.display());
                continue;
            }
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("reading dictionary {}", path.display()))?;
            let dict_file: DictionaryFile = toml::from_str(&content)
                .with_context(|| format!("parsing dictionary {}", path.display()))?;

            merged.terms.extend(dict_file.terms);

            if let Some(hints) = dict_file.context_hints {
                if let Some(domain) = hints.domain {
                    merged.domains.push(domain);
                }
                if let Some(notes) = hints.notes {
                    merged.notes.push(notes);
                }
            }

            tracing::info!("Loaded dictionary: {}", path.display());
        }

        tracing::info!(
            "Dictionary loaded: {} terms, {} domains",
            merged.terms.len(),
            merged.domains.len()
        );
        Ok(merged)
    }

    /// Apply simple term replacements to text.
    pub fn apply_terms(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (from, to) in &self.terms {
            result = result.replace(from, to);
        }
        result
    }

    /// Format dictionary info for inclusion in AI prompts.
    pub fn format_for_prompt(&self) -> String {
        let mut parts = Vec::new();

        if !self.domains.is_empty() {
            parts.push(format!("Domain: {}", self.domains.join(", ")));
        }

        if !self.notes.is_empty() {
            parts.push(format!("Notes: {}", self.notes.join("; ")));
        }

        if !self.terms.is_empty() {
            let terms_str: Vec<String> = self
                .terms
                .iter()
                .map(|(k, v)| format!("  {} → {}", k, v))
                .collect();
            parts.push(format!("Term corrections:\n{}", terms_str.join("\n")));
        }

        parts.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_terms() {
        let mut dict = Dictionary::default();
        dict.terms.insert("ラスト".to_string(), "Rust".to_string());
        dict.terms
            .insert("クロード".to_string(), "Claude".to_string());

        assert_eq!(dict.apply_terms("ラストでクロードを使う"), "RustでClaudeを使う");
    }
}
