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
];

/// Walk a repository and return all indexable files.
pub fn walk_repo(repo_path: &Path, max_file_size: u64) -> Result<Vec<WalkedFile>> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("Repository path not found: {}", repo_path.display()))?;

    let mut files = Vec::new();

    let walker = WalkBuilder::new(&repo_path)
        .hidden(true)
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
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        if entry.file_type().is_none() {
            continue;
        }

        let path = entry.path();

        // Skip if in a blocked directory
        if path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|s| SKIP_DIRS.contains(&s))
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
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("src/lib.py"), "def foo(): pass").unwrap();
        fs::write(dir.join("README.md"), "# Hello").unwrap();
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
