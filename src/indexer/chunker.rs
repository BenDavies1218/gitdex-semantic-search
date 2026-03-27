use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::models::Chunk;
use super::language::Language;

const LINE_CHUNK_SIZE: usize = 100;
const LINE_CHUNK_OVERLAP: usize = 20;
const MAX_AST_CHUNK_LINES: usize = 200;

/// Chunk a file's contents. Uses tree-sitter for supported languages,
/// falls back to line-based chunking otherwise.
pub fn chunk_file(
    relative_path: &str,
    content: &str,
    language: Language,
) -> Vec<Chunk> {
    if language.has_tree_sitter_grammar() {
        match chunk_with_tree_sitter(relative_path, content, language) {
            Ok(chunks) if !chunks.is_empty() => return chunks,
            Ok(_) => {
                debug!("Tree-sitter produced no chunks for {}, falling back", relative_path);
            }
            Err(err) => {
                warn!("Tree-sitter failed for {}: {}, falling back to line chunking", relative_path, err);
            }
        }
    }

    chunk_by_lines(relative_path, content, language)
}

/// Split content into fixed-size line chunks with overlap.
pub fn chunk_by_lines(
    relative_path: &str,
    content: &str,
    language: Language,
) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return vec![];
    }

    // If the file fits in one chunk, return it whole
    if lines.len() <= LINE_CHUNK_SIZE {
        return vec![Chunk {
            file_path: relative_path.to_string(),
            language: language.as_str().to_string(),
            chunk_type: "block".to_string(),
            chunk_name: None,
            content: content.to_string(),
            start_line: 1,
            end_line: lines.len() as u32,
        }];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < lines.len() {
        let end = (start + LINE_CHUNK_SIZE).min(lines.len());
        let chunk_content = lines[start..end].join("\n");

        chunks.push(Chunk {
            file_path: relative_path.to_string(),
            language: language.as_str().to_string(),
            chunk_type: "block".to_string(),
            chunk_name: None,
            content: chunk_content,
            start_line: (start + 1) as u32,
            end_line: end as u32,
        });

        if end >= lines.len() {
            break;
        }

        start = end - LINE_CHUNK_OVERLAP;
    }

    chunks
}

/// Chunk using tree-sitter AST parsing.
fn chunk_with_tree_sitter(
    relative_path: &str,
    content: &str,
    language: Language,
) -> Result<Vec<Chunk>> {
    // TODO: implement in Task 8
    anyhow::bail!("Tree-sitter chunking not yet implemented")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_empty_content() {
        let chunks = chunk_by_lines("empty.txt", "", Language::Other);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_small_file_single_chunk() {
        let content = (1..=50).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_by_lines("small.py", &content, Language::Python);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 50);
        assert_eq!(chunks[0].chunk_type, "block");
        assert_eq!(chunks[0].language, "python");
    }

    #[test]
    fn test_chunk_exact_100_lines() {
        let content = (1..=100).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_by_lines("exact.rs", &content, Language::Rust);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].end_line, 100);
    }

    #[test]
    fn test_chunk_overlap() {
        let content = (1..=150).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_by_lines("overlap.go", &content, Language::Go);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 100);
        assert_eq!(chunks[1].start_line, 81);
        assert_eq!(chunks[1].end_line, 150);
    }

    #[test]
    fn test_chunk_large_file_multiple_chunks() {
        let content = (1..=350).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_by_lines("large.js", &content, Language::JavaScript);

        assert!(chunks.len() >= 3);

        for window in chunks.windows(2) {
            let gap = window[1].start_line - window[0].start_line;
            assert_eq!(gap, (LINE_CHUNK_SIZE - LINE_CHUNK_OVERLAP) as u32);
        }
    }

    #[test]
    fn test_chunk_file_falls_back_for_unsupported() {
        let content = "key: value\nother: stuff";
        let chunks = chunk_file("config.yaml", content, Language::Other);

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, "block");
    }
}
