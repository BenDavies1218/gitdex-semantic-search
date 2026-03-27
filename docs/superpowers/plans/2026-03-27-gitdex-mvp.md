# Gitdex MVP Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust binary that indexes a code repository into Qdrant vectors and exposes semantic search via MCP tools.

**Architecture:** Single Rust binary with two CLI modes (`index` and `serve`). The indexer walks a repo, chunks code with tree-sitter, embeds via Ollama, and upserts to Qdrant. The MCP server exposes `search_code`, `index_repo`, and `get_index_status` over stdio transport.

**Tech Stack:** Rust, clap, tokio, reqwest, qdrant-client, tree-sitter (+ language grammar crates), rmcp, tracing, anyhow, ignore, serde

**Spec:** `docs/superpowers/specs/2026-03-27-gitdex-mvp-design.md`

---

## Chunk 1: Project Scaffold, Config, Models, CLI

### Task 1: Initialise Cargo project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Initialise the Rust project**

```bash
cd /Users/benjamindavies/Documents/GitHub/gitdex
cargo init --name gitdex
```

- [ ] **Step 2: Set up Cargo.toml with all dependencies**

Replace `Cargo.toml` contents with:

```toml
[package]
name = "gitdex"
version = "0.1.0"
edition = "2021"
description = "Local code indexer with MCP-based semantic search"

[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# Async
tokio = { version = "1", features = ["full"] }

# HTTP
reqwest = { version = "0.12", features = ["json"] }

# Serialisation
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Error handling
anyhow = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# File walking
ignore = "0.4"

# Vector DB
qdrant-client = "1"

# MCP
rmcp = { version = "0.1", features = ["server", "transport-io"] }

# Tree-sitter (all grammar crates must target the same tree-sitter version)
tree-sitter = "0.23"
tree-sitter-python = "0.23"
tree-sitter-javascript = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-go = "0.23"
tree-sitter-rust = "0.23"
tree-sitter-java = "0.23"

# Time
chrono = { version = "0.4", features = ["serde"] }
```

Note: Pin exact crate versions after verifying compatibility during implementation. The versions above are starting points — check crates.io for the latest compatible versions of `rmcp`, `qdrant-client`, and the tree-sitter grammar crates.

- [ ] **Step 3: Verify it compiles**

```bash
cargo check
```

Expected: compiles with no errors (warnings are OK at this stage). If any dependency versions conflict, resolve by checking crates.io for compatible versions.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "feat: initialise Cargo project with dependencies"
```

---

### Task 2: Config module

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write config module with defaults and tests**

Create `src/config.rs` with:

```rust
use std::path::PathBuf;

pub const EMBEDDING_MODEL: &str = "nomic-embed-text";
pub const EMBEDDING_DIMENSIONS: u64 = 768;

#[derive(Debug, Clone)]
pub struct Config {
    pub repo_path: PathBuf,
    pub qdrant_url: String,
    pub ollama_url: String,
    pub collection_name: String,
    pub embed_concurrency: usize,
    pub upsert_batch_size: usize,
    pub max_file_size: u64,
    pub verbose: bool,
}

