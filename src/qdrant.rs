use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    value::Kind, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder,
    DeletePointsBuilder, Distance, FieldType, Filter, GetPointsBuilder, PointStruct,
    SearchPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use serde_json::json;
use std::collections::HashMap;
use tracing::info;

use crate::config::EMBEDDING_DIMENSIONS;
use crate::models::{IndexStatus, SearchResult};

const METADATA_POINT_ID: u64 = 0;

pub struct QdrantStore {
    client: Qdrant,
    collection_name: String,
}

impl QdrantStore {
    pub async fn new(url: &str, collection_name: String) -> Result<Self> {
        let client = Qdrant::from_url(url)
            .build()
            .context(format!("Failed to connect to Qdrant at {}", url))?;

        Ok(Self {
            client,
            collection_name,
        })
    }

    pub async fn health_check(&self) -> Result<()> {
        self.client
            .health_check()
            .await
            .context("Qdrant health check failed. Is Qdrant running?")?;
        Ok(())
    }

    pub async fn ensure_collection(&self) -> Result<()> {
        let exists = self.client
            .collection_exists(&self.collection_name)
            .await
            .context("Failed to check collection existence")?;

        if exists {
            // Validate that existing collection has correct dimensions
            let info = self.client
                .collection_info(&self.collection_name)
                .await
                .context("Failed to get collection info")?;

            if let Some(result) = &info.result {
                if let Some(config) = &result.config {
                    if let Some(params) = &config.params {
                        if let Some(qdrant_client::qdrant::vectors_config::Config::Params(
                            vector_params,
                        )) = params.vectors_config.as_ref().and_then(|vc| vc.config.as_ref())
                        {
                            if vector_params.size != EMBEDDING_DIMENSIONS {
                                anyhow::bail!(
                                    "Collection '{}' has vector size {} but configured embedding \
                                     dimensions are {}. Delete the collection and re-index, or \
                                     use a different collection name.",
                                    self.collection_name,
                                    vector_params.size,
                                    EMBEDDING_DIMENSIONS
                                );
                            }
                        }
                    }
                }
            }

            info!("Using existing collection '{}'", self.collection_name);
        } else {
            self.client
                .create_collection(
                    CreateCollectionBuilder::new(&self.collection_name)
                        .vectors_config(
                            VectorParamsBuilder::new(EMBEDDING_DIMENSIONS, Distance::Cosine),
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
                    ),
                )
                .await
                .context("Failed to create language index")?;

            self.client
                .create_field_index(
                    CreateFieldIndexCollectionBuilder::new(
                        &self.collection_name,
                        "file_path",
                        FieldType::Text,
                    ),
                )
                .await
                .context("Failed to create file_path index")?;

            info!("Created collection '{}' with indexes", self.collection_name);
        }

        Ok(())
    }

    pub async fn delete_by_file_path(&self, file_path: &str) -> Result<()> {
        let filter = Filter::must([qdrant_client::qdrant::Condition::matches(
            "file_path",
            file_path.to_string(),
        )]);

        self.client
            .delete_points(
                DeletePointsBuilder::new(&self.collection_name).points(filter),
            )
            .await
            .context(format!("Failed to delete points for {}", file_path))?;

        Ok(())
    }

