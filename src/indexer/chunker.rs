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
    let ts_language = get_tree_sitter_language(language)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_language)
        .context("Failed to set tree-sitter language")?;

    let tree = parser
        .parse(content, None)
        .context("Tree-sitter parse returned None")?;

    let root = tree.root_node();
    if root.has_error() {
        anyhow::bail!("Tree-sitter parse produced errors");
    }

    let node_types = get_extractable_node_types(language);
    let lines: Vec<&str> = content.lines().collect();
    let source = content.as_bytes();
    let mut chunks = Vec::new();

    extract_nodes(
        root,
        &node_types,
        &lines,
        source,
        relative_path,
        language,
        &mut chunks,
    );

    // Collect leftover lines (imports, constants, etc.) into "module" chunks
    let covered: Vec<bool> = {
        let mut covered = vec![false; lines.len()];
        for chunk in &chunks {
            for i in (chunk.start_line as usize - 1)..(chunk.end_line as usize) {
                if i < covered.len() {
                    covered[i] = true;
                }
            }
        }
        covered
    };

    let mut module_lines = Vec::new();
    let mut module_start: Option<usize> = None;

    for (i, &is_covered) in covered.iter().enumerate() {
        if !is_covered && !lines[i].trim().is_empty() {
            if module_start.is_none() {
                module_start = Some(i);
            }
            module_lines.push(lines[i]);
        } else if module_start.is_some() && is_covered && !module_lines.is_empty() {
            let start = module_start.unwrap();
            chunks.push(Chunk {
                file_path: relative_path.to_string(),
                language: language.as_str().to_string(),
                chunk_type: "module".to_string(),
                chunk_name: None,
                content: module_lines.join("\n"),
                start_line: (start + 1) as u32,
                end_line: i as u32,
            });
            module_lines.clear();
            module_start = None;
        }
    }

    // Flush any remaining module lines
    if !module_lines.is_empty() {
        if let Some(start) = module_start {
            chunks.push(Chunk {
                file_path: relative_path.to_string(),
                language: language.as_str().to_string(),
                chunk_type: "module".to_string(),
                chunk_name: None,
                content: module_lines.join("\n"),
                start_line: (start + 1) as u32,
                end_line: lines.len() as u32,
            });
        }
    }

    chunks.sort_by_key(|c| c.start_line);

    Ok(chunks)
}

fn extract_nodes(
    node: tree_sitter::Node,
    node_types: &[&str],
    lines: &[&str],
    source: &[u8],
    relative_path: &str,
    language: Language,
    chunks: &mut Vec<Chunk>,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        let kind = child.kind();

        if node_types.contains(&kind) {
            let start_line = child.start_position().row;
            let end_line = child.end_position().row;
            let line_count = end_line - start_line + 1;

            let chunk_content = lines[start_line..=end_line.min(lines.len() - 1)].join("\n");
            let chunk_name = extract_node_name(&child, source);
            let chunk_type = normalise_chunk_type(kind);

            if line_count > MAX_AST_CHUNK_LINES {
                let sub_chunks = chunk_by_lines(relative_path, &chunk_content, language);
                for mut sub in sub_chunks {
                    // sub chunks are 1-indexed relative to chunk_content;
                    // offset by start_line (0-indexed), subtract 1 to avoid double-counting
                    sub.start_line = sub.start_line + start_line as u32 - 1;
                    sub.end_line = sub.end_line + start_line as u32 - 1;
                    sub.chunk_type = chunk_type.clone();
                    sub.chunk_name = chunk_name.clone();
                    chunks.push(sub);
                }
            } else {
                chunks.push(Chunk {
                    file_path: relative_path.to_string(),
                    language: language.as_str().to_string(),
                    chunk_type,
                    chunk_name,
                    content: chunk_content,
                    start_line: (start_line + 1) as u32,
                    end_line: (end_line + 1) as u32,
                });
            }
        } else {
            // Recurse into children to find nested extractable nodes
            extract_nodes(child, node_types, lines, source, relative_path, language, chunks);
        }
    }
}

