# Lookup

Lookup is a lightweight MCP server for web and torrent search, page reading,
research, news, weather, time, calculations, and unit conversion. It is designed
for LM Studio and other MCP clients that work with local models.

## Setup

Lookup is not installed as a Python package. LM Studio launches `Search.py`
directly with a Python interpreter. Keyless mode uses only Python's standard
library.

The setup script supports macOS and Linux. It installs
[`uv`](https://docs.astral.sh/uv/) for the current user when needed, lets `uv`
install Python when needed, creates `.venv`, and checks `Search.py` directly.
It does not build or install Lookup, and no lockfile is required.

```sh
chmod +x setup.sh
./setup.sh
```

No `sudo` is required. The bootstrap needs either `curl` or `wget` and an
internet connection the first time it runs.

At the end, the script prints an MCP configuration containing the exact
executable path for this checkout. It looks like this:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "/absolute/path/to/Lookup/.venv/bin/python",
      "args": ["/absolute/path/to/Lookup/Search.py"]
    }
  }
}
```

Copy that configuration into LM Studio or your MCP client, then restart or
reload the client.

### Update

Run the updater from the Lookup folder:

```sh
./update.sh
```

The updater downloads only the newest `Search.py` from the repository's `main`
branch, validates it, performs an MCP startup check, and then replaces the local
source. It does not use GitHub Releases or modify `.venv`, `mcp.json`, API keys,
the README, or other local files.

### Use an existing Python

If Python 3.9 or newer is already installed, `uv` and `.venv` are optional. Use
the path returned by `command -v python3` as `command`, with the absolute path
to `Search.py` in `args`.

### Manual environment

If `uv` is already installed:

```sh
uv venv --python 3.12
.venv/bin/python Search.py
```

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

Lookup works without API keys:

- SearXNG provides web search.
- Direct HTTP extraction reads webpages.
- Open-Meteo provides weather and geocoding.
- Time, calculations, and unit conversion are built in.

Public SearXNG instances are best effort. For more predictable service, set a
trusted instance or enable an optional provider. Exa and Brave use their HTTP
APIs directly and do not require additional Python packages.

| Provider | Search | Read pages | Configuration |
|---|:---:|:---:|---|
| SearXNG | Yes | No | `SEARXNG_URL` (optional) |
| Brave | Yes | No | `BRAVE_API_KEY` |
| Exa | Yes | No | `EXA_API_KEY` |
| Ollama | Yes | Yes | `OLLAMA_API_KEY` and the `ollama` package |
| Tavily | Yes | Yes | `TAVILY_API_KEY` and the `tavily-python` package |
| Direct | No | Yes | None |

Set `provider` to `exa` in `web_search`, `search_and_fetch`, `research`, or
`news_search` to require Exa. With `provider: auto`, Lookup uses Exa as a
configured fallback before keyless SearXNG. Domain and recency filters are
passed to Exa's native search filters.

Install an optional provider library into `.venv` with:

```sh
uv pip install --python .venv/bin/python ollama
uv pip install --python .venv/bin/python tavily-python
```

Add keys or provider settings to the MCP server's `env` object:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "/absolute/path/to/Lookup/.venv/bin/python",
      "args": ["/absolute/path/to/Lookup/Search.py"],
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
| `TAVILY_API_KEY` | Enable Tavily search and extraction |
| `SEARXNG_URL` | Preferred SearXNG instance or comma-separated instances |
| `TORZNAB_URLS` | Comma-separated Torznab API endpoints |
| `TORRENT_SITE_URLS` | Extra HTML indexer templates containing `{query}` |
| `LOOKUP_ALLOW_PRIVATE_URLS` | Allow requests to local and private addresses |
| `LOOKUP_LOG_LEVEL` | Logging level, such as `INFO` or `DEBUG` |
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
.venv/bin/python Search.py
```

Lookup caches repeated requests in memory, limits output size, cools down failing
providers, and guards against repetitive tool-call loops.

## License

[MIT](LICENSE)
