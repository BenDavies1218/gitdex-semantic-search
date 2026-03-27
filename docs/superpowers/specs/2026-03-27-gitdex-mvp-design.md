# Gitdex MVP — Design Specification

## Overview

Gitdex is a local code indexing tool that allows LLMs to semantically search a code repository. It parses a local Git repository, chunks code into meaningful units using tree-sitter, generates vector embeddings via Ollama, stores them in Qdrant, and exposes search as an MCP (Model Context Protocol) tool.

The primary consumer is LLM-assisted development tools — Claude Code, Cursor, and similar. The MCP stdio transport means these tools can spawn gitdex directly and discover its tools automatically.

**MVP scope**: one repo, one branch, manual re-indexing, three MCP tools (search, index, status).

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                gitdex (single Rust binary)       │
│                                                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Indexer   │  │ MCP      │  │ Embeddings    │  │
│  │ Pipeline  │  │ Server   │  │ Client        │  │
│  │          │  │ (stdio)  │  │ (Ollama HTTP) │  │
│  └────┬─────┘  └────┬─────┘  └───────┬───────┘  │
│       │              │                │          │
│       └──────┬───────┘                │          │
│              │                        │          │
│         ┌────▼────┐                   │          │
│         │ Qdrant  │◀──────────────────┘          │
│         │ Client  │                              │
│         └────┬────┘                              │
└──────────────┼───────────────────────────────────┘
               │
               ▼
        ┌──────────┐         ┌──────────┐
        │ Qdrant   │         │ Ollama   │
        │ (external)│         │ (host)   │
        └──────────┘         └──────────┘
```

**Components**:
- **Single Rust binary** with two modes: `gitdex index` and `gitdex serve`
- **Qdrant** — external vector database, user-managed (Docker, native, or cloud)
- **Ollama** — external embedding service running on the host

---

## Tech Stack

| Component | Technology | Why |
|-----------|-----------|-----|
| Language | Rust | Single compiled binary, no runtime dependencies, excellent tree-sitter support |
| MCP SDK | `rmcp` | Most active Rust MCP implementation |
| Vector DB | Qdrant (`qdrant-client` crate) | Official Rust client, hybrid search, payload filtering |
| Code Parser | `tree-sitter` + individual grammar crates | AST-level chunking, upstream-recommended approach |
| Embeddings | Ollama + `nomic-embed-text` | Local, free, no API key, good for code |
| HTTP Client | `reqwest` | Mature async HTTP for Ollama API calls |
| Async Runtime | `tokio` | Standard Rust async runtime |
| CLI | `clap` | Argument parsing with derive macros |
| Serialisation | `serde` / `serde_json` | JSON handling throughout |
| Error Handling | `anyhow` | Ergonomic error chains with context |
| Logging | `tracing` + `tracing-subscriber` | Structured logging to stderr |
| File Walking | `ignore` | Respects .gitignore, same library as ripgrep |

---

## Project Structure

```
gitdex/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point (clap), dispatches to index/serve
│   ├── cli.rs               # Clap command definitions
│   ├── indexer/
│   │   ├── mod.rs            # Orchestrates the indexing pipeline
│   │   ├── walker.rs         # Recursive file walking with skip-lists
│   │   ├── chunker.rs        # tree-sitter parsing + fallback line chunking
│   │   └── language.rs       # Language detection + grammar mapping
│   ├── embeddings.rs         # Ollama HTTP client (embed text → Vec<f32>)
│   ├── qdrant.rs             # Qdrant operations (create collection, upsert, search)
│   ├── mcp.rs                # MCP server setup, tool definitions, handlers
│   ├── config.rs             # Config struct loaded from env vars / CLI flags
│   └── models.rs             # Shared types: Chunk, SearchResult, IndexStatus
```

---

## CLI Interface

```
gitdex index <repo-path>          # Run indexing pipeline, exit on completion
    --qdrant-url <url>            # Default: http://localhost:6333
    --ollama-url <url>            # Default: http://localhost:11434
    --collection <name>           # Default: repo_index
    --verbose / -v                # Debug-level logging

gitdex serve <repo-path>          # Start MCP server over stdio, scoped to this repo
    --qdrant-url <url>
    --ollama-url <url>
    --collection <name>
    --verbose / -v                # Debug-level logging (warn-only by default in serve)
