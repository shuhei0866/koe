use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Serde helper for terms.toml file format.
#[derive(Debug, Deserialize, Serialize, Default)]
struct TermsFile {
    #[serde(default)]
    terms: HashMap<String, String>,
}

/// Category-based context information.
#[derive(Debug, Clone, Default)]
pub struct MemoryContext {
    /// category -> list of entries
    pub sections: HashMap<String, Vec<String>>,
}

/// Memory manages learned terms and contextual information for AI post-processing.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    /// from -> to term dictionary
    pub terms: HashMap<String, String>,
    /// Category-based context
    pub context: MemoryContext,
    /// Directory where terms.toml and context.md are stored
    dir: PathBuf,
}

impl Memory {
    /// Load memory from the given directory.
    ///
    /// If the directory does not exist, returns a Memory with default (empty) values.
    /// If individual files are missing or empty, they are treated as empty.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut memory = Self {
            terms: HashMap::new(),
            context: MemoryContext::default(),
            dir: dir.to_path_buf(),
        };

        if !dir.exists() {
            return Ok(memory);
        }

        // Load terms.toml
        let terms_path = dir.join("terms.toml");
        if terms_path.exists() {
            let content = std::fs::read_to_string(&terms_path)
                .with_context(|| format!("reading {}", terms_path.display()))?;
            if !content.trim().is_empty() {
                let terms_file: TermsFile = toml::from_str(&content)
                    .with_context(|| format!("parsing {}", terms_path.display()))?;
                memory.terms = terms_file.terms;
            }
        }

        // Load context.md
        let context_path = dir.join("context.md");
        if context_path.exists() {
            let content = std::fs::read_to_string(&context_path)
                .with_context(|| format!("reading {}", context_path.display()))?;
            memory.context = Self::parse_context_md(&content);
        }

        Ok(memory)
    }

    /// Save memory to the directory (creates it if needed).
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating directory {}", self.dir.display()))?;

        // Save terms.toml
        let terms_file = TermsFile {
            terms: self.terms.clone(),
        };
        let terms_content =
            toml::to_string_pretty(&terms_file).context("serializing terms.toml")?;
        let terms_path = self.dir.join("terms.toml");
        std::fs::write(&terms_path, &terms_content)
            .with_context(|| format!("writing {}", terms_path.display()))?;

        // Save context.md
        let context_content = self.format_context_md();
        let context_path = self.dir.join("context.md");
        std::fs::write(&context_path, &context_content)
            .with_context(|| format!("writing {}", context_path.display()))?;

        Ok(())
    }

    /// Add a term mapping. Overwrites if `from` already exists.
    pub fn add_term(&mut self, from: &str, to: &str) {
        self.terms.insert(from.to_string(), to.to_string());
    }

    /// Add a context entry under the given category. Skips if the same content already exists.
    pub fn add_context(&mut self, category: &str, content: &str) {
        let entries = self
            .context
            .sections
            .entry(category.to_string())
            .or_default();
        let content_str = content.to_string();
        if !entries.contains(&content_str) {
            entries.push(content_str);
        }
    }

    /// Format terms for Whisper initial_prompt.
    /// Returns comma-separated list of correct term forms (the "to" side).
    /// Example: "Rust, Claude, koe, Ubuntu"
    pub fn format_for_whisper_hint(&self) -> String {
        if self.terms.is_empty() {
            return String::new();
        }
        let mut terms: Vec<&str> = self.terms.values().map(|s| s.as_str()).collect();
        terms.sort();
        terms.dedup();
        terms.join(", ")
    }

    /// Count total entries (terms + all context entries).
    pub fn total_entries(&self) -> usize {
        let context_count: usize = self.context.sections.values().map(|v| v.len()).sum();
        self.terms.len() + context_count
    }

    /// Check if memory has accumulated enough entries to warrant consolidation.
    pub fn needs_consolidation(&self, threshold: usize) -> bool {
        self.total_entries() >= threshold
    }

    /// Format memory for injection into an AI prompt.
    pub fn format_for_prompt(&self) -> String {
        let mut parts = Vec::new();

        if !self.terms.is_empty() {
            let mut term_lines: Vec<String> = self
                .terms
                .iter()
                .map(|(from, to)| format!("- {} → {}", from, to))
                .collect();
            term_lines.sort();
            parts.push(format!("## 用語辞書\n{}", term_lines.join("\n")));
        }

        if !self.context.sections.is_empty() {
            let mut categories: Vec<&String> = self.context.sections.keys().collect();
            categories.sort();
            for category in categories {
                let entries = &self.context.sections[category];
                let entry_lines: Vec<String> =
                    entries.iter().map(|e| format!("- {}", e)).collect();
                parts.push(format!("## {}\n{}", category, entry_lines.join("\n")));
            }
        }

        parts.join("\n\n")
    }

    /// Parse context.md content into MemoryContext.
    fn parse_context_md(content: &str) -> MemoryContext {
        let mut sections: HashMap<String, Vec<String>> = HashMap::new();
        let mut current_category: Option<String> = None;

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(header) = trimmed.strip_prefix("## ") {
                let category = header.trim().to_string();
                if !category.is_empty() {
                    current_category = Some(category);
                }
            } else if let Some(entry) = trimmed.strip_prefix("- ") {
                if let Some(ref cat) = current_category {
                    let entry = entry.trim().to_string();
                    if !entry.is_empty() {
                        sections.entry(cat.clone()).or_default().push(entry);
                    }
                }
            }
        }

        MemoryContext { sections }
    }

    /// Format context as markdown.
    fn format_context_md(&self) -> String {
        let mut parts = Vec::new();
        let mut categories: Vec<&String> = self.context.sections.keys().collect();
        categories.sort();

        for category in categories {
            let entries = &self.context.sections[category];
            let entry_lines: Vec<String> = entries.iter().map(|e| format!("- {}", e)).collect();
            parts.push(format!("## {}\n{}", category, entry_lines.join("\n")));
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("{}\n", parts.join("\n\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a unique temp directory for test isolation.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("koe-memory-tests")
            .join(name)
            .join(format!("{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn test_terms_roundtrip() {
        let dir = test_dir("terms_roundtrip");
        let mut mem = Memory::load(&dir).unwrap();

        mem.add_term("ラスト", "Rust");
        mem.add_term("クロード", "Claude");
        mem.save().unwrap();

        let loaded = Memory::load(&dir).unwrap();
        assert_eq!(loaded.terms.get("ラスト").unwrap(), "Rust");
        assert_eq!(loaded.terms.get("クロード").unwrap(), "Claude");
        assert_eq!(loaded.terms.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_context_roundtrip() {
        let dir = test_dir("context_roundtrip");
        let mut mem = Memory::load(&dir).unwrap();

        mem.add_context(
            "user_profile",
            "Rust エンジニア。koe という音声入力ツールを開発している。",
        );
        mem.add_context("domain", "ソフトウェア開発、Linux デスクトップ環境");
        mem.save().unwrap();

        let loaded = Memory::load(&dir).unwrap();
        assert_eq!(loaded.context.sections.len(), 2);
        assert_eq!(
            loaded.context.sections["user_profile"],
            vec!["Rust エンジニア。koe という音声入力ツールを開発している。"]
        );
        assert_eq!(
            loaded.context.sections["domain"],
            vec!["ソフトウェア開発、Linux デスクトップ環境"]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_add_term_dedup() {
        let dir = test_dir("term_dedup");
        let mut mem = Memory::load(&dir).unwrap();

        mem.add_term("ラスト", "Rust");
        mem.add_term("ラスト", "Rust Language");

        assert_eq!(mem.terms.len(), 1);
        assert_eq!(mem.terms.get("ラスト").unwrap(), "Rust Language");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_add_context_dedup() {
        let dir = test_dir("context_dedup");
        let mut mem = Memory::load(&dir).unwrap();

        mem.add_context("user_profile", "Rust エンジニア");
        mem.add_context("user_profile", "Rust エンジニア");
        mem.add_context("user_profile", "Linux ユーザー");

        assert_eq!(mem.context.sections["user_profile"].len(), 2);
        assert_eq!(
            mem.context.sections["user_profile"],
            vec!["Rust エンジニア", "Linux ユーザー"]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_for_prompt() {
        let dir = test_dir("format_prompt");
        let mut mem = Memory::load(&dir).unwrap();

        mem.add_term("ラスト", "Rust");
        mem.add_term("クロード", "Claude");
        mem.add_context("user_profile", "Rust エンジニア");
        mem.add_context("domain", "ソフトウェア開発");

        let output = mem.format_for_prompt();

        // Terms section should be present
        assert!(output.contains("## 用語辞書"));
        assert!(output.contains("- クロード → Claude"));
        assert!(output.contains("- ラスト → Rust"));

        // Context sections should be present
        assert!(output.contains("## domain"));
        assert!(output.contains("- ソフトウェア開発"));
        assert!(output.contains("## user_profile"));
        assert!(output.contains("- Rust エンジニア"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = std::env::temp_dir()
            .join("koe-memory-tests")
            .join("nonexistent-dir-that-does-not-exist-12345");
        let _ = std::fs::remove_dir_all(&dir);

        let mem = Memory::load(&dir).unwrap();
        assert!(mem.terms.is_empty());
        assert!(mem.context.sections.is_empty());
    }

    #[test]
    fn test_format_for_whisper_hint() {
        let dir = test_dir("whisper_hint");
        let mut mem = Memory::load(&dir).unwrap();
        mem.add_term("ラスト", "Rust");
        mem.add_term("クロード", "Claude");
        mem.add_term("コエ", "koe");
        let hint = mem.format_for_whisper_hint();
        assert!(hint.contains("Rust"));
        assert!(hint.contains("Claude"));
        assert!(hint.contains("koe"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_total_entries() {
        let dir = test_dir("total_entries");
        let mut mem = Memory::load(&dir).unwrap();
        assert_eq!(mem.total_entries(), 0);

        mem.add_term("a", "A");
        mem.add_term("b", "B");
        mem.add_context("domain", "ソフトウェア");
        assert_eq!(mem.total_entries(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_format_for_whisper_hint_empty() {
        let dir = test_dir("whisper_hint_empty");
        let mem = Memory::load(&dir).unwrap();
        assert_eq!(mem.format_for_whisper_hint(), "");
    }

    #[test]
    fn test_needs_consolidation() {
        let dir = test_dir("needs_consolidation");
        let mut mem = Memory::load(&dir).unwrap();
        assert!(!mem.needs_consolidation(3));

        mem.add_term("a", "A");
        mem.add_term("b", "B");
        mem.add_context("domain", "test");
        assert!(mem.needs_consolidation(3));
        assert!(!mem.needs_consolidation(4));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_empty_files() {
        let dir = test_dir("empty_files");
        std::fs::create_dir_all(&dir).unwrap();

        // Write empty files
        std::fs::write(dir.join("terms.toml"), "").unwrap();
        std::fs::write(dir.join("context.md"), "").unwrap();

        let mem = Memory::load(&dir).unwrap();
        assert!(mem.terms.is_empty());
        assert!(mem.context.sections.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