impl Config {
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            repo_path,
            qdrant_url: std::env::var("GITDEX_QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6333".to_string()),
            ollama_url: std::env::var("GITDEX_OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            collection_name: std::env::var("GITDEX_COLLECTION")
                .unwrap_or_else(|_| "repo_index".to_string()),
            embed_concurrency: 20,
            upsert_batch_size: 100,
            max_file_size: 1_048_576, // 1MB
            verbose: false,
        }
    }

    pub fn repo_name(&self) -> String {
        self.repo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::new(PathBuf::from("/tmp/test-repo"));
        assert_eq!(config.qdrant_url, "http://localhost:6333");
        assert_eq!(config.ollama_url, "http://localhost:11434");
        assert_eq!(config.collection_name, "repo_index");
        assert_eq!(config.embed_concurrency, 20);
        assert_eq!(config.upsert_batch_size, 100);
        assert_eq!(config.max_file_size, 1_048_576);
        assert!(!config.verbose);
    }

    #[test]
    fn test_repo_name_from_path() {
        let config = Config::new(PathBuf::from("/home/user/my-project"));
        assert_eq!(config.repo_name(), "my-project");
    }

    #[test]
    fn test_repo_name_root_path() {
        let config = Config::new(PathBuf::from("/"));
        assert_eq!(config.repo_name(), "unknown");
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

```bash
cargo test config::tests -- --nocapture
```

Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: add config module with defaults and env var support"
```

---

### Task 3: Shared models

**Files:**
- Create: `src/models.rs`

- [ ] **Step 1: Create models with basic tests**

Create `src/models.rs`:

```rust
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
        // We can't easily force a zero hash, but we verify the guard logic exists
        // by checking that typical inputs don't produce 0
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
```

- [ ] **Step 2: Run tests**

```bash
cargo test models::tests -- --nocapture
```

Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/models.rs
git commit -m "feat: add shared models (Chunk, SearchResult, IndexResult, point ID)"
```

---

### Task 4: CLI with clap

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create CLI definitions**

Create `src/cli.rs`:

```rust
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "gitdex", about = "Local code indexer with MCP-based semantic search")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Index a code repository
    Index {
        /// Path to the repository to index
        repo_path: PathBuf,

        /// Qdrant server URL
        #[arg(long, env = "GITDEX_QDRANT_URL", default_value = "http://localhost:6333")]
        qdrant_url: String,

        /// Ollama server URL
        #[arg(long, env = "GITDEX_OLLAMA_URL", default_value = "http://localhost:11434")]
        ollama_url: String,

        /// Qdrant collection name
        #[arg(long, env = "GITDEX_COLLECTION", default_value = "repo_index")]
        collection: String,

        /// Enable debug-level logging
        #[arg(short, long)]
        verbose: bool,
    },

    /// Start MCP server over stdio
    Serve {
        /// Path to the repository this server is scoped to
        repo_path: PathBuf,

        /// Qdrant server URL
        #[arg(long, env = "GITDEX_QDRANT_URL", default_value = "http://localhost:6333")]
        qdrant_url: String,

        /// Ollama server URL
        #[arg(long, env = "GITDEX_OLLAMA_URL", default_value = "http://localhost:11434")]
        ollama_url: String,

        /// Qdrant collection name
        #[arg(long, env = "GITDEX_COLLECTION", default_value = "repo_index")]
        collection: String,

        /// Enable debug-level logging
        #[arg(short, long)]
        verbose: bool,
    },
}
```

- [ ] **Step 2: Wire up main.rs**

Replace `src/main.rs` with:

```rust
mod cli;
mod config;
mod models;

use clap::Parser;
use cli::{Cli, Command};
use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Index {
            repo_path,
            qdrant_url,
            ollama_url,
            collection,
            verbose,
        } => {
            let mut config = Config::new(repo_path);
            config.qdrant_url = qdrant_url;
            config.ollama_url = ollama_url;
            config.collection_name = collection;
            config.verbose = verbose;

            init_logging(verbose, false);
            tracing::info!("Indexing {}", config.repo_path.display());

            // TODO: run indexing pipeline
            eprintln!("Indexing not yet implemented");
        }
        Command::Serve {
            repo_path,
            qdrant_url,
            ollama_url,
            collection,
            verbose,
        } => {
            let mut config = Config::new(repo_path);
            config.qdrant_url = qdrant_url;
            config.ollama_url = ollama_url;
            config.collection_name = collection;
            config.verbose = verbose;

            init_logging(verbose, true);

            // TODO: start MCP server
            eprintln!("MCP server not yet implemented");
        }
    }

    Ok(())
}

fn init_logging(verbose: bool, is_serve: bool) {
    use tracing_subscriber::EnvFilter;

    let filter = if verbose {
        EnvFilter::new("debug")
    } else if is_serve {
        EnvFilter::new("warn")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
```

- [ ] **Step 3: Verify it compiles and CLI works**

```bash
cargo build
cargo run -- --help
cargo run -- index --help
cargo run -- serve --help
```

Expected: help text shows both subcommands with correct arguments.

- [ ] **Step 4: Commit**

```bash
git add src/cli.rs src/main.rs
git commit -m "feat: add CLI with index and serve subcommands"
```

---

## Chunk 2: File Walking & Language Detection

### Task 5: Language detection

**Files:**
- Create: `src/indexer/language.rs`
- Create: `src/indexer/mod.rs`

- [ ] **Step 1: Create the indexer module and language detection with tests**

Create `src/indexer/mod.rs`:

```rust
pub mod language;
pub mod walker;
pub mod chunker;
```

Create `src/indexer/chunker.rs` as a stub (implemented fully in Task 7):

```rust
// Stub — full implementation in Task 7
use crate::models::Chunk;
use super::language::Language;

pub fn chunk_file(_relative_path: &str, _content: &str, _language: Language) -> Vec<Chunk> {
    vec![]
}
```

Create `src/indexer/language.rs`:

```rust
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Go,
    Rust,
    Java,
    Other,
}

impl Language {
    pub fn from_extension(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("py") => Language::Python,
            Some("js" | "jsx") => Language::JavaScript,
            Some("ts" | "tsx") => Language::TypeScript,
            Some("go") => Language::Go,
            Some("rs") => Language::Rust,
            Some("java") => Language::Java,
            _ => Language::Other,
        }
    }

    pub fn has_tree_sitter_grammar(&self) -> bool {
        !matches!(self, Language::Other)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Go => "go",
            Language::Rust => "rust",
            Language::Java => "java",
            Language::Other => "other",
        }
    }
}

/// File extensions that are considered indexable text files when Language is Other.
const INDEXABLE_EXTENSIONS: &[&str] = &[
    "md", "yaml", "yml", "toml", "json", "sql", "sh", "bash",
    "c", "cpp", "cc", "h", "hpp", "cs", "rb", "php", "swift",
    "kt", "scala", "r", "lua", "zig", "nim", "ex", "exs",
    "html", "css", "scss", "xml", "graphql", "proto", "tf",
    "dockerfile", "makefile",
];

/// Check if a file should be indexed based on its extension.
pub fn is_indexable(path: &Path) -> bool {
    let lang = Language::from_extension(path);
    if lang.has_tree_sitter_grammar() {
        return true;
    }

    // Check if the extension is in the indexable list
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        return INDEXABLE_EXTENSIONS.contains(&ext.to_lowercase().as_str());
    }

    // Files without extensions: check for known names
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let lower = name.to_lowercase();
        return lower == "dockerfile" || lower == "makefile";
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_python_detection() {
        assert_eq!(Language::from_extension(Path::new("main.py")), Language::Python);
    }

    #[test]
    fn test_javascript_detection() {
        assert_eq!(Language::from_extension(Path::new("app.js")), Language::JavaScript);
        assert_eq!(Language::from_extension(Path::new("App.jsx")), Language::JavaScript);
    }

    #[test]
    fn test_typescript_detection() {
        assert_eq!(Language::from_extension(Path::new("app.ts")), Language::TypeScript);
        assert_eq!(Language::from_extension(Path::new("App.tsx")), Language::TypeScript);
    }

    #[test]
    fn test_go_detection() {
        assert_eq!(Language::from_extension(Path::new("main.go")), Language::Go);
    }

    #[test]
    fn test_rust_detection() {
        assert_eq!(Language::from_extension(Path::new("lib.rs")), Language::Rust);
    }

    #[test]
    fn test_java_detection() {
        assert_eq!(Language::from_extension(Path::new("Main.java")), Language::Java);
    }

    #[test]
    fn test_other_known_extension() {
        assert_eq!(Language::from_extension(Path::new("config.yaml")), Language::Other);
        assert!(is_indexable(Path::new("config.yaml")));
    }

    #[test]
    fn test_unknown_extension() {
        assert_eq!(Language::from_extension(Path::new("image.png")), Language::Other);
        assert!(!is_indexable(Path::new("image.png")));
    }

    #[test]
    fn test_tree_sitter_grammar_available() {
        assert!(Language::Python.has_tree_sitter_grammar());
        assert!(Language::Rust.has_tree_sitter_grammar());
        assert!(!Language::Other.has_tree_sitter_grammar());
    }

    #[test]
    fn test_dockerfile_no_extension() {
        assert!(is_indexable(Path::new("Dockerfile")));
    }

    #[test]
    fn test_as_str() {
        assert_eq!(Language::Python.as_str(), "python");
        assert_eq!(Language::TypeScript.as_str(), "typescript");
    }
}
```

- [ ] **Step 2: Add `mod indexer;` to main.rs**

Add after the existing mod declarations in `src/main.rs`:

```rust
mod indexer;
```

- [ ] **Step 3: Run tests**

```bash
cargo test indexer::language::tests -- --nocapture
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/indexer/
git commit -m "feat: add language detection with tree-sitter grammar mapping"
```

---

### Task 6: File walker

**Files:**
- Create: `src/indexer/walker.rs`

- [ ] **Step 1: Implement walker with tests**

Create `src/indexer/walker.rs`:

```rust
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use super::language::{is_indexable, Language};

/// A file discovered during walking, with its detected language.
#[derive(Debug)]
pub struct WalkedFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub language: Language,
}

/// Directories to always skip (in addition to .gitignore rules).
const SKIP_DIRS: &[&str] = &[
    "node_modules", "vendor", "__pycache__", ".venv", "dist",
    "build", ".next", "target", ".cargo", ".git",
];

/// File names to always skip.
const SKIP_FILES: &[&str] = &[
    "package-lock.json", "poetry.lock", "Cargo.lock",
    "yarn.lock", "pnpm-lock.yaml",
];

/// Extensions to always skip (binary/non-text).
const SKIP_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "ico", "svg", "webp",
    "wasm", "exe", "dll", "so", "dylib", "a", "o", "obj",
    "zip", "tar", "gz", "bz2", "xz", "7z", "rar",
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx",
    "mp3", "mp4", "avi", "mov", "wav", "flac",
    "ttf", "otf", "woff", "woff2", "eot",
    "pyc", "pyo", "class",
    "min.js", "min.css",
];

/// Walk a repository and return all indexable files.
pub fn walk_repo(repo_path: &Path, max_file_size: u64) -> Result<Vec<WalkedFile>> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("Repository path not found: {}", repo_path.display()))?;

    let mut files = Vec::new();

    let walker = WalkBuilder::new(&repo_path)
        .hidden(true) // skip hidden files by default
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                warn!("Walk error: {}", err);
                continue;
            }
        };

        // Skip directories
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(true) {
            continue;
        }

        let path = entry.path();

        // Skip if in a blocked directory
        if path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(|s| SKIP_DIRS.contains(&s))
                .unwrap_or(false)
        }) {
            continue;
        }

        // Skip by file name
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if SKIP_FILES.contains(&name) {
                debug!("Skipping lock file: {}", path.display());
                continue;
            }
            // Skip minified files
            if name.ends_with(".min.js") || name.ends_with(".min.css") {
                debug!("Skipping minified: {}", path.display());
                continue;
            }
        }

        // Skip by extension
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if SKIP_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                continue;
            }
        }

        // Skip files over size limit
        if let Ok(metadata) = path.metadata() {
            if metadata.len() > max_file_size {
                debug!("Skipping large file ({}B): {}", metadata.len(), path.display());
                continue;
            }
        }

        // Check if file is indexable
        if !is_indexable(path) {
            continue;
        }

        let relative_path = path
            .strip_prefix(&repo_path)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let language = Language::from_extension(path);

        files.push(WalkedFile {
            path: path.to_path_buf(),
            relative_path,
            language,
        });
    }

    tracing::info!("Walked {} indexable files", files.len());
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_repo(dir: &Path) {
        // Create a .git dir so it looks like a repo
        fs::create_dir_all(dir.join(".git")).unwrap();

        // Create indexable files
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("src/lib.py"), "def foo(): pass").unwrap();
        fs::write(dir.join("README.md"), "# Hello").unwrap();

        // Create files that should be skipped
        fs::write(dir.join("image.png"), "fake png").unwrap();
        fs::write(dir.join("Cargo.lock"), "lock contents").unwrap();
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("node_modules/pkg/index.js"), "module.exports = {}").unwrap();
    }

    #[test]
    fn test_walk_finds_indexable_files() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_repo(tmp.path());

        let files = walk_repo(tmp.path(), 1_048_576).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

        assert!(paths.contains(&"src/main.rs"), "Should find Rust files");
        assert!(paths.contains(&"src/lib.py"), "Should find Python files");
        assert!(paths.contains(&"README.md"), "Should find markdown files");
    }

    #[test]
    fn test_walk_skips_binary_and_lock_files() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_repo(tmp.path());

        let files = walk_repo(tmp.path(), 1_048_576).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

        assert!(!paths.contains(&"image.png"), "Should skip PNG files");
        assert!(!paths.contains(&"Cargo.lock"), "Should skip lock files");
    }

    #[test]
    fn test_walk_skips_node_modules() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_repo(tmp.path());

        let files = walk_repo(tmp.path(), 1_048_576).unwrap();
        let has_node_modules = files.iter().any(|f| f.relative_path.contains("node_modules"));

        assert!(!has_node_modules, "Should skip node_modules");
    }

    #[test]
    fn test_walk_skips_large_files() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_repo(tmp.path());

        // Create a large file
        let big_content = "x".repeat(2_000_000);
        fs::write(tmp.path().join("big.rs"), big_content).unwrap();

        let files = walk_repo(tmp.path(), 1_048_576).unwrap();
        let has_big = files.iter().any(|f| f.relative_path == "big.rs");

        assert!(!has_big, "Should skip files over 1MB");
    }

    #[test]
    fn test_walk_detects_language() {
        let tmp = tempfile::tempdir().unwrap();
        create_test_repo(tmp.path());

        let files = walk_repo(tmp.path(), 1_048_576).unwrap();
        let rs_file = files.iter().find(|f| f.relative_path == "src/main.rs").unwrap();
        assert_eq!(rs_file.language, Language::Rust);

        let py_file = files.iter().find(|f| f.relative_path == "src/lib.py").unwrap();
        assert_eq!(py_file.language, Language::Python);
    }

    #[test]
    fn test_walk_nonexistent_path() {
        let result = walk_repo(Path::new("/nonexistent/path"), 1_048_576);
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Add `tempfile` as a dev dependency**

Add to `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run tests**

```bash
cargo test indexer::walker::tests -- --nocapture
```

Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/indexer/walker.rs Cargo.toml Cargo.lock
git commit -m "feat: add file walker with skip-lists and .gitignore support"
```

---

## Chunk 3: Chunking (Line-Based and AST)

### Task 7: Line-based fallback chunker

**Files:**
- Create: `src/indexer/chunker.rs`

- [ ] **Step 1: Implement line-based chunking with tests**

Create `src/indexer/chunker.rs`:

```rust
use anyhow::{Context, Result};
use std::path::Path;
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

        // First chunk: lines 1-100
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 100);

        // Second chunk: starts at 81 (100 - 20 overlap + 1)
        assert_eq!(chunks[1].start_line, 81);
        assert_eq!(chunks[1].end_line, 150);
    }

    #[test]
    fn test_chunk_large_file_multiple_chunks() {
        let content = (1..=350).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let chunks = chunk_by_lines("large.js", &content, Language::JavaScript);

        // Should produce multiple chunks with overlap
        assert!(chunks.len() >= 3);

        // Verify no gaps: each chunk's start is at most LINE_CHUNK_SIZE - LINE_CHUNK_OVERLAP
        // after the previous chunk's start
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
```

- [ ] **Step 2: Run tests**

```bash
cargo test indexer::chunker::tests -- --nocapture
```

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/indexer/chunker.rs
git commit -m "feat: add line-based fallback chunker with overlap"
```

---

### Task 8: Tree-sitter AST chunking

**Files:**
- Modify: `src/indexer/chunker.rs`

- [ ] **Step 1: Write tree-sitter chunking tests**

Add these tests to the existing `mod tests` block in `src/indexer/chunker.rs`:

```rust
    #[test]
    fn test_tree_sitter_python_functions() {
        let content = r#"
import os

def hello():
    print("hello")

def world():
    print("world")

X = 42
"#.trim();
        let chunks = chunk_file("test.py", content, Language::Python);

        let names: Vec<Option<&str>> = chunks.iter()
            .map(|c| c.chunk_name.as_deref())
            .collect();

        assert!(names.contains(&Some("hello")), "Should extract hello function");
        assert!(names.contains(&Some("world")), "Should extract world function");
    }

    #[test]
    fn test_tree_sitter_rust_items() {
        let content = r#"
use std::io;

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
}
"#.trim();
        let chunks = chunk_file("test.rs", content, Language::Rust);

        let types: Vec<&str> = chunks.iter().map(|c| c.chunk_type.as_str()).collect();
        assert!(types.contains(&"struct"), "Should extract struct");
        assert!(types.contains(&"impl"), "Should extract impl block");
        assert!(types.contains(&"function"), "Should extract function");
    }

    #[test]
    fn test_tree_sitter_fallback_on_invalid_syntax() {
        let content = "this is not {{ valid python syntax ]]]]";
        let chunks = chunk_file("broken.py", content, Language::Python);

        // Should fall back to line chunking
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].chunk_type, "block");
    }
```

- [ ] **Step 2: Implement `chunk_with_tree_sitter`**

Replace the `chunk_with_tree_sitter` function stub and add supporting code in `src/indexer/chunker.rs`:

```rust
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

    // Collect leftover lines (imports, constants, etc.) into a "module" chunk
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
        } else if module_start.is_some() && is_covered {
            // Flush the accumulated module chunk
            if !module_lines.is_empty() {
                let start = module_start.unwrap();
                chunks.push(Chunk {
                    file_path: relative_path.to_string(),
                    language: language.as_str().to_string(),
                    chunk_type: "module".to_string(),
                    chunk_name: None,
                    content: module_lines.join("\n"),
                    start_line: (start + 1) as u32,
                    end_line: i as u32, // i is the first covered line (0-indexed), so i as u32 = 1-indexed last uncovered line
                });
                module_lines.clear();
                module_start = None;
            }
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

    // Sort chunks by start line
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
                // Split large nodes into sub-chunks
                let sub_chunks = chunk_by_lines(relative_path, &chunk_content, language);
                for mut sub in sub_chunks {
                    // Adjust line numbers: sub chunks are 1-indexed relative to chunk_content,
                    // add the 0-indexed start_line offset, subtract 1 to avoid double-counting
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
        "decorated_definition" => "function".to_string(), // usually wraps a function
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
```

- [ ] **Step 3: Run tests**

```bash
cargo test indexer::chunker::tests -- --nocapture
```

Expected: all 9 tests pass (6 existing + 3 new).

- [ ] **Step 4: Commit**

```bash
git add src/indexer/chunker.rs
git commit -m "feat: add tree-sitter AST chunking for 6 languages"
```

---

## Chunk 4: Embeddings Client

### Task 9: Ollama embeddings client

**Files:**
- Create: `src/embeddings.rs`

- [ ] **Step 1: Implement the embedding client**

Create `src/embeddings.rs`:

```rust
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

const EMBEDDING_MODEL: &str = "nomic-embed-text";

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    prompt: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

pub struct EmbeddingClient {
    http: Client,
    ollama_url: String,
    semaphore: Arc<Semaphore>,
}

impl EmbeddingClient {
    pub fn new(ollama_url: String, concurrency: usize) -> Self {
        Self {
            http: Client::new(),
            ollama_url,
            semaphore: Arc::new(Semaphore::new(concurrency)),
        }
    }

    /// Test that Ollama is reachable and the model is available.
    pub async fn health_check(&self) -> Result<()> {
        self.embed("health check")
            .await
            .context(
                format!(
                    "Cannot reach Ollama at {}. Is it running? Try: ollama serve",
                    self.ollama_url
                )
            )?;
        Ok(())
    }

    /// Embed a single text string.
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/api/embeddings", self.ollama_url);
        let request = EmbedRequest {
            model: EMBEDDING_MODEL.to_string(),
            prompt: text.to_string(),
        };

        let mut last_err = None;

        for attempt in 0..3 {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(100 * 2u64.pow(attempt));
                tokio::time::sleep(delay).await;
            }

            match self.http.post(&url).json(&request).send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let body: EmbedResponse = resp.json().await
                            .context("Failed to parse Ollama embedding response")?;
                        return Ok(body.embedding);
                    }
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    last_err = Some(anyhow::anyhow!("Ollama returned {}: {}", status, body));
                }
                Err(err) => {
                    last_err = Some(err.into());
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Embedding failed after retries")))
    }

    /// Embed multiple texts with bounded concurrency.
    /// Prepends file_path to each text for context.
    pub async fn embed_many(
        &self,
        items: Vec<(String, String)>, // (file_path, content)
    ) -> Result<Vec<Option<Vec<f32>>>> {
        let mut handles = Vec::with_capacity(items.len());

        for (file_path, content) in items {
            let sem = self.semaphore.clone();
            let client = Self {
                http: self.http.clone(),
                ollama_url: self.ollama_url.clone(),
                semaphore: self.semaphore.clone(),
            };

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let text = format!("{}\n{}", file_path, content);
                match client.embed(&text).await {
                    Ok(vec) => Some(vec),
                    Err(err) => {
                        warn!("Failed to embed chunk from {}: {}", file_path, err);
                        None
                    }
                }
            });

            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            results.push(handle.await.context("Embedding task panicked")?);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_request_serialises() {
        let req = EmbedRequest {
            model: "nomic-embed-text".to_string(),
            prompt: "hello world".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("nomic-embed-text"));
        assert!(json.contains("hello world"));
    }

    #[test]
    fn test_embed_response_deserialises() {
        let json = r#"{"embedding": [0.1, 0.2, 0.3]}"#;
        let resp: EmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.embedding.len(), 3);
        assert!((resp.embedding[0] - 0.1).abs() < f32::EPSILON);
    }

    // Integration tests (require Ollama running) go in tests/ directory
    // and are gated behind a feature flag or ignored by default
}
```

- [ ] **Step 2: Add `mod embeddings;` to main.rs**

- [ ] **Step 3: Run tests**

```bash
cargo test embeddings::tests -- --nocapture
```

Expected: 2 tests pass (serialisation only — no network calls in unit tests).

- [ ] **Step 4: Commit**

```bash
git add src/embeddings.rs src/main.rs
git commit -m "feat: add Ollama embedding client with retry and concurrency"
```

---

## Chunk 5: Qdrant Client

### Task 10: Qdrant operations

**Files:**
- Create: `src/qdrant.rs`

- [ ] **Step 1: Implement the Qdrant client wrapper**

Create `src/qdrant.rs`:

```rust
use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, PointStruct, ScalarQuantizationBuilder,
    SearchPointsBuilder, VectorParamsBuilder, DeletePointsBuilder,
    PointId, Filter, Condition, FieldCondition, Match, Value,
    UpsertPointsBuilder, vectors_config::Config as VectorsConfig,
    CreateFieldIndexCollectionBuilder, FieldType,
    with_payload_selector::SelectorOptions, WithPayloadSelector,
};
use qdrant_client::Qdrant;
use serde_json::json;
use tracing::info;

use crate::config::EMBEDDING_DIMENSIONS;
use crate::models::{SearchResult, make_point_id};

const METADATA_POINT_ID: u64 = 0;

pub struct QdrantClient {
    client: Qdrant,
    collection_name: String,
}

impl QdrantClient {
    pub async fn new(url: &str, collection_name: String) -> Result<Self> {
        let client = Qdrant::from_url(url)
            .build()
            .context(format!("Failed to connect to Qdrant at {}", url))?;

        Ok(Self {
            client,
            collection_name,
        })
    }

    /// Check if Qdrant is reachable.
    pub async fn health_check(&self) -> Result<()> {
        self.client
            .health_check()
            .await
            .context("Qdrant health check failed. Is Qdrant running?")?;
        Ok(())
    }

    /// Ensure the collection exists with correct config.
    pub async fn ensure_collection(&self) -> Result<()> {
        let exists = self.client
            .collection_exists(&self.collection_name)
            .await
            .context("Failed to check collection existence")?;

        if exists {
            // Verify dimension matches
            let info = self.client
                .collection_info(&self.collection_name)
                .await
                .context("Failed to get collection info")?;

            // Verify vector config matches expected dimensions
            if let Some(config) = &info.result {
                if let Some(vectors_config) = &config.config {
                    if let Some(params_map) = &vectors_config.params {
                        // Check dimension in the config
                        // The exact API may vary — verify at implementation time
                    }
                }
            }

            info!("Using existing collection '{}'", self.collection_name);
        } else {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name)
                        .vectors_config(
                            VectorParamsBuilder::new(
                                EMBEDDING_DIMENSIONS,
                                Distance::Cosine,
                            )
                        ),
                )
                .await
                .context("Failed to create collection")?;

            // Create payload indexes
            self.client
                .create_field_index(
                    CreateFieldIndexCollectionBuilder::new(
                        &self.collection_name,
                        "language",
                        FieldType::Keyword,
                    )
                )
                .await
                .context("Failed to create language index")?;

            self.client
                .create_field_index(
                    CreateFieldIndexCollectionBuilder::new(
                        &self.collection_name,
                        "file_path",
                        FieldType::Text,
                    )
                )
                .await
                .context("Failed to create file_path index")?;

            info!("Created collection '{}' with indexes", self.collection_name);
        }

        Ok(())
    }

    /// Delete all points matching a given file path.
    pub async fn delete_by_file_path(&self, file_path: &str) -> Result<()> {
        let filter = Filter::must([Condition::matches(
            "file_path",
            file_path.to_string(),
        )]);

        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name)
                    .points(filter),
            )
            .await
            .context(format!("Failed to delete points for {}", file_path))?;

        Ok(())
    }

    /// Upsert a batch of points. Retries once on failure per spec.
    pub async fn upsert_batch(
        &self,
        points: Vec<PointStruct>,
    ) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }

        let result = self.client
            .upsert_points(
                UpsertPointsBuilder::new(&self.collection_name, points.clone())
            )
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(first_err) => {
                tracing::warn!("Upsert failed, retrying once: {}", first_err);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                self.client
                    .upsert_points(
                        UpsertPointsBuilder::new(&self.collection_name, points)
                    )
                    .await
                    .context("Upsert failed after retry")?;

                Ok(())
            }
        }
    }

    /// Store indexing metadata (commit hash, timestamp).
    pub async fn upsert_metadata(
        &self,
        last_commit: &str,
        last_indexed: &str,
    ) -> Result<()> {
        let point = PointStruct::new(
            METADATA_POINT_ID,
            vec![0.0f32; EMBEDDING_DIMENSIONS as usize],
            json!({
                "type": "metadata",
                "last_commit": last_commit,
                "last_indexed": last_indexed,
            })
            .try_into()
            .unwrap(),
        );

        self.upsert_batch(vec![point]).await
    }

    /// Search for similar vectors with optional filters.
    pub async fn search(
        &self,
        vector: Vec<f32>,
        top_k: u64,
        language: Option<&str>,
        file_path_prefix: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        // Check collection exists
        let exists = self.client
            .collection_exists(&self.collection_name)
            .await
            .context("Failed to check collection")?;

        if !exists {
            anyhow::bail!("No index found. Run index_repo first.");
        }

        let mut must_conditions = Vec::new();
        let mut must_not_conditions = Vec::new();

        // Exclude metadata point
        must_not_conditions.push(Condition::matches("type", "metadata".to_string()));

        if let Some(lang) = language {
            must_conditions.push(Condition::matches("language", lang.to_string()));
        }

        if let Some(prefix) = file_path_prefix {
            must_conditions.push(Condition::matches_text("file_path", prefix));
        }

        let filter = Some(Filter {
            must: must_conditions,
            must_not: must_not_conditions,
            ..Default::default()
        });

        let mut builder = SearchPointsBuilder::new(
            &self.collection_name,
            vector,
            top_k,
        ).with_payload(true);

        if let Some(f) = filter {
            builder = builder.filter(f);
        }

        let results = self.client
            .search_points(builder)
            .await
            .context("Qdrant search failed")?;

        let search_results = results
            .result
            .into_iter()
            .map(|point| {
                let payload = point.payload;
                SearchResult {
                    file_path: get_payload_string(&payload, "file_path"),
                    chunk_name: get_payload_string_opt(&payload, "chunk_name"),
                    chunk_type: get_payload_string(&payload, "chunk_type"),
                    language: get_payload_string(&payload, "language"),
                    content: get_payload_string(&payload, "content"),
                    start_line: get_payload_u32(&payload, "start_line"),
                    end_line: get_payload_u32(&payload, "end_line"),
                    score: point.score,
                }
            })
            .collect();

        Ok(search_results)
    }

    /// Get index status (metadata point + collection info).
    pub async fn get_status(&self, repo_path: &str) -> Result<crate::models::IndexStatus> {
        let exists = self.client
            .collection_exists(&self.collection_name)
            .await?;

        if !exists {
            return Ok(crate::models::IndexStatus {
                last_commit: None,
                last_indexed: None,
                total_points: 0,
                collection_name: self.collection_name.clone(),
                repo_path: repo_path.to_string(),
            });
        }

        let info = self.client.collection_info(&self.collection_name).await?;
        let total_points = info.result
            .map(|r| r.points_count.unwrap_or(0))
            .unwrap_or(0);

        // Try to retrieve the metadata point
        let metadata = self.client
            .get_points(
                &self.collection_name,
                None,
                &[METADATA_POINT_ID.into()],
                Some(true),
                None,
            )
            .await
            .ok()
            .and_then(|r| r.result.into_iter().next());

        let (last_commit, last_indexed) = match metadata {
            Some(point) => (
                get_payload_string_opt(&point.payload, "last_commit"),
                get_payload_string_opt(&point.payload, "last_indexed"),
            ),
            None => (None, None),
        };

        Ok(crate::models::IndexStatus {
            last_commit,
            last_indexed,
            total_points: total_points.saturating_sub(1), // exclude metadata point
            collection_name: self.collection_name.clone(),
            repo_path: repo_path.to_string(),
        })
    }
}

fn get_payload_string(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> String {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn get_payload_string_opt(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_payload_u32(
    payload: &std::collections::HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> u32 {
    payload
        .get(key)
        .and_then(|v| v.as_integer())
        .unwrap_or(0) as u32
}
```

**Note**: The exact Qdrant Rust client API may differ from what's shown above. During implementation, consult the `qdrant-client` crate docs at https://docs.rs/qdrant-client and adjust struct names, builder patterns, and method signatures accordingly. In particular:
- `qdrant_client::qdrant::Value` is a protobuf type — `as_str()` and `as_integer()` may not exist as convenience methods. You may need to pattern match on `v.kind` (e.g. `Value { kind: Some(Kind::StringValue(s)) }`).
- Builder patterns and import paths may differ between `qdrant-client` 1.x versions.
The logic and intent of each function is correct — the specific API surface may need adaptation.

- [ ] **Step 2: Add `mod qdrant;` to main.rs**

- [ ] **Step 3: Verify it compiles**

```bash
cargo check
```

Expected: compiles (no runtime tests — Qdrant integration tests need a running instance).

- [ ] **Step 4: Commit**

```bash
git add src/qdrant.rs src/main.rs
git commit -m "feat: add Qdrant client with collection setup, upsert, search, and status"
```

---

## Chunk 6: Indexing Pipeline

### Task 11: Pipeline orchestration

**Files:**
- Modify: `src/indexer/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement the indexing pipeline**

Replace `src/indexer/mod.rs` with:

```rust
pub mod language;
pub mod walker;
pub mod chunker;

use anyhow::{Context, Result};
use qdrant_client::qdrant::PointStruct;
use serde_json::json;
use std::path::Path;
use tracing::{info, warn};

use crate::config::Config;
use crate::embeddings::EmbeddingClient;
use crate::models::{IndexResult, make_point_id};
use crate::qdrant::QdrantClient;

/// Run the full indexing pipeline.
pub async fn run_indexing(config: &Config) -> Result<IndexResult> {
    let start = std::time::Instant::now();

    // Step 1: Validate preconditions
    let repo_path = &config.repo_path;
    anyhow::ensure!(
        repo_path.join(".git").exists(),
        "Not a git repository: {}",
        repo_path.display()
    );

    let embedding_client = EmbeddingClient::new(
        config.ollama_url.clone(),
        config.embed_concurrency,
    );
    let qdrant_client = QdrantClient::new(
        &config.qdrant_url,
        config.collection_name.clone(),
    ).await?;

    info!("Checking Ollama...");
    embedding_client.health_check().await?;

    info!("Checking Qdrant...");
    qdrant_client.health_check().await?;

    // Step 2: Ensure collection
    qdrant_client.ensure_collection().await?;

    // Step 3: Walk repo
    info!("Walking {}...", repo_path.display());
    let files = walker::walk_repo(repo_path, config.max_file_size)?;
    info!("Found {} indexable files", files.len());

    // Step 4: Chunk all files
    let mut all_chunks = Vec::new();
    let mut files_indexed = 0;

    for file in &files {
        let content = match std::fs::read_to_string(&file.path) {
            Ok(c) => c,
            Err(err) => {
                warn!("Skipping {}: {}", file.relative_path, err);
                continue;
            }
        };

        if content.is_empty() {
            continue;
        }

        let chunks = chunker::chunk_file(
            &file.relative_path,
            &content,
            file.language,
        );

        if !chunks.is_empty() {
            files_indexed += 1;
            all_chunks.extend(chunks);
        }
    }

    info!("Chunked {} files into {} chunks", files_indexed, all_chunks.len());

    // Step 5: Delete stale points for files being re-indexed
    let unique_files: std::collections::HashSet<&str> = all_chunks
        .iter()
        .map(|c| c.file_path.as_str())
        .collect();

    for file_path in &unique_files {
        qdrant_client.delete_by_file_path(file_path).await?;
    }

    // Step 6: Embed all chunks
    info!("Embedding {} chunks...", all_chunks.len());
    let embed_items: Vec<(String, String)> = all_chunks
        .iter()
        .map(|c| (c.file_path.clone(), c.content.clone()))
        .collect();

    let embeddings = embedding_client.embed_many(embed_items).await?;
    info!("Embedding complete");

    // Step 7: Upsert to Qdrant
    let repo_name = config.repo_name();
    let mut points = Vec::new();

    for (chunk, embedding) in all_chunks.iter().zip(embeddings.iter()) {
        let vector = match embedding {
            Some(v) => v.clone(),
            None => continue, // skip chunks that failed to embed
        };

        let point_id = make_point_id(&chunk.file_path, chunk.start_line);

        let payload = json!({
            "file_path": chunk.file_path,
            "language": chunk.language,
            "chunk_type": chunk.chunk_type,
            "chunk_name": chunk.chunk_name,
            "content": chunk.content,
            "start_line": chunk.start_line,
            "end_line": chunk.end_line,
            "repo": repo_name,
        });

        points.push(PointStruct::new(
            point_id,
            vector,
            payload.try_into().unwrap(),
        ));
    }

    // Batch upsert
    for batch in points.chunks(config.upsert_batch_size) {
        qdrant_client.upsert_batch(batch.to_vec()).await?;
    }

    // Write metadata
    let commit_hash = get_head_commit(repo_path).unwrap_or_else(|| "unknown".to_string());
    let timestamp = chrono::Utc::now().to_rfc3339();
    qdrant_client.upsert_metadata(&commit_hash, &timestamp).await?;

    let duration = start.elapsed();

    let result = IndexResult {
        files_indexed,
        chunks_created: points.len(),
        duration_seconds: duration.as_secs_f64(),
    };

    info!(
        "Done. {} files, {} chunks, {:.1}s",
        result.files_indexed, result.chunks_created, result.duration_seconds
    );

    Ok(result)
}

fn get_head_commit(repo_path: &Path) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
}
```

- [ ] **Step 2: Wire up the index command in main.rs**

In the `Command::Index` arm of `main.rs`, replace the TODO with:

```rust
            let result = crate::indexer::run_indexing(&config).await?;
            eprintln!(
                "Indexed {} files, {} chunks in {:.1}s",
                result.files_indexed, result.chunks_created, result.duration_seconds
            );