```

The `serve` command requires a `repo-path` argument. This scopes the MCP server to a single repository — `index_repo` uses this path as its default, and the server validates that any explicit `repo_path` passed to `index_repo` matches.

Config resolution: CLI flags > environment variables (`GITDEX_*`) > defaults.

---

## File Walking & Language Detection

### Walker (`walker.rs`)

Uses the `ignore` crate (same as ripgrep) to recursively walk the repo. This automatically respects `.gitignore` rules.

**Additional skip directories**: `.git`, `node_modules`, `vendor`, `__pycache__`, `.venv`, `dist`, `build`, `.next`, `target`, `.cargo`

**Skip files**:
- Binary extensions: `.png`, `.jpg`, `.wasm`, `.exe`, `.so`, `.dylib`, etc.
- Lock files: `package-lock.json`, `poetry.lock`, `Cargo.lock`, `yarn.lock`, `pnpm-lock.yaml`
- Minified: `*.min.js`, `*.min.css`
- Files over 1MB

### Language Detection (`language.rs`)

Maps file extension to a `Language` enum:

| Extension | Language | Tree-sitter grammar crate |
|-----------|----------|--------------------------|
| `.py` | Python | `tree-sitter-python` |
| `.js`, `.jsx` | JavaScript | `tree-sitter-javascript` |
| `.ts`, `.tsx` | TypeScript | `tree-sitter-typescript` |
| `.go` | Go | `tree-sitter-go` |
| `.rs` | Rust | `tree-sitter-rust` |
| `.java` | Java | `tree-sitter-java` |

All other text files (`.md`, `.yaml`, `.toml`, `.json`, `.sql`, `.sh`, `.c`, `.cpp`, `.rb`, etc.) use line-based fallback chunking.

---

## Chunking Strategy

### AST Chunking (`chunker.rs`)

For supported languages, parse with tree-sitter and extract meaningful code units.

**Nodes extracted per language**:
- **Python**: `function_definition`, `class_definition`, `decorated_definition`
- **JavaScript/TypeScript**: `function_declaration`, `class_declaration`, `arrow_function` (when assigned to variable/export), `method_definition`
- **Go**: `function_declaration`, `method_declaration`, `type_declaration`
- **Rust**: `function_item`, `impl_item`, `struct_item`, `enum_item`, `trait_item`
- **Java**: `method_declaration`, `class_declaration`, `interface_declaration`

**Rules**:
1. Walk AST, extract top-level and second-level nodes (e.g. methods inside a class)
2. Each node becomes one chunk with its full source text
3. Nodes exceeding 200 lines split into sub-chunks of ~100 lines with 20-line overlap
4. Code between extracted nodes (imports, module-level statements, constants) grouped into a "module" chunk
5. Tree-sitter parse failure falls back to line-based chunking

### Line-Based Fallback

- Chunks of 100 lines with 20-line overlap
- `chunk_type`: `"block"`
- `chunk_name`: `None`

### Chunk Data Model

```rust
pub struct Chunk {
    pub file_path: String,
    pub language: String,
    pub chunk_type: String,      // "function", "class", "method", "impl", "block", etc.
    pub chunk_name: Option<String>,
    pub content: String,
    pub start_line: u32,
    pub end_line: u32,
}
```

### Point ID Generation

Deterministic hash from `file_path` + `start_line`. IDs are ephemeral — they change when code moves within a file. This is fine because the pipeline deletes all points for a file before re-upserting, so correctness is maintained on every re-index.

```rust
fn make_point_id(file_path: &str, start_line: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    format!("{}::{}", file_path, start_line).hash(&mut hasher);
    let id = hasher.finish();
    // Reserve ID 0 for the metadata point
    if id == 0 { 1 } else { id }
}
```

---

## Embeddings Client

### Ollama Client (`embeddings.rs`)

```rust
pub struct EmbeddingClient {
    http: reqwest::Client,
    ollama_url: String,
    model: String,          // "nomic-embed-text"
    semaphore: Semaphore,   // bounds concurrency to 20
}

