use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// A single transcription history entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    /// UUID v4 identifier.
    pub id: String,
    /// When the transcription was recorded.
    pub timestamp: DateTime<Utc>,
    /// Raw text from the speech recognizer.
    pub raw_text: String,
    /// Post-processed text (may be identical to raw_text if no AI processing).
    pub processed_text: String,
}

/// Query parameters for searching history.
#[derive(Debug, Default)]
pub struct SearchQuery {
    /// Case-insensitive partial match applied to both raw_text and processed_text.
    pub text: Option<String>,
    /// Only entries at or after this timestamp.
    pub from: Option<DateTime<Utc>>,
    /// Only entries at or before this timestamp.
    pub to: Option<DateTime<Utc>>,
}

/// Manages persisted transcription history in a JSONL file.
#[derive(Debug)]
pub struct History {
    /// In-memory list of entries (chronological order, oldest first).
    pub entries: Vec<HistoryEntry>,
    /// Directory where `history.jsonl` is stored.
    dir: PathBuf,
    /// Maximum number of entries to retain. Oldest entries are removed first.
    pub max_entries: usize,
}

impl History {
    /// Load history from `{dir}/history.jsonl`.
    ///
    /// Blank lines and lines that fail to parse are silently skipped.
    /// If the file does not exist, returns an empty History.
    pub fn load(dir: &Path, max_entries: usize) -> Result<Self> {
        let mut entries = Vec::new();

        let file_path = dir.join("history.jsonl");
        if file_path.exists() {
            let content = std::fs::read_to_string(&file_path)
                .with_context(|| format!("reading {}", file_path.display()))?;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<HistoryEntry>(trimmed) {
                    Ok(entry) => entries.push(entry),
                    Err(_) => {
                        // Skip corrupt lines gracefully.
                        tracing::warn!("Skipping corrupt history line: {}", trimmed);
                    }
                }
            }
        }