```

- [ ] **Step 3: Verify it compiles**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src/indexer/mod.rs src/main.rs
git commit -m "feat: add indexing pipeline orchestration"
```

---

### Task 12: Manual integration test

This is not an automated test — it verifies the full pipeline works end-to-end.

- [ ] **Step 1: Start Qdrant**

```bash
docker run -d -p 6333:6333 --name gitdex-qdrant qdrant/qdrant
```

- [ ] **Step 2: Ensure Ollama is running with the model**

```bash
ollama pull nomic-embed-text
```

- [ ] **Step 3: Build and run against a test repo**

```bash
cargo build
cargo run -- index /path/to/some/small/repo
```

Expected: output showing files walked, chunks created, embeddings generated, upserted to Qdrant.

- [ ] **Step 4: Verify points in Qdrant**

```bash
curl -s http://localhost:6333/collections/repo_index | jq '.result.points_count'
```

Expected: a number > 0.

- [ ] **Step 5: Fix any issues found, commit**

---

## Chunk 7: MCP Server

### Task 13: MCP server with three tools

**Files:**
- Create: `src/mcp.rs`
- Modify: `src/main.rs`

- [ ] **Step 0: Verify rmcp API compatibility**

Before writing the full implementation, check that `rmcp` compiles and its API matches our usage. Create a minimal test:

