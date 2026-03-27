use anyhow::Result;
use rmcp::{
    ServerHandler, ServiceExt,
    model::{ServerCapabilities, ServerInfo},
    schemars, tool,
};
use std::sync::Arc;

use crate::config::Config;
use crate::embeddings::EmbeddingClient;
use crate::models::SearchResult;
use crate::qdrant::QdrantStore;

#[derive(Clone)]
pub struct GitdexMcp {
    config: Arc<Config>,
    embedding_client: Arc<EmbeddingClient>,
    qdrant_store: Arc<QdrantStore>,
}

impl GitdexMcp {
    pub async fn new(config: Config) -> Result<Self> {
        let embedding_client = EmbeddingClient::new(
            config.ollama_url.clone(),
            config.embed_concurrency,
        );
        let qdrant_store =
            QdrantStore::new(&config.qdrant_url, config.collection_name.clone()).await?;

        Ok(Self {
            config: Arc::new(config),
            embedding_client: Arc::new(embedding_client),
            qdrant_store: Arc::new(qdrant_store),
        })
    }
}

#[tool(tool_box)]
impl GitdexMcp {
    /// Search the indexed code repository for relevant code chunks using semantic similarity.
    /// Use this to find implementations, understand how features work, or locate specific functions and classes.
    #[tool(description = "Search the indexed code repository for relevant code chunks using semantic similarity")]
    pub async fn search_code(
        &self,
        #[tool(param)]
        #[schemars(description = "Natural language or code pattern to search for")]
        query: String,
        #[tool(param)]
        #[schemars(description = "Filter by language (e.g. 'python', 'rust')")]
        language: Option<String>,
        #[tool(param)]
        #[schemars(description = "Filter to files under a path (e.g. 'src/auth/')")]
        file_path_prefix: Option<String>,
        #[tool(param)]
        #[schemars(description = "Number of results to return (default: 10, max: 50)")]
        top_k: Option<u64>,
    ) -> String {
        let top_k = top_k.unwrap_or(10).min(50);

        let vector = match self.embedding_client.embed(&query).await {
            Ok(v) => v,
            Err(err) => return format!("Error embedding query: {}", err),
        };

        let results = match self
            .qdrant_store
            .search(
                vector,
                top_k,
                language.as_deref(),
                file_path_prefix.as_deref(),
            )
            .await
        {
            Ok(r) => r,
            Err(err) => return format!("Search error: {}", err),
        };

        SearchResult::format_results(&results, &query)
    }

    /// Index or re-index the configured code repository.
    /// Run this after pulling new changes to update the search index.
    #[tool(description = "Index or re-index the configured code repository")]
    pub async fn index_repo(
        &self,
        #[tool(param)]
        #[schemars(description = "Absolute path to the repository (defaults to configured repo)")]
        repo_path: Option<String>,
    ) -> String {
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
    #[tool(description = "Check the current state of the code index")]
    pub async fn get_index_status(&self) -> String {
        let repo_path = self.config.repo_path.to_string_lossy().to_string();

        match self.qdrant_store.get_status(&repo_path).await {
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
                 and get_index_status to check index health."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
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