        Ok(Self {
            entries,
            dir: dir.to_path_buf(),
            max_entries,
        })
    }

    /// Persist all current entries to `{dir}/history.jsonl`.
    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating directory {}", self.dir.display()))?;

        let file_path = self.dir.join("history.jsonl");
        let mut file = std::fs::File::create(&file_path)
            .with_context(|| format!("creating {}", file_path.display()))?;

        for entry in &self.entries {
            let line =
                serde_json::to_string(entry).context("serializing history entry")?;
            writeln!(file, "{}", line)
                .with_context(|| format!("writing {}", file_path.display()))?;
        }

        Ok(())
    }

    /// Add a new entry, persist immediately, and trim oldest entries if max_entries is exceeded.
    ///
    /// Auto-creates the directory if it does not exist.
    pub fn add_entry(
        &mut self,
        raw_text: impl Into<String>,
        processed_text: impl Into<String>,
    ) -> Result<&HistoryEntry> {
        let entry = HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            raw_text: raw_text.into(),
            processed_text: processed_text.into(),
        };
        self.entries.push(entry);

        // Trim oldest entries when limit is exceeded.
        if self.entries.len() > self.max_entries {
            let excess = self.entries.len() - self.max_entries;
            self.entries.drain(0..excess);
        }

        self.save()?;

        // Return reference to the last entry (the one just added, after potential trim).
        Ok(self.entries.last().unwrap())
    }

    /// Search entries by text and/or date range.
    ///
    /// Returns matching entries in newest-first order.
    pub fn search(&self, query: &SearchQuery) -> Vec<&HistoryEntry> {
        let needle = query.text.as_deref().map(|s| s.to_lowercase());

        let mut results: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| {
                // Date range filter.
                if let Some(from) = query.from {
                    if e.timestamp < from {
                        return false;
                    }
                }
                if let Some(to) = query.to {
                    if e.timestamp > to {
                        return false;
                    }
                }

                // Text filter (case-insensitive for ASCII).
                if let Some(ref needle) = needle {
                    let raw_lower = e.raw_text.to_lowercase();
                    let processed_lower = e.processed_text.to_lowercase();
                    if !raw_lower.contains(needle.as_str())
                        && !processed_lower.contains(needle.as_str())
                    {
                        return false;
                    }
                }

                true
            })
            .collect();

        // Return newest-first.
        results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        results
    }

    /// Delete the entry with the given id. Returns true if an entry was removed.
    pub fn delete_entry(&mut self, id: &str) -> Result<bool> {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        let removed = self.entries.len() < before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// Remove all entries and persist the empty state.
    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.save()
    }

    /// Export history as CSV to the given writer.
    ///
    /// Columns: id, timestamp (RFC3339), raw_text, processed_text.
    pub fn export_csv<W: Write>(&self, writer: W) -> Result<()> {
        let mut wtr = csv::Writer::from_writer(writer);
        wtr.write_record(["id", "timestamp", "raw_text", "processed_text"])
            .context("writing CSV header")?;
        for entry in &self.entries {
            wtr.write_record([
                entry.id.as_str(),
                &entry.timestamp.to_rfc3339(),
                entry.raw_text.as_str(),
                entry.processed_text.as_str(),
            ])
            .context("writing CSV record")?;
        }
        wtr.flush().context("flushing CSV writer")?;
        Ok(())
    }

    /// Export history as a JSON array string.
    pub fn export_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.entries).context("serializing history to JSON")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Create a unique, clean temp directory for test isolation.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("koe-history-tests")
            .join(name)
            .join(format!("{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    // -------------------------------------------------------------------------
    // Basic load / save
    // -------------------------------------------------------------------------

    #[test]
    fn test_load_nonexistent_dir() {
        let dir = test_dir("load_nonexistent");
        let h = History::load(&dir, 100).unwrap();
        assert!(h.entries.is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_add_entry_creates_dir_and_persists() {
        let dir = test_dir("add_creates_dir");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("hello world", "Hello World").unwrap();

        // File must exist now.
        assert!(dir.join("history.jsonl").exists());

        // Reload and verify.
        let h2 = History::load(&dir, 100).unwrap();
        assert_eq!(h2.entries.len(), 1);
        assert_eq!(h2.entries[0].raw_text, "hello world");
        assert_eq!(h2.entries[0].processed_text, "Hello World");

        cleanup(&dir);
    }

    #[test]
    fn test_roundtrip_multiple_entries() {
        let dir = test_dir("roundtrip_multi");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("first", "First").unwrap();
        h.add_entry("second", "Second").unwrap();
        h.add_entry("third", "Third").unwrap();

        let h2 = History::load(&dir, 100).unwrap();
        assert_eq!(h2.entries.len(), 3);
        assert_eq!(h2.entries[0].raw_text, "first");
        assert_eq!(h2.entries[2].raw_text, "third");

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // UUID ids
    // -------------------------------------------------------------------------

    #[test]
    fn test_entry_id_is_uuid() {
        let dir = test_dir("uuid_id");
        let mut h = History::load(&dir, 100).unwrap();
        let entry = h.add_entry("test", "Test").unwrap();
        // Must parse as a valid UUID.
        uuid::Uuid::parse_str(&entry.id).expect("id should be a valid UUID");
        cleanup(&dir);
    }

    #[test]
    fn test_ids_are_unique() {
        let dir = test_dir("unique_ids");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("a", "A").unwrap();
        h.add_entry("b", "B").unwrap();
        h.add_entry("c", "C").unwrap();
        let ids: std::collections::HashSet<_> =
            h.entries.iter().map(|e| e.id.clone()).collect();
        assert_eq!(ids.len(), 3);
        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // max_entries trimming
    // -------------------------------------------------------------------------

    #[test]
    fn test_max_entries_trim() {
        let dir = test_dir("max_entries");
        let mut h = History::load(&dir, 3).unwrap();

        h.add_entry("one", "one").unwrap();
        h.add_entry("two", "two").unwrap();
        h.add_entry("three", "three").unwrap();
        h.add_entry("four", "four").unwrap(); // should push "one" out

        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.entries[0].raw_text, "two");
        assert_eq!(h.entries[2].raw_text, "four");

        // Reload and verify persistence.
        let h2 = History::load(&dir, 3).unwrap();
        assert_eq!(h2.entries.len(), 3);
        assert_eq!(h2.entries[0].raw_text, "two");

        cleanup(&dir);
    }

    #[test]
    fn test_max_entries_one() {
        let dir = test_dir("max_entries_one");
        let mut h = History::load(&dir, 1).unwrap();

        h.add_entry("first", "first").unwrap();
        h.add_entry("second", "second").unwrap();

        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].raw_text, "second");

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // delete_entry
    // -------------------------------------------------------------------------

    #[test]
    fn test_delete_entry() {
        let dir = test_dir("delete_entry");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("keep", "keep").unwrap();
        let id_to_delete = {
            let e = h.add_entry("delete me", "delete me").unwrap();
            e.id.clone()
        };
        h.add_entry("also keep", "also keep").unwrap();

        assert_eq!(h.entries.len(), 3);

        let removed = h.delete_entry(&id_to_delete).unwrap();
        assert!(removed);
        assert_eq!(h.entries.len(), 2);
        assert!(h.entries.iter().all(|e| e.id != id_to_delete));

        // Verify persisted.
        let h2 = History::load(&dir, 100).unwrap();
        assert_eq!(h2.entries.len(), 2);
        assert!(h2.entries.iter().all(|e| e.id != id_to_delete));

        cleanup(&dir);
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let dir = test_dir("delete_nonexistent");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("something", "something").unwrap();

        let removed = h.delete_entry("nonexistent-id").unwrap();
        assert!(!removed);
        assert_eq!(h.entries.len(), 1);

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // clear
    // -------------------------------------------------------------------------

    #[test]
    fn test_clear() {
        let dir = test_dir("clear");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("a", "a").unwrap();
        h.add_entry("b", "b").unwrap();

        h.clear().unwrap();
        assert!(h.entries.is_empty());

        let h2 = History::load(&dir, 100).unwrap();
        assert!(h2.entries.is_empty());

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // search — text filter
    // -------------------------------------------------------------------------

    #[test]
    fn test_search_no_filter_returns_all_newest_first() {
        let dir = test_dir("search_no_filter");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("first", "first processed").unwrap();
        h.add_entry("second", "second processed").unwrap();

        let results = h.search(&SearchQuery::default());
        assert_eq!(results.len(), 2);
        // Newest first.
        assert_eq!(results[0].raw_text, "second");
        assert_eq!(results[1].raw_text, "first");

        cleanup(&dir);
    }

    #[test]
    fn test_search_text_partial_match_raw() {
        let dir = test_dir("search_text_raw");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("hello world", "Hi there").unwrap();
        h.add_entry("goodbye", "See you").unwrap();

        let results = h.search(&SearchQuery {
            text: Some("hello".to_string()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].raw_text, "hello world");

        cleanup(&dir);
    }

    #[test]
    fn test_search_text_partial_match_processed() {
        let dir = test_dir("search_text_processed");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("raw one", "processed alpha").unwrap();
        h.add_entry("raw two", "processed beta").unwrap();

        let results = h.search(&SearchQuery {
            text: Some("alpha".to_string()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].raw_text, "raw one");

        cleanup(&dir);
    }

    #[test]
    fn test_search_text_case_insensitive() {
        let dir = test_dir("search_case");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("Hello World", "Hello World").unwrap();

        let results = h.search(&SearchQuery {
            text: Some("HELLO".to_string()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);

        let results2 = h.search(&SearchQuery {
            text: Some("hello".to_string()),
            ..Default::default()
        });
        assert_eq!(results2.len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn test_search_text_no_match() {
        let dir = test_dir("search_no_match");
        let mut h = History::load(&dir, 100).unwrap();

        h.add_entry("foo bar", "foo bar").unwrap();

        let results = h.search(&SearchQuery {
            text: Some("xyz".to_string()),
            ..Default::default()
        });
        assert!(results.is_empty());

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // search — date range filter
    // -------------------------------------------------------------------------

    #[test]
    fn test_search_date_range_from() {
        let dir = test_dir("search_date_from");
        let mut h = History::load(&dir, 100).unwrap();

        // Inject entries with explicit timestamps.
        let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t1,
            raw_text: "jan 2024".into(),
            processed_text: "jan 2024".into(),
        });
        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t2,
            raw_text: "jun 2024".into(),
            processed_text: "jun 2024".into(),
        });
        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t3,
            raw_text: "jan 2025".into(),
            processed_text: "jan 2025".into(),
        });

        let cutoff = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
        let results = h.search(&SearchQuery {
            from: Some(cutoff),
            ..Default::default()
        });
        assert_eq!(results.len(), 2);
        // Newest first.
        assert_eq!(results[0].raw_text, "jan 2025");
        assert_eq!(results[1].raw_text, "jun 2024");

        cleanup(&dir);
    }

    #[test]
    fn test_search_date_range_to() {
        let dir = test_dir("search_date_to");
        let mut h = History::load(&dir, 100).unwrap();

        let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t1,
            raw_text: "old".into(),
            processed_text: "old".into(),
        });
        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t2,
            raw_text: "new".into(),
            processed_text: "new".into(),
        });

        let cutoff = Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap();
        let results = h.search(&SearchQuery {
            to: Some(cutoff),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].raw_text, "old");

        cleanup(&dir);
    }

    #[test]
    fn test_search_date_range_from_and_to() {
        let dir = test_dir("search_date_from_to");
        let mut h = History::load(&dir, 100).unwrap();

        for year in [2023u32, 2024, 2025] {
            h.entries.push(HistoryEntry {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Utc.with_ymd_and_hms(year as i32, 6, 1, 0, 0, 0).unwrap(),
                raw_text: format!("year {}", year),
                processed_text: format!("year {}", year),
            });
        }

        let results = h.search(&SearchQuery {
            from: Some(Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()),
            to: Some(Utc.with_ymd_and_hms(2024, 12, 31, 0, 0, 0).unwrap()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].raw_text, "year 2024");

        cleanup(&dir);
    }

    #[test]
    fn test_search_combined_text_and_date() {
        let dir = test_dir("search_combined");
        let mut h = History::load(&dir, 100).unwrap();

        let t1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();

        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t1,
            raw_text: "rust code old".into(),
            processed_text: "rust code old".into(),
        });
        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t2,
            raw_text: "rust code new".into(),
            processed_text: "rust code new".into(),
        });
        h.entries.push(HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: t2,
            raw_text: "python code".into(),
            processed_text: "python code".into(),
        });

        let results = h.search(&SearchQuery {
            text: Some("rust".to_string()),
            from: Some(Utc.with_ymd_and_hms(2024, 6, 1, 0, 0, 0).unwrap()),
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].raw_text, "rust code new");

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // Corrupt / blank line handling
    // -------------------------------------------------------------------------

    #[test]
    fn test_load_skips_blank_lines() {
        let dir = test_dir("skip_blank");
        std::fs::create_dir_all(&dir).unwrap();

        let valid = HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            raw_text: "valid".into(),
            processed_text: "valid".into(),
        };
        let line = serde_json::to_string(&valid).unwrap();
        let content = format!("\n{}\n\n\n", line);
        std::fs::write(dir.join("history.jsonl"), content).unwrap();

        let h = History::load(&dir, 100).unwrap();
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].raw_text, "valid");

        cleanup(&dir);
    }

    #[test]
    fn test_load_skips_corrupt_lines() {
        let dir = test_dir("skip_corrupt");
        std::fs::create_dir_all(&dir).unwrap();

        let valid = HistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            raw_text: "good line".into(),
            processed_text: "good line".into(),
        };
        let good = serde_json::to_string(&valid).unwrap();
        let content = format!("{}\ncorrupt{{not json\n{}\n", good, good);
        std::fs::write(dir.join("history.jsonl"), content).unwrap();

        let h = History::load(&dir, 100).unwrap();
        assert_eq!(h.entries.len(), 2);

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // export_csv
    // -------------------------------------------------------------------------

    #[test]
    fn test_export_csv_header_and_rows() {
        let dir = test_dir("export_csv");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("raw text", "processed text").unwrap();

        let mut buf = Vec::new();
        h.export_csv(&mut buf).unwrap();
        let csv_str = String::from_utf8(buf).unwrap();

        assert!(csv_str.starts_with("id,timestamp,raw_text,processed_text"));
        assert!(csv_str.contains("raw text"));
        assert!(csv_str.contains("processed text"));

        cleanup(&dir);
    }

    #[test]
    fn test_export_csv_empty() {
        let dir = test_dir("export_csv_empty");
        let h = History::load(&dir, 100).unwrap();

        let mut buf = Vec::new();
        h.export_csv(&mut buf).unwrap();
        let csv_str = String::from_utf8(buf).unwrap();

        // Only the header row.
        let lines: Vec<&str> = csv_str.lines().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], "id,timestamp,raw_text,processed_text");

        cleanup(&dir);
    }

    #[test]
    fn test_export_csv_comma_in_field() {
        let dir = test_dir("export_csv_comma");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("hello, world", "hi, there").unwrap();

        let mut buf = Vec::new();
        h.export_csv(&mut buf).unwrap();
        let csv_str = String::from_utf8(buf).unwrap();

        // Fields with commas must be quoted.
        assert!(csv_str.contains("\"hello, world\""));

        cleanup(&dir);
    }

    // -------------------------------------------------------------------------
    // export_json
    // -------------------------------------------------------------------------

    #[test]
    fn test_export_json_empty() {
        let dir = test_dir("export_json_empty");
        let h = History::load(&dir, 100).unwrap();

        let json = h.export_json().unwrap();
        let parsed: Vec<HistoryEntry> = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_export_json_roundtrip() {
        let dir = test_dir("export_json_roundtrip");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("raw", "processed").unwrap();

        let json = h.export_json().unwrap();
        let parsed: Vec<HistoryEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].raw_text, "raw");
        assert_eq!(parsed[0].processed_text, "processed");

        cleanup(&dir);
    }

    #[test]
    fn test_export_json_multiple_entries() {
        let dir = test_dir("export_json_multi");
        let mut h = History::load(&dir, 100).unwrap();
        h.add_entry("a", "A").unwrap();
        h.add_entry("b", "B").unwrap();
        h.add_entry("c", "C").unwrap();

        let json = h.export_json().unwrap();
        let parsed: Vec<HistoryEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 3);
        // Order is chronological (oldest first), matching internal storage.
        assert_eq!(parsed[0].raw_text, "a");
        assert_eq!(parsed[2].raw_text, "c");

        cleanup(&dir);
    }
}