```bash
cargo doc -p rmcp --no-deps 2>&1 | head -20
```

Review the generated docs or `rmcp` examples on crates.io/GitHub. The code below uses `#[tool(tool_box)]`, `#[tool]`, `ServerHandler`, and `ServiceExt` — verify these exist in your version. If the API differs, adapt the code below accordingly. The `rmcp` crate is young and its API may have changed since this plan was written.

- [ ] **Step 1: Implement the MCP server**

Create `src/mcp.rs`:

```rust
use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    model::{ServerCapabilities, ServerInfo, Implementation},
    schemars, tool,
};
use std::sync::Arc;

use crate::config::Config;
use crate::embeddings::EmbeddingClient;
use crate::models::SearchResult;
use crate::qdrant::QdrantClient;

pub struct GitdexMcp {
    config: Arc<Config>,
    embedding_client: Arc<EmbeddingClient>,
    qdrant_client: Arc<QdrantClient>,
}

#[tool(tool_box)]
impl GitdexMcp {
    pub async fn new(config: Config) -> Result<Self> {
        let embedding_client = EmbeddingClient::new(
            config.ollama_url.clone(),
            config.embed_concurrency,
        );
        let qdrant_client = QdrantClient::new(
            &config.qdrant_url,
            config.collection_name.clone(),
        ).await?;

        Ok(Self {
            config: Arc::new(config),
            embedding_client: Arc::new(embedding_client),
            qdrant_client: Arc::new(qdrant_client),
        })
    }

    /// Search the indexed code repository for relevant code chunks using semantic similarity.
    /// Use this to find implementations, understand how features work, or locate specific functions and classes.
    #[tool]
    async fn search_code(
        &self,
        /// Natural language or code pattern to search for
        query: String,
        /// Filter by language (e.g. "python", "rust")
        language: Option<String>,
        /// Filter to files under a path (e.g. "src/auth/")
        file_path_prefix: Option<String>,
        /// Number of results to return (default: 10, max: 50)
        top_k: Option<u64>,
    ) -> String {
        let top_k = top_k.unwrap_or(10).min(50);

        let vector = match self.embedding_client.embed(&query).await {
            Ok(v) => v,
            Err(err) => return format!("Error embedding query: {}", err),
        };

        let results = match self.qdrant_client.search(
            vector,
            top_k,
            language.as_deref(),
            file_path_prefix.as_deref(),
        ).await {
            Ok(r) => r,
            Err(err) => return format!("Search error: {}", err),
        };

        SearchResult::format_results(&results, &query)
    }

    /// Index or re-index the configured code repository.
    /// Run this after pulling new changes to update the search index.
    #[tool]
    async fn index_repo(
        &self,
        /// Absolute path to the repository (defaults to configured repo)
        repo_path: Option<String>,
    ) -> String {
        // Validate repo_path matches config if provided
        if let Some(ref path) = repo_path {
            let config_path = self.config.repo_path.to_string_lossy().to_string();
            if path != &config_path {
                return format!(
                    "Error: This server is scoped to {}. Cannot index {}.",
                    config_path, path
                );
            }
        }

        match crate::indexer::run_indexing(&self.config).await {
            Ok(result) => format!(
                "Indexing complete. {} files indexed, {} chunks created in {:.1}s.",
                result.files_indexed, result.chunks_created, result.duration_seconds
            ),
            Err(err) => format!("Indexing failed: {}", err),
        }
    }

    /// Check the current state of the code index.
    #[tool]
    async fn get_index_status(&self) -> String {
        let repo_path = self.config.repo_path.to_string_lossy().to_string();

        match self.qdrant_client.get_status(&repo_path).await {
            Ok(status) => {
                let commit = status.last_commit.as_deref().unwrap_or("none");
                let indexed = status.last_indexed.as_deref().unwrap_or("never");
                format!(
                    "Collection: {}\nRepo: {}\nLast commit: {}\nLast indexed: {}\nTotal chunks: {}",
                    status.collection_name, status.repo_path, commit, indexed, status.total_points
                )
            }
            Err(err) => format!("Error getting status: {}", err),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for GitdexMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Gitdex provides semantic code search over an indexed repository. \
                 Use search_code to find relevant code, index_repo to update the index, \
                 and get_index_status to check index health.".to_string()
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..ServerInfo::default()
        }
    }
}

pub async fn run_mcp_server(config: Config) -> Result<()> {
    let server = GitdexMcp::new(config).await?;

    let transport = rmcp::transport::io::stdio();

    let service = server.serve(transport).await?;
    service.waiting().await?;

    Ok(())
}
```