impl EmbeddingClient {
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>>;
    pub async fn embed_many(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;
}
```

**Input formatting**: Prepend file path to content before embedding:
```
src/auth/handler.py
def validate_token(token: str) -> bool:
    ...
```

**Concurrency**: 20 concurrent requests via `tokio::sync::Semaphore`. Ollama doesn't support batch embedding, so parallelism is the only way to get throughput.

**Retries**: Up to 3 retries with exponential backoff on transient failures (connection refused, 5xx). Fail hard if unreachable after retries.

**Startup check**: Single test embedding call before indexing begins. Exit immediately with a clear message if Ollama is unreachable.

---

## Qdrant Operations

### Qdrant Client (`qdrant.rs`)

**Collection setup**:
- Create if not exists: 768-dim vectors (hardcoded for `nomic-embed-text`), cosine distance
- If collection already exists, verify vector dimension matches 768; fail with clear error if mismatched
- Create payload indexes: `language` (keyword), `file_path` (text)
- No migration logic for MVP

**Upsert**:
- Batch upsert in groups of 100 points
- Each point: `u64` ID, vector, payload
- Payload fields: `file_path`, `language`, `chunk_type`, `chunk_name`, `content`, `start_line`, `end_line`, `repo`
- `repo` is derived from the basename of the configured repo path (e.g., `/home/user/my-project` → `"my-project"`), added during the upsert step (not part of the `Chunk` struct)

**Stale point cleanup**: Before upserting, delete all existing points for files being re-indexed. Handles cases where refactored files produce fewer chunks.

```rust
pub async fn delete_by_file_path(&self, file_path: &str) -> Result<()>;
```

**Search**:
```rust
pub struct SearchRequest {
    pub vector: Vec<f32>,
    pub top_k: u64,
    pub language: Option<String>,
    pub file_path_prefix: Option<String>,
}

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
```

**Filtering**: Qdrant payload filters applied server-side:
- `language`: exact match on keyword-indexed `language` field
- `file_path_prefix`: use a `text` index type on `file_path` with `MatchText` filter for substring matching (Qdrant keyword indexes only support exact match, not prefix/substring)

**Payload indexes to create on collection setup**:
- `language`: keyword index
- `file_path`: text index (enables substring matching for path prefix filters)

**Metadata storage**: Last indexed commit hash and timestamp stored as a Qdrant point (ID `0`) rather than a separate state file.

---

## MCP Server

### Setup (`mcp.rs`)

Uses `rmcp` crate, stdio transport. Registered with:
```bash
claude mcp add gitdex -- gitdex serve /path/to/my-project
```

All logging to stderr. Stdout reserved for MCP JSON-RPC protocol.

### Tools

#### `search_code`

Semantic code search across the indexed repository.

**Parameters**:
- `query` (string, required): Natural language or code pattern
- `language` (string, optional): Filter by language
- `file_path_prefix` (string, optional): Filter to files under a path
- `top_k` (integer, optional): Number of results, default 10, max 50

**Flow**: Embed query via Ollama → search Qdrant with filters → return formatted text.

**Response format** (structured text, not JSON):
```
Found 5 results for "authentication logic":

── src/auth/handler.py:42-67 (function: validate_token) [score: 0.87] ──
def validate_token(token: str) -> bool:
    ...

── src/auth/middleware.py:10-35 (function: require_auth) [score: 0.82] ──
def require_auth(request):
    ...
```

#### `index_repo`

Full re-index of the configured repository.

**Parameters**:
- `repo_path` (string, optional): Absolute path to the repository. Defaults to the repo path the server was started with. If provided, must match the configured repo path (the server is scoped to one repo for MVP).

**Returns**: Summary with files indexed, chunks created, duration.

**Concurrency note**: `index_repo` is a blocking call. While indexing is in progress, `search_code` calls will still work against the existing index, but results may be stale until indexing completes. The pipeline deletes stale points per-file then upserts, so there is a brief window where a file's chunks are missing.

#### `get_index_status`

Current state of the index for the configured collection.

**Parameters**: None.

**Returns**: Last commit hash, timestamp, total points, collection name, repo path.

---

## Indexing Pipeline

### Orchestration (`indexer/mod.rs`)

Sequential steps, parallel within each step:

1. **Validate preconditions** — check repo path has `.git`, ping Ollama and Qdrant, fail fast with clear errors
2. **Ensure collection** — create Qdrant collection if it doesn't exist (768-dim, cosine); if it exists, verify dimension matches; create payload indexes
3. **Walk repo** — use `ignore` crate, apply skip-lists, collect `(file_path, language)` tuples
4. **Chunk all files** — parse with tree-sitter or fall back to line chunking, collect `Chunk` structs
5. **Delete stale points** — for each file being re-indexed, delete its existing points from Qdrant
6. **Embed all chunks** — `embed_many` with bounded concurrency (20), pair each chunk with its vector
7. **Upsert to Qdrant** — batch upsert in groups of 100, write metadata point (ID `0`) with commit hash and timestamp
8. **Report results** — return `IndexResult { files_indexed, chunks_created, duration }`

**Not incremental for MVP** — re-indexes everything every run. Stale point deletion per-file ensures correctness on re-runs.

---

## Error Handling & Logging

### Error Strategy

`anyhow` for ergonomic error chains with context:
```rust
let contents = fs::read_to_string(&path)
    .with_context(|| format!("Failed to read {}", path.display()))?;
```

### Failure Modes

| Failure | Behaviour |
|---------|-----------|
| Ollama unreachable | Fail fast at startup, suggest `ollama serve` |
| Qdrant unreachable | Fail fast at startup, suggest checking Qdrant URL |
| Single file unreadable | Skip file, warn to stderr, continue |
| Tree-sitter parse failure | Fall back to line chunking, warn |
| Single embedding request fails after retries | Skip chunk, warn, continue |
| Qdrant upsert batch fails | Retry once, then fail pipeline |
| Collection missing or empty on search | Return clear message: "No index found. Run index_repo first." |
| Embedding dimension mismatch | Detected at startup check; if collection exists with different dimensions, fail with clear error explaining the model/collection mismatch |

### Logging

`tracing` + `tracing-subscriber`, structured output to stderr.

- Default: `info` (progress summaries)
- `--verbose` / `-v`: `debug` (per-file details)
- MCP `serve` mode: `warn` only by default

---

## Configuration

```rust
pub struct Config {
    pub repo_path: PathBuf,       // required (CLI argument)
    pub qdrant_url: String,       // default: http://localhost:6333
    pub ollama_url: String,       // default: http://localhost:11434
    pub collection_name: String,  // default: repo_index
    pub embed_concurrency: usize, // default: 20
    pub upsert_batch_size: usize, // default: 100
    pub max_file_size: u64,       // default: 1MB
}
```

**Note**: The embedding model is hardcoded to `nomic-embed-text` for MVP. The collection is always created with 768-dim vectors matching this model. Making the model configurable requires dynamically detecting the embedding dimension, which is post-MVP scope.

Resolution: CLI flags > env vars (`GITDEX_*`) > defaults. No config file for MVP.

---

## Qdrant Data Model

### Collection: `repo_index`

**Vector config**: size 768, cosine distance.

**Point structure**:
```json
{
  "id": 12345678901234,
  "vector": [0.1, 0.2, ...],
  "payload": {
    "file_path": "src/auth/handler.py",
    "language": "python",
    "chunk_type": "function",
    "chunk_name": "validate_token",
    "content": "def validate_token(token: str) -> bool:\n    ...",
    "start_line": 42,
    "end_line": 67,
    "repo": "my-project"
  }
}
```

**Metadata point** (ID `0`):
```json
{
  "id": 0,
  "vector": [0.0, 0.0, ...],
  "payload": {
    "type": "metadata",
    "last_commit": "abc123f",
    "last_indexed": "2026-03-27T10:00:00Z"
  }
}
```

---

## Distribution (MVP)

Build from source:
```bash
cargo install --path .
```

No release binaries, homebrew, or packaging for MVP.

---

## Usage Flow

```bash
# Prerequisites
ollama pull nomic-embed-text
docker run -d -p 6333:6333 qdrant/qdrant

# Install
cd gitdex && cargo install --path .

# Index a repo
gitdex index /path/to/my-project

# Register with Claude Code
claude mcp add gitdex -- gitdex serve /path/to/my-project

# Done — Claude Code can now use search_code, index_repo, get_index_status

# Re-index after code changes
gitdex index /path/to/my-project
```

---

## Post-MVP Roadmap (Not In Scope)

1. Incremental indexing via `git diff`
2. Embedded ONNX embeddings (no Ollama dependency)
3. Branch-aware collections
4. File watcher for auto re-indexing
5. Hybrid search (BM25 + semantic)
6. Multiple repo support
7. Release binaries and homebrew tap
8. Chunk context enrichment (imports, docstrings, class hierarchy)
