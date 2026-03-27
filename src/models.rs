use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub file_path: String,
    pub language: String,
    pub chunk_type: String,
    pub chunk_name: Option<String>,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub chunk_name: Option<String>,
    pub chunk_type: String,
    pub language: String,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexResult {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexStatus {
    pub last_commit: Option<String>,
    pub last_indexed: Option<String>,
    pub total_points: u64,
    pub collection_name: String,
    pub repo_path: String,
}

/// Generate a deterministic point ID from file path and start line.
/// Reserves ID 0 for the metadata point.
pub fn make_point_id(file_path: &str, start_line: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{}::{}", file_path, start_line).hash(&mut hasher);
    let id = hasher.finish();
    if id == 0 { 1 } else { id }
}

impl SearchResult {
    /// Format as human-readable text for MCP tool responses.
    pub fn format_results(results: &[SearchResult], query: &str) -> String {
        if results.is_empty() {
            return format!("No results found for \"{}\".", query);
        }

        let mut out = format!("Found {} results for \"{}\":\n", results.len(), query);

        for r in results {
            let name_part = match &r.chunk_name {
                Some(name) => format!(" ({}: {})", r.chunk_type, name),
                None => format!(" ({})", r.chunk_type),
            };
            out.push_str(&format!(
                "\n── {}:{}-{}{} [score: {:.2}] ──\n{}\n",
                r.file_path, r.start_line, r.end_line, name_part, r.score, r.content
            ));
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_point_id_deterministic() {
        let id1 = make_point_id("src/main.rs", 10);
        let id2 = make_point_id("src/main.rs", 10);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_make_point_id_different_inputs() {
        let id1 = make_point_id("src/main.rs", 10);
        let id2 = make_point_id("src/main.rs", 20);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_make_point_id_not_zero() {
        let id = make_point_id("src/main.rs", 1);
        assert_ne!(id, 0);
    }

    #[test]
    fn test_format_results_empty() {
        let out = SearchResult::format_results(&[], "test query");
        assert_eq!(out, "No results found for \"test query\".");
    }

    #[test]
    fn test_format_results_with_entries() {
        let results = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            chunk_name: Some("main".to_string()),
            chunk_type: "function".to_string(),
            language: "rust".to_string(),
            content: "fn main() {}".to_string(),
            start_line: 1,
            end_line: 3,
            score: 0.95,
        }];
        let out = SearchResult::format_results(&results, "entry point");
        assert!(out.contains("Found 1 results"));
        assert!(out.contains("src/main.rs:1-3"));
        assert!(out.contains("(function: main)"));
        assert!(out.contains("[score: 0.95]"));
        assert!(out.contains("fn main() {}"));
    }
}
