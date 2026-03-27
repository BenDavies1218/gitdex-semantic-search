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
    pub http_timeout_seconds: u64,
    pub verbose: bool,
}

impl Config {
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            repo_path,
            qdrant_url: std::env::var("GITDEX_QDRANT_URL")
                .unwrap_or_else(|_| "http://localhost:6334".to_string()),
            ollama_url: std::env::var("GITDEX_OLLAMA_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            collection_name: std::env::var("GITDEX_COLLECTION")
                .unwrap_or_else(|_| "repo_index".to_string()),
            embed_concurrency: 20,
            upsert_batch_size: 100,
            max_file_size: 1_048_576, // 1MB
            http_timeout_seconds: 30,
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
        assert_eq!(config.qdrant_url, "http://localhost:6334");
        assert_eq!(config.ollama_url, "http://localhost:11434");
        assert_eq!(config.collection_name, "repo_index");
        assert_eq!(config.embed_concurrency, 20);
        assert_eq!(config.upsert_batch_size, 100);
        assert_eq!(config.max_file_size, 1_048_576);
        assert_eq!(config.http_timeout_seconds, 30);
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
