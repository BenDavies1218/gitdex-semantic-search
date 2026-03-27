use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::warn;

use crate::config::EMBEDDING_MODEL;

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
    pub fn new(ollama_url: String, concurrency: usize, timeout_seconds: u64) -> Self {
        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_seconds))
            .build()
            .expect("Failed to build HTTP client");
        Self {
            http,
            ollama_url,
            semaphore: Arc::new(Semaphore::new(concurrency)),
        }
    }

    /// Test that Ollama is reachable and the model is available.
    pub async fn health_check(&self) -> Result<()> {
        self.embed("health check")
            .await
            .context(format!(
                "Cannot reach Ollama at {}. Is it running? Try: ollama serve",
                self.ollama_url
            ))?;
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

        for attempt in 0..3u32 {
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
    /// Each item is (file_path, content) — file_path is prepended for context.
    pub async fn embed_many(
        self: &Arc<Self>,
        items: Vec<(String, String)>,
    ) -> Result<Vec<Option<Vec<f32>>>> {
        let mut handles = Vec::with_capacity(items.len());

        for (file_path, content) in items {
            let client = Arc::clone(self);

            let handle = tokio::spawn(async move {
                let _permit = client.semaphore.acquire().await.unwrap();
                let text = format!("{}\n{}", file_path, content);
                match client.embed(&text).await {
                    Ok(embedding) => Some(embedding),
                    Err(err) => {
                        warn!(
                            "Failed to embed chunk from {}: {}",
                            file_path, err
                        );
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
}
