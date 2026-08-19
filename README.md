# Lookup

Lookup is a high-performance, lightweight MCP server written in Rust for web and torrent search, page reading,
research, news, weather, time, calculations, and unit conversion. Web search supports keyless SearXNG plus
optional Exa, Brave, Ollama, and Tavily providers. It is designed for LM Studio and other MCP clients that work
with local models.

## Setup

Lookup compiles directly to a native release binary. Keyless mode operates with zero external dependencies.

### macOS & Linux

The setup script ensures Rust/Cargo is installed (via `rustup` if needed), builds the release binary, and performs an MCP startup check:

```sh
chmod +x setup.sh
./setup.sh
```

No `sudo` is required.

### Windows

Run PowerShell or Command Prompt as your standard user:

```powershell
# PowerShell
.\setup.ps1
```

Or from CMD / double-click:

```cmd
setup.cmd
```

The script automatically downloads `rustup-init.exe` if `cargo` is not present, builds the release binary, and outputs the MCP configuration.

### MCP Configuration

At the end of setup, the script prints an MCP configuration containing the exact executable path for your system:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "/absolute/path/to/Lookup/target/release/lookup",
      "args": []
    }
  }
}
```

On Windows, paths will use backslashes (e.g. `C:\\path\\to\\Lookup\\target\\release\\lookup.exe`).

Copy that configuration into LM Studio or your MCP client, then restart or
reload the client.

### Manual Build

If `cargo` is already installed on your system:

```sh
cargo build --release
```

The compiled executable will be located at `target/release/lookup` (or `target\release\lookup.exe` on Windows).

### Update

Run the updater from the Lookup folder:

```sh
# macOS / Linux
./update.sh

# Windows
.\update.ps1
# or
update.cmd
```

The updater pulls the latest source and rebuilds the optimized release binary.

## Tools

| Tool | Purpose |
|---|---|
| `search_and_fetch` | Search and read a few strong results |
| `web_search` | Return compact search results and URLs |
| `read_url` | Read a known webpage |
| `research` | Gather several independent sources |
| `news_search` | Find recent news |
| `page_links` | List useful links from a webpage |
| `torrent_search` | Find magnet and `.torrent` links with metadata and validation |
| `weather` | Get current weather and a forecast |
| `current_time` | Get the time in a timezone |
| `calculate` | Evaluate a restricted mathematical expression |
| `convert_units` | Convert common units |

For most web questions, use `search_and_fetch`. Natural requests that mention a
torrent or magnet link are automatically routed through torrent-aware search.

## Search providers

Lookup works out-of-the-box without API keys:

- SearXNG provides web search.
- Direct HTTP extraction reads webpages.
- Open-Meteo provides weather and geocoding.
- Time, calculations, and unit conversion are built in.

Public SearXNG instances are best effort. For more predictable service, set a
trusted instance or enable an optional provider. Exa and Brave use their HTTP
APIs directly.

| Provider | Search | Read pages | Configuration |
|---|:---:|:---:|---|
| SearXNG | Yes | No | `SEARXNG_URL` (optional) |
| Brave | Yes | No | `BRAVE_API_KEY` |
| Exa | Yes | No | `EXA_API_KEY` |
| Ollama | Yes | Yes | `OLLAMA_API_KEY` (and optional `OLLAMA_HOST`) |
| Tavily | Yes | Yes | `TAVILY_API_KEY` |
| Direct | No | Yes | None |
| Chromium | No | Yes | `LOOKUP_CHROMIUM_PATH` (optional; Chrome, Chromium, Edge, and Brave installs are auto-detected on Linux, macOS, and Windows) |

### Exa

Set `EXA_API_KEY` to enable Exa Search. Use `provider: "exa"` in `web_search`,
`search_and_fetch`, `research`, or `news_search` to require Exa:

```json
{
  "query": "latest Rust security announcements",
  "provider": "exa",
  "max_results": 5,
  "recency": "month"
}
```

With `provider: "auto"`, Lookup uses configured providers first and Exa remains
a fallback before keyless SearXNG. Domain and recency filters are passed to
Exa's native search filters. Exa is used for search; page content is read using
the configured fetch flow, which tries Chromium first and then falls back to
other extraction providers and direct HTTP.

Add keys or provider settings to the MCP server's `env` object:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "/absolute/path/to/Lookup/target/release/lookup",
      "args": [],
      "env": {
        "EXA_API_KEY": "your-exa-key",
        "BRAVE_API_KEY": "your-key",
        "SEARXNG_URL": "https://your-searxng.example"
      }
    }
  }
}
```

Multiple SearXNG instances can be separated with commas.

## Torrent search

Torrent results can include the name, size, seeders, leechers, source, trust
signals, direct link, and BitTorrent info hash. Lookup validates magnet syntax
and torrent metainfo when possible, then removes duplicate results by info hash.

Lookup combines normal web results with Internet Archive, built-in HTML indexer
sources, and optional Torznab-compatible indexers.

Configure Torznab endpoints with a comma-separated list:

```json
{
  "env": {
    "TORZNAB_URLS": "https://indexer.example/api?apikey=your-key"
  }
}
```

Add generic HTML indexer search templates with `{query}` as the placeholder:

```json
{
  "env": {
    "TORRENT_SITE_URLS": "https://indexer.example/search?q={query}"
  }
}
```

Only obtain content that you are authorized to access. A trust signal describes
the source; it is not a legal or content-safety determination.

## Configuration

| Variable | Purpose |
|---|---|
| `BRAVE_API_KEY` | Enable Brave Search |
| `EXA_API_KEY` | Enable Exa Search |
| `OLLAMA_API_KEY` | Enable Ollama search and extraction |
| `OLLAMA_HOST` | Ollama API base URL (default: `https://api.ollama.com` or local) |
| `TAVILY_API_KEY` | Enable Tavily search and extraction |
| `SEARXNG_URL` | Preferred SearXNG instance or comma-separated instances |
| `TORZNAB_URLS` | Comma-separated Torznab API endpoints |
| `TORRENT_SITE_URLS` | Extra HTML indexer templates containing `{query}` |
| `LOOKUP_ALLOW_PRIVATE_URLS` | Allow requests to local and private addresses |
| `LOOKUP_CACHE_DB` | SQLite cache path (default: `.lookup-cache.sqlite3`; use `memory` or `off` to disable persistence) |
| `LOOKUP_CHROMIUM_PATH` | Path to a local Chromium-based browser executable; overrides Linux, macOS, and Windows auto-detection |
| `LOOKUP_LOG_LEVEL` | Logging level, such as `INFO` or `DEBUG` (default: `WARN`) |
| `TZ` | Fallback local timezone |

Private, loopback, local, and link-local destinations are blocked by default to
reduce server-side request forgery risk. Only set
`LOOKUP_ALLOW_PRIVATE_URLS=true` when you intentionally need access to trusted
local services.

## Examples

```text
Find the latest LM Studio release.
Research whether Wi-Fi 7 is worth upgrading to.
What's the weather in Vancouver tomorrow?
Read https://example.com.
Find the official Debian torrent.
```

Run the MCP server directly with:

```sh
./target/release/lookup
```

Lookup caches repeated requests in memory and SQLite, limits output size, cools down failing
providers, and guards against repetitive tool-call loops. When a supported Chromium-based browser
is available, automatic page reads try bounded local rendering first and then fall back to configured
extraction providers and direct HTTP. DNS is pinned to the validated public target and cross-host
browser requests are blocked.

## License

[MIT](LICENSE)