fn extract_node_name(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
        .or_else(|| {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" || child.kind() == "type_identifier" {
                    return child.utf8_text(source).ok().map(|s| s.to_string());
                }
            }
            None
        })
}

fn normalise_chunk_type(tree_sitter_kind: &str) -> String {
    match tree_sitter_kind {
        "function_definition" | "function_declaration" | "function_item" => "function".to_string(),
        "class_definition" | "class_declaration" => "class".to_string(),
        "method_definition" | "method_declaration" => "method".to_string(),
        "impl_item" => "impl".to_string(),
        "struct_item" => "struct".to_string(),
        "enum_item" => "enum".to_string(),
        "trait_item" => "trait".to_string(),
        "interface_declaration" => "interface".to_string(),
        "type_declaration" => "type".to_string(),
        "decorated_definition" => "function".to_string(),
        "arrow_function" => "function".to_string(),
        other => other.to_string(),
    }
}

fn get_tree_sitter_language(language: Language) -> Result<tree_sitter::Language> {
    match language {
        Language::Python => Ok(tree_sitter_python::LANGUAGE.into()),
        Language::JavaScript => Ok(tree_sitter_javascript::LANGUAGE.into()),
        Language::TypeScript => Ok(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::Go => Ok(tree_sitter_go::LANGUAGE.into()),
        Language::Rust => Ok(tree_sitter_rust::LANGUAGE.into()),
        Language::Java => Ok(tree_sitter_java::LANGUAGE.into()),
        Language::Other => anyhow::bail!("No tree-sitter grammar for 'Other'"),
    }
}

fn get_extractable_node_types(language: Language) -> Vec<&'static str> {
    match language {
        Language::Python => vec![
            "function_definition", "class_definition", "decorated_definition",
        ],
        Language::JavaScript => vec![
            "function_declaration", "class_declaration", "method_definition", "arrow_function",
        ],
        Language::TypeScript => vec![
            "function_declaration", "class_declaration", "method_definition", "arrow_function",
        ],
        Language::Go => vec![
            "function_declaration", "method_declaration", "type_declaration",
        ],
        Language::Rust => vec![
            "function_item", "impl_item", "struct_item", "enum_item", "trait_item",
        ],
        Language::Java => vec![
            "method_declaration", "class_declaration", "interface_declaration",
        ],
        Language::Other => vec![],
    }
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

    #[test]
    fn test_tree_sitter_python_functions() {
        let content = r#"import os

def hello():
    print("hello")

def world():
    print("world")

X = 42"#;
        let chunks = chunk_file("test.py", content, Language::Python);

        let names: Vec<Option<&str>> = chunks.iter()
            .map(|c| c.chunk_name.as_deref())
            .collect();

        assert!(names.contains(&Some("hello")), "Should extract hello function, got: {:?}", names);
        assert!(names.contains(&Some("world")), "Should extract world function, got: {:?}", names);
    }

    #[test]
    fn test_tree_sitter_rust_items() {
        let content = r#"use std::io;

pub struct Config {
    pub name: String,
}

impl Config {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

pub fn run(config: Config) {
    println!("{}", config.name);
}"#;
        let chunks = chunk_file("test.rs", content, Language::Rust);

        let types: Vec<&str> = chunks.iter().map(|c| c.chunk_type.as_str()).collect();
        assert!(types.contains(&"struct"), "Should extract struct, got: {:?}", types);
        assert!(types.contains(&"impl"), "Should extract impl block, got: {:?}", types);
        assert!(types.contains(&"function"), "Should extract function, got: {:?}", types);
    }

    #[test]
    fn test_tree_sitter_fallback_on_invalid_syntax() {
        let content = "this is not {{ valid python syntax ]]]]";
        let chunks = chunk_file("broken.py", content, Language::Python);

        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].chunk_type, "block");
    }
}