**Important**: The `rmcp` crate API may differ from what's shown above. The exact derive macros, attribute syntax, and transport setup should be verified against the `rmcp` documentation. The logic is correct — adapt the API surface during implementation.

- [ ] **Step 2: Wire up the serve command in main.rs**

In the `Command::Serve` arm, replace the TODO with:

```rust
            crate::mcp::run_mcp_server(config).await?;
```

- [ ] **Step 3: Add `mod mcp;` to main.rs and add `schemars` to Cargo.toml**

Add to `Cargo.toml` dependencies:
```toml
schemars = "0.8"
```

- [ ] **Step 4: Verify it compiles**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add src/mcp.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: add MCP server with search_code, index_repo, get_index_status tools"
```

---

### Task 14: End-to-end MCP test

- [ ] **Step 1: Build the binary**

```bash
cargo build --release
```

- [ ] **Step 2: Test MCP server with Claude Code**

```bash
claude mcp add gitdex -- ./target/release/gitdex serve /path/to/test/repo
```

- [ ] **Step 3: Verify tools appear in Claude Code**

Start a Claude Code session and check that `search_code`, `index_repo`, and `get_index_status` are listed as available tools.

- [ ] **Step 4: Test the tools**

Ask Claude Code to:
1. Run `get_index_status` — should show empty/no index
2. Run `index_repo` — should index the repo
3. Run `search_code` with a query — should return relevant results
4. Run `get_index_status` again — should show updated metadata

- [ ] **Step 5: Fix any issues found, commit**

```bash
git add -A
git commit -m "fix: resolve issues found during end-to-end MCP testing"
```

---

### Task 15: Final cleanup and README

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write README**

Create `README.md` with:
- What gitdex does (one paragraph)
- Prerequisites (Rust, Ollama, Qdrant)
- Installation instructions (`cargo install --path .`)
- Quick start (index a repo, register with Claude Code)
- CLI reference (both subcommands with flags)
- MCP tools reference (three tools with parameters)

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add README with setup and usage instructions"
```
