# WIP (Work in Progress)

# gitdex

[![CI](https://github.com/BenDavies1218/gitdex-semantic-search/actions/workflows/ci.yml/badge.svg)](https://github.com/BenDavies1218/gitdex-semantic-search/actions/workflows/ci.yml)

**Local semantic code search for LLM-assisted development.**

Gitdex indexes your Git repositories using tree-sitter AST parsing and vector embeddings, then exposes semantic search via [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) tools. Point Claude Code, Cursor, or any MCP-compatible tool at your codebase and search it with natural language.

```
gitdex index ./my-project        # Index the repo
claude mcp add gitdex -- \       # Register with Claude Code
  gitdex serve ./my-project
```

## How It Works

```
  Repository          gitdex              Services
  ──────────    ──────────────────    ──────────────────

  .git/repo  ──>  Walk files          Ollama
                  (gitignore-aware)   (nomic-embed-text)
                       │                    ^
                       v                    │
                  Chunk with           Embed chunks ──>  Qdrant
                  tree-sitter                           (vector DB)
                       │                                    ^
                       v                                    │
                  MCP Server  ──────── search ──────────────┘
                  (stdio)
```

1. **Walk** — discovers files respecting `.gitignore`, skips binaries and lock files
2. **Chunk** — parses supported languages into functions, classes, and methods via tree-sitter; falls back to line-based chunking for other text files
3. **Embed** — generates 768-dimensional vectors via Ollama (`nomic-embed-text`) with bounded concurrency
4. **Store** — upserts vectors to Qdrant with payload indexes for fast filtered search
5. **Search** — MCP server embeds your query and performs cosine similarity search over the index

Subsequent runs use **incremental indexing** — only files changed since the last indexed commit are re-processed.

## Prerequisites

- [Rust](https://rustup.rs/) 1.75+
- [Ollama](https://ollama.com/) with the `nomic-embed-text` model
- [Qdrant](https://qdrant.tech/) (via Docker or native install)

## Installation

```bash
git clone https://github.com/BenDavies1218/gitdex-semantic-search.git
cd gitdex-semantic-search
cargo install --path .
```

## Quick Start

```bash
# 1. Pull the embedding model
ollama pull nomic-embed-text

# 2. Start Qdrant
docker compose up -d

# 3. Index a repository
gitdex index /path/to/your/repo

# 4. Register with Claude Code
claude mcp add gitdex -- gitdex serve /path/to/your/repo
```

That's it — Claude Code can now use `search_code`, `index_repo`, and `get_index_status`.

## MCP Tools

### `search_code`

Semantic search across the indexed repository.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | yes | Natural language or code pattern |
| `language` | string | no | Filter by language (e.g. `python`, `rust`) |
| `file_path_contains` | string | no | Filter to files containing this path segment |
| `top_k` | integer | no | Number of results (default: 10, max: 50) |

### `index_repo`

Re-index the configured repository. Uses incremental indexing when possible.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `repo_path` | string | no | Must match the configured repo path |

### `get_index_status`

Returns collection name, repo path, last commit hash, last indexed timestamp, and total chunk count.

## CLI Reference

### `gitdex index <repo-path>`

Run the full indexing pipeline.

| Flag | Default | Description |
|------|---------|-------------|
| `--qdrant-url` | `http://localhost:6334` | Qdrant gRPC endpoint |
| `--ollama-url` | `http://localhost:11434` | Ollama server URL |
| `--collection` | `repo_index` | Qdrant collection name |
| `-v, --verbose` | off | Debug-level logging |

### `gitdex serve <repo-path>`

Start an MCP server over stdio, scoped to a single repository. Same flags as `index`. Logging defaults to `warn` level to keep stdout clean for the MCP protocol.

## Supported Languages

| Language | Chunking | Extracted Units |
|----------|----------|-----------------|
| Python | AST (tree-sitter) | Functions, classes, decorated definitions |
| JavaScript | AST (tree-sitter) | Functions, classes, methods, arrow functions |
| TypeScript | AST (tree-sitter) | Functions, classes, methods, arrow functions |
| Go | AST (tree-sitter) | Functions, methods, type declarations |
| Rust | AST (tree-sitter) | Functions, impls, structs, enums, traits |
| Java | AST (tree-sitter) | Methods, classes, interfaces |
| All other text | Line-based | 100-line chunks with 20-line overlap |

## Environment Variables

All CLI flags can also be set via environment variables:

| Variable | Flag equivalent |
|----------|----------------|
| `GITDEX_QDRANT_URL` | `--qdrant-url` |
| `GITDEX_OLLAMA_URL` | `--ollama-url` |
| `GITDEX_COLLECTION` | `--collection` |

## Architecture

Single Rust binary, two modes:

- **`index`** — pipeline: walk repo, chunk with tree-sitter, embed via Ollama, upsert to Qdrant, store metadata
- **`serve`** — MCP server over stdio: embed query, search Qdrant, return formatted results

All embedding requests use bounded concurrency (20 parallel requests) with retry and exponential backoff. The Qdrant client connects via gRPC on port 6334.

## Roadmap

- [ ] **ONNX runtime embeddings** — remove Ollama dependency with local ONNX inference for faster, self-contained embedding
- [ ] **Multi-repo support** — index and search across multiple repositories from a single MCP server
- [ ] **File watcher** — automatically re-index on file changes using `inotify`/`fsevents`
- [ ] **Hybrid search (BM25 + vector)** — combine keyword and semantic search for higher precision
- [ ] **Branch-aware indexing** — maintain separate indexes per branch
- [ ] **Pre-built binaries** — GitHub Releases with binaries for macOS, Linux, and Windows
- [ ] **C/C++ tree-sitter support** — extend AST chunking to C and C++

## Licence

MIT
