# gitdex

Local code indexer with MCP-based semantic search. Parses a Git repository with tree-sitter, generates vector embeddings via Ollama, stores them in Qdrant, and exposes search as MCP tools for LLM-assisted development.

## Prerequisites

- **Rust** (1.75+) — `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Ollama** — `curl -fsSL https://ollama.com/install.sh | sh`
- **Qdrant** — via Docker or native install

## Installation

```bash
git clone https://github.com/your-user/gitdex.git
cd gitdex
cargo install --path .
```

## Quick Start

```bash
# 1. Pull the embedding model
ollama pull nomic-embed-text

# 2. Start Qdrant
docker run -d -p 6333:6333 --name qdrant qdrant/qdrant

# 3. Index a repository
gitdex index /path/to/your/repo

# 4. Register with Claude Code
claude mcp add gitdex -- gitdex serve /path/to/your/repo

# Done — Claude Code can now use search_code, index_repo, and get_index_status
```

## CLI Reference

### `gitdex index <repo-path>`

Index a code repository. Walks the repo, chunks code with tree-sitter, generates embeddings via Ollama, and upserts to Qdrant.

| Flag | Default | Description |
|------|---------|-------------|
| `--qdrant-url` | `http://localhost:6333` | Qdrant server URL |
| `--ollama-url` | `http://localhost:11434` | Ollama server URL |
| `--collection` | `repo_index` | Qdrant collection name |
| `-v, --verbose` | off | Debug-level logging |

### `gitdex serve <repo-path>`

Start an MCP server over stdio, scoped to a single repository.

Same flags as `index`. In serve mode, logging defaults to `warn` level (only errors/warnings to stderr, keeping stdout clean for MCP protocol).

## MCP Tools

### `search_code`

Semantic search across the indexed repository.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | Natural language or code pattern |
| `language` | string | no | Filter by language (e.g. `python`) |
| `file_path_prefix` | string | no | Filter to files under a path |
| `top_k` | integer | no | Number of results (default: 10, max: 50) |

### `index_repo`

Re-index the configured repository.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `repo_path` | string | no | Must match the configured repo path |

### `get_index_status`

Returns collection name, repo path, last commit hash, last indexed timestamp, and total chunk count.

## Supported Languages (AST Chunking)

Python, JavaScript, TypeScript, Go, Rust, Java.

All other text files are indexed using line-based chunking.

## Environment Variables

All CLI flags can also be set via environment variables:

| Variable | Flag equivalent |
|----------|----------------|
| `GITDEX_QDRANT_URL` | `--qdrant-url` |
| `GITDEX_OLLAMA_URL` | `--ollama-url` |
| `GITDEX_COLLECTION` | `--collection` |

## Architecture

Single Rust binary with two modes. The indexer pipeline: walk repo → chunk with tree-sitter → embed via Ollama → upsert to Qdrant. The MCP server exposes tools over stdio transport for LLM tool integration.

See [design spec](docs/superpowers/specs/2026-03-27-gitdex-mvp-design.md) for full details.
