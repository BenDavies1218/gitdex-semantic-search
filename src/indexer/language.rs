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
    "md", "yaml", "yml", "toml", "json", "sql", "sh", "bash", "c", "cpp", "cc", "h", "hpp", "cs",
    "rb", "php", "swift", "kt", "scala", "r", "lua", "zig", "nim", "ex", "exs", "html", "css",
    "scss", "xml", "graphql", "proto", "tf",
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
    use std::path::Path;

    #[test]
    fn test_python_detection() {
        assert_eq!(
            Language::from_extension(Path::new("main.py")),
            Language::Python
        );
    }

    #[test]
    fn test_javascript_detection() {
        assert_eq!(
            Language::from_extension(Path::new("app.js")),
            Language::JavaScript
        );
        assert_eq!(
            Language::from_extension(Path::new("App.jsx")),
            Language::JavaScript
        );
    }

    #[test]
    fn test_typescript_detection() {
        assert_eq!(
            Language::from_extension(Path::new("app.ts")),
            Language::TypeScript
        );
        assert_eq!(
            Language::from_extension(Path::new("App.tsx")),
            Language::TypeScript
        );
    }

    #[test]
    fn test_go_detection() {
        assert_eq!(Language::from_extension(Path::new("main.go")), Language::Go);
    }

    #[test]
    fn test_rust_detection() {
        assert_eq!(
            Language::from_extension(Path::new("lib.rs")),
            Language::Rust
        );
    }

    #[test]
    fn test_java_detection() {
        assert_eq!(
            Language::from_extension(Path::new("Main.java")),
            Language::Java
        );
    }

    #[test]
    fn test_other_known_extension() {
        assert_eq!(
            Language::from_extension(Path::new("config.yaml")),
            Language::Other
        );
        assert!(is_indexable(Path::new("config.yaml")));
    }

    #[test]
    fn test_unknown_extension() {
        assert_eq!(
            Language::from_extension(Path::new("image.png")),
            Language::Other
        );
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