    /// Upsert a batch of points. Retries once on failure.
    pub async fn upsert_batch(&self, points: Vec<PointStruct>) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }

        let result = self
            .client
            .upsert_points(UpsertPointsBuilder::new(
                &self.collection_name,
                points.clone(),
            ))
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(first_err) => {
                tracing::warn!("Upsert failed, retrying once: {}", first_err);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                self.client
                    .upsert_points(UpsertPointsBuilder::new(&self.collection_name, points))
                    .await
                    .context("Upsert failed after retry")?;

                Ok(())
            }
        }
    }

    pub async fn upsert_metadata(&self, last_commit: &str, last_indexed: &str) -> Result<()> {
        let point = PointStruct::new(
            METADATA_POINT_ID,
            vec![0.0f32; EMBEDDING_DIMENSIONS as usize],
            qdrant_client::Payload::try_from(json!({
                "type": "metadata",
                "last_commit": last_commit,
                "last_indexed": last_indexed,
            }))
            .unwrap(),
        );

        self.upsert_batch(vec![point]).await
    }

    pub async fn search(
        &self,
        vector: Vec<f32>,
        top_k: u64,
        language: Option<&str>,
        file_path_contains: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let exists = self
            .client
            .collection_exists(&self.collection_name)
            .await
            .context("Failed to check collection")?;

        if !exists {
            anyhow::bail!("No index found. Run index_repo first.");
        }

        let mut must_conditions = Vec::new();
        let must_not_conditions = vec![qdrant_client::qdrant::Condition::matches(
            "type",
            "metadata".to_string(),
        )];

        if let Some(lang) = language {
            must_conditions.push(qdrant_client::qdrant::Condition::matches(
                "language",
                lang.to_string(),
            ));
        }

        if let Some(text) = file_path_contains {
            must_conditions.push(qdrant_client::qdrant::Condition::matches_text(
                "file_path",
                text,
            ));
        }

        let filter = Filter {
            must: must_conditions,
            must_not: must_not_conditions,
            ..Default::default()
        };

        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&self.collection_name, vector, top_k)
                    .filter(filter)
                    .with_payload(true),
            )
            .await
            .context("Qdrant search failed")?;

        let search_results = results
            .result
            .into_iter()
            .map(|point| {
                let payload = point.payload;
                SearchResult {
                    file_path: get_string(&payload, "file_path"),
                    chunk_name: get_string_opt(&payload, "chunk_name"),
                    chunk_type: get_string(&payload, "chunk_type"),
                    language: get_string(&payload, "language"),
                    content: get_string(&payload, "content"),
                    start_line: get_integer(&payload, "start_line") as u32,
                    end_line: get_integer(&payload, "end_line") as u32,
                    score: point.score,
                }
            })
            .collect();

        Ok(search_results)
    }

    /// Retrieve the last indexed commit hash from the metadata point.
    pub async fn get_last_commit(&self) -> Result<Option<String>> {
        let exists = self
            .client
            .collection_exists(&self.collection_name)
            .await?;

        if !exists {
            return Ok(None);
        }

        let metadata = self
            .client
            .get_points(
                GetPointsBuilder::new(&self.collection_name, &[METADATA_POINT_ID.into()])
                    .with_payload(true),
            )
            .await
            .ok()
            .and_then(|r| r.result.into_iter().next());

        Ok(metadata.and_then(|point| get_string_opt(&point.payload, "last_commit")))
    }

    pub async fn get_status(&self, repo_path: &str) -> Result<IndexStatus> {
        let exists = self
            .client
            .collection_exists(&self.collection_name)
            .await?;

        if !exists {
            return Ok(IndexStatus {
                last_commit: None,
                last_indexed: None,
                total_points: 0,
                collection_name: self.collection_name.clone(),
                repo_path: repo_path.to_string(),
            });
        }

        let info = self
            .client
            .collection_info(&self.collection_name)
            .await?;

        let total_points = info.result.map(|r| r.points_count.unwrap_or(0)).unwrap_or(0);

        // Try to retrieve the metadata point
        let metadata = self
            .client
            .get_points(
                GetPointsBuilder::new(&self.collection_name, &[METADATA_POINT_ID.into()])
                    .with_payload(true),
            )
            .await
            .ok()
            .and_then(|r| r.result.into_iter().next());

        let (last_commit, last_indexed) = match metadata {
            Some(point) => (
                get_string_opt(&point.payload, "last_commit"),
                get_string_opt(&point.payload, "last_indexed"),
            ),
            None => (None, None),
        };

        Ok(IndexStatus {
            last_commit,
            last_indexed,
            total_points: total_points.saturating_sub(1), // exclude metadata point
            collection_name: self.collection_name.clone(),
            repo_path: repo_path.to_string(),
        })
    }
}

fn get_string(
    payload: &HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> String {
    payload
        .get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn get_string_opt(
    payload: &HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::StringValue(s)) => Some(s.clone()),
            _ => None,
        })
}

fn get_integer(
    payload: &HashMap<String, qdrant_client::qdrant::Value>,
    key: &str,
) -> i64 {
    payload
        .get(key)
        .and_then(|v| match &v.kind {
            Some(Kind::IntegerValue(i)) => Some(*i),
            _ => None,
        })
        .unwrap_or(0)
}
