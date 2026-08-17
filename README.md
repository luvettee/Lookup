# Lookup

A lightweight MCP server for web search, URL fetching, weather, time, and calculations. Works especially well with LM Studio.

## Tools

| Tool | Description |
|------|-------------|
| `web_search` | Search the web with multiple provider options |
| `web_fetch` | Extract text content from any URL |
| `weather` | Current conditions and multi-day forecast |
| `current_time` | Date and time in any IANA timezone |
| `calculate` | Evaluate math expressions with standard functions |

## Quick Start

1. Copy `mcp.json` to your MCP client configuration
2. Set your API keys in the `env` section:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "uv",
      "args": ["run", "--quiet", "--with", "ollama", "--with", "tavily-python", "Search.py"],
      "env": {
        "OLLAMA_API_KEY": "your-key",
        "TAVILY_API_KEY": "your-key",
        "SEARXNG_URL": "your-searxng-url"
      }
    }
  }
}
```

## Search Providers

| Provider | API Key Required | Notes |
|----------|------------------|-------|
| `ollama` | Yes | Default, requires `OLLAMA_API_KEY` |
| `tavily` | Yes | Requires `TAVILY_API_KEY` and `tavily-python` |
| `searxng` | No | Free, can work without an API key; uses public instances or set `SEARXNG_URL` |

## API Keys

- **Ollama** (`web_search`, `web_fetch`): requires `OLLAMA_API_KEY`.
- **Tavily** (`web_search`, `web_fetch`): requires `TAVILY_API_KEY` and `tavily-python`.
- **SearXNG** (`web_search` only): no API key required. Uses public instances, or set `SEARXNG_URL` to one or more comma-separated instance URLs.
- **Weather, Time, Calculator**: require no API key at all.

## Weather

Uses [Open-Meteo](https://open-meteo.com) — no API key needed. Supports up to 7-day forecasts with geocoding.

## Calculator

Supports: `+`, `-`, `*`, `/`, `%`, `**`, parentheses, and functions (`sin`, `cos`, `sqrt`, `log`, `ceil`, `floor`, etc.)

## Dependencies

- Python 3.10+
- [uv](https://github.com/astral-sh/uv) (for running)
- `ollama` package (for Ollama provider)
- `tavily-python` (for Tavily provider)
