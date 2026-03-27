use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "gitdex",
    about = "Local code indexer with MCP-based semantic search"
)]
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
        #[arg(
            long,
            env = "GITDEX_QDRANT_URL",
            default_value = "http://localhost:6334"
        )]
        qdrant_url: String,

        /// Ollama server URL
        #[arg(
            long,
            env = "GITDEX_OLLAMA_URL",
            default_value = "http://localhost:11434"
        )]
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
        #[arg(
            long,
            env = "GITDEX_QDRANT_URL",
            default_value = "http://localhost:6334"
        )]
        qdrant_url: String,

        /// Ollama server URL
        #[arg(
            long,
            env = "GITDEX_OLLAMA_URL",
            default_value = "http://localhost:11434"
        )]
        ollama_url: String,

        /// Qdrant collection name
        #[arg(long, env = "GITDEX_COLLECTION", default_value = "repo_index")]
        collection: String,

        /// Enable debug-level logging
        #[arg(short, long)]
        verbose: bool,
    },
}
