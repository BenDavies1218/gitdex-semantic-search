mod cli;
mod config;
mod embeddings;
mod indexer;
mod mcp;
mod models;
mod qdrant;

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

            let result = crate::indexer::run_indexing(&config).await?;
            eprintln!(
                "Indexed {} files, {} chunks in {:.1}s",
                result.files_indexed, result.chunks_created, result.duration_seconds
            );
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

            crate::mcp::run_mcp_server(config).await?;
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
