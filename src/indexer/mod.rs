pub mod chunker;
pub mod language;
pub mod walker;

use anyhow::Result;
use qdrant_client::qdrant::PointStruct;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use tracing::{info, warn};

use crate::config::Config;
use crate::embeddings::EmbeddingClient;
use crate::models::{make_point_id, IndexResult};
use crate::qdrant::QdrantStore;

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

    let embedding_client = std::sync::Arc::new(EmbeddingClient::new(
        config.ollama_url.clone(),
        config.embed_concurrency,
        config.http_timeout_seconds,
    ));
    let qdrant_client =
        QdrantStore::new(&config.qdrant_url, config.collection_name.clone()).await?;

    info!("Checking Ollama...");
    embedding_client.health_check().await?;

    info!("Checking Qdrant...");
    qdrant_client.health_check().await?;

    // Step 2: Ensure collection
    qdrant_client.ensure_collection().await?;

    // Step 3: Detect incremental mode
    let current_commit = get_head_commit(repo_path);
    let last_commit = qdrant_client.get_last_commit().await?;

    let changed_files: Option<Vec<String>> = match (&last_commit, &current_commit) {
        (Some(last), Some(current)) if last != current => {
            match get_changed_files(repo_path, last, current) {
                Some(files) if !files.is_empty() => {
                    info!(
                        "Incremental mode: {} files changed since {}",
                        files.len(),
                        &last[..8.min(last.len())]
                    );
                    Some(files)
                }
                _ => {
                    info!("No changed files detected, performing full index");
                    None
                }
            }
        }
        _ => {
            info!("No previous index found, performing full index");
            None
        }
    };

    // Step 4: Walk repo
    info!("Walking {}...", repo_path.display());
    let all_files = walker::walk_repo(repo_path, config.max_file_size)?;

    // Filter to changed files if in incremental mode
    let files: Vec<_> = match &changed_files {
        Some(changed) => {
            let changed_set: HashSet<&str> = changed.iter().map(|s| s.as_str()).collect();
            all_files
                .into_iter()
                .filter(|f| changed_set.contains(f.relative_path.as_str()))
                .collect()
        }
        None => all_files,
    };
    info!("Found {} files to index", files.len());

    // Step 5: Chunk all files
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

        let chunks = chunker::chunk_file(&file.relative_path, &content, file.language);

        if !chunks.is_empty() {
            files_indexed += 1;
            all_chunks.extend(chunks);
        }
    }

    info!(
        "Chunked {} files into {} chunks",
        files_indexed,
        all_chunks.len()
    );

    // Delete points for files that were removed
    if let Some(ref changed) = changed_files {
        let indexed_paths: HashSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
        for changed_path in changed {
            if !indexed_paths.contains(changed_path.as_str()) {
                info!("Removing deleted file from index: {}", changed_path);
                qdrant_client.delete_by_file_path(changed_path).await?;
            }
        }
    }

    // Step 6: Delete stale points for files being re-indexed
    let unique_files: HashSet<&str> = all_chunks.iter().map(|c| c.file_path.as_str()).collect();

    for file_path in &unique_files {
        qdrant_client.delete_by_file_path(file_path).await?;
    }

    // Step 7: Embed all chunks
    info!("Embedding {} chunks...", all_chunks.len());
    let embed_items: Vec<(String, String)> = all_chunks
        .iter()
        .map(|c| (c.file_path.clone(), c.content.clone()))
        .collect();

    let embeddings = embedding_client.embed_many(embed_items).await?;
    info!("Embedding complete");

    // Step 8: Upsert to Qdrant
    let repo_name = config.repo_name();
    let mut points = Vec::new();

    for (chunk, embedding) in all_chunks.iter().zip(embeddings.iter()) {
        let vector = match embedding {
            Some(v) => v.clone(),
            None => continue,
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
            qdrant_client::Payload::try_from(payload).unwrap(),
        ));
    }

    // Batch upsert
    for batch in points.chunks(config.upsert_batch_size) {
        qdrant_client.upsert_batch(batch.to_vec()).await?;
    }

    // Write metadata
    let commit_hash = current_commit.unwrap_or_else(|| "unknown".to_string());
    let timestamp = chrono::Utc::now().to_rfc3339();
    qdrant_client
        .upsert_metadata(&commit_hash, &timestamp)
        .await?;

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

/// Get files changed between two commits using git diff.
fn get_changed_files(repo_path: &Path, from_commit: &str, to_commit: &str) -> Option<Vec<String>> {
    std::process::Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("{}..{}", from_commit, to_commit),
        ])
        .current_dir(repo_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .map(|l| l.to_string())
                        .collect(),
                )
            } else {
                None
            }
        })
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
