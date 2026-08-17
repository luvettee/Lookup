# Lookup

**Lookup** is a lightweight MCP server for web search, page reading, research, recent news, weather, time, calculations, and unit conversion.

It is designed for **LM Studio and local models** with simple tools, compact responses, fast fallbacks, and protections against unnecessary tool loops.

## Quick Start

### Requirements

- Python 3.9+
- [`uv`](https://github.com/astral-sh/uv)

### Add Lookup to LM Studio

Use this as your MCP configuration:

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "uv",
      "args": [
        "run",
        "--quiet",
        "/Users/bober/Desktop/Lookup/Search.py"
      ]
    }
  }
}
```

If your `Search.py` is somewhere else, replace the path with its absolute path.

That is enough to run Lookup in **keyless mode**.

## What Lookup Can Do

| Tool | Use it for |
|---|---|
| `search_and_fetch` | Most web questions — searches and reads a few good pages |
| `web_search` | Search results / URLs only |
| `read_url` | Read one URL you already have |
| `research` | Gather a few independent sources |
| `news_search` | Recent news |
| `page_links` | Get useful links from a webpage |
| `weather` | Current weather and forecast |
| `current_time` | Date and time in a timezone |
| `calculate` | Safe math expressions |
| `convert_units` | Unit conversion |

### Which web tool should I use?

For most current-information questions, use:

```text
search_and_fetch
```

Use the others when you specifically need:

```text
web_search       → search results only
read_url         → read a known URL
research         → multiple sources
news_search      → recent news
page_links       → links from a known page
```

## Keyless Mode

Without any API keys, Lookup can use:

- **SearXNG** for web search
- **Direct HTTP extraction** for reading webpages
- **Open-Meteo** for weather
- Built-in time, math, and unit conversion

When `SEARXNG_URL` is not configured, Lookup can discover public SearXNG instances through **searx.space** and prefer healthy working instances automatically.

Public SearXNG instances are third-party services and may be slow, rate-limited, unavailable, or protected by bot challenges. For the most predictable keyless setup, use your own trusted SearXNG instance.

## Optional Search Providers

Lookup also supports **Brave Search**, **Ollama**, and **Tavily**.

### Brave Search

Brave uses its API directly and does not require an extra Python package.

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "uv",
      "args": [
        "run",
        "--quiet",
        "/Users/bober/Desktop/Lookup/Search.py"
      ],
      "env": {
        "BRAVE_API_KEY": "your-key"
      }
    }
  }
}
```

### Ollama

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "uv",
      "args": [
        "run",
        "--quiet",
        "--with",
        "ollama",
        "/Users/bober/Desktop/Lookup/Search.py"
      ],
      "env": {
        "OLLAMA_API_KEY": "your-key"
      }
    }
  }
}
```

### Tavily

```json
{
  "mcpServers": {
    "Lookup": {
      "command": "uv",
      "args": [
        "run",
        "--quiet",
        "--with",
        "tavily-python",
        "/Users/bober/Desktop/Lookup/Search.py"
      ],
      "env": {
        "TAVILY_API_KEY": "your-key"
      }
    }
  }
}
```

## Providers

| Provider | Search | Read pages | Key |
|---|:---:|:---:|---|
| `auto` | Yes | Yes | Depends |
| `brave` | Yes | No | `BRAVE_API_KEY` |
| `ollama` | Yes | Yes | `OLLAMA_API_KEY` |
| `tavily` | Yes | Yes | `TAVILY_API_KEY` |
| `searxng` | Yes | No | None |
| `direct` | No | Yes | None |

`auto` prefers configured healthy providers and falls back when needed.

For page reading, Lookup can use direct HTTP extraction and configured provider extractors.

## Lightweight by Default

Lookup is intentionally conservative with context.

Typical behavior:

- `web_search` → 5 compact results
- `search_and_fetch` → up to 2 strong pages
- `read_url` → about 5,000 characters
- `research` → 3 sources
- `news_search` → 5 compact results
- `page_links` → 10 useful links

Web-tool responses are capped so a single call does not flood a smaller model's context window.

Lookup also reuses cached searches/pages, fetches independent sources concurrently, skips recently failing providers, and stops early when enough useful information has already been gathered.

## SearXNG

You can optionally provide one or more preferred SearXNG instances:

```json
{
  "env": {
    "SEARXNG_URL": "https://your-searxng.example"
  }
}
```

Multiple instances can be comma-separated.

If none are configured, Lookup can lazily discover public instances from searx.space, reject unhealthy/bot-protected instances, and remember working instances for later searches.

Public-instance availability is **best effort**, not guaranteed.

## Examples

### Search and read

```json
{ "query": "latest LM Studio release" }
```

Use with `search_and_fetch`.

### Search only

```json
{ "query": "LM Studio documentation" }
```

Use with `web_search`.

### Read a URL

```json
{ "url": "https://example.com" }
```

Use with `read_url`.

### Research

```json
{ "query": "Is Wi-Fi 7 worth upgrading to?" }
```

Use with `research`.

### News

```json
{ "query": "NVIDIA" }
```

Use with `news_search`.

### Weather

```json
{ "location": "Vancouver", "days": 3 }
```

### Time

```json
{ "timezone": "America/Vancouver" }
```

### Calculator

```json
{ "expression": "sqrt(16) + 2**3" }
```

### Unit conversion

```json
{ "value": 100, "from_unit": "km", "to_unit": "mi" }
```

## Environment Variables

| Variable | Purpose |
|---|---|
| `BRAVE_API_KEY` | Enable Brave Search |
| `OLLAMA_API_KEY` | Enable Ollama |
| `TAVILY_API_KEY` | Enable Tavily |
| `SEARXNG_URL` | Preferred SearXNG instance(s) |
| `LOOKUP_ALLOW_PRIVATE_URLS` | Allow local/private URL destinations |
| `TZ` | Fallback local timezone |

## Safety

Lookup only accepts HTTP(S) URLs and blocks private, loopback, local, and link-local destinations by default.

If you intentionally need access to trusted local services:

```json
{
  "env": {
    "LOOKUP_ALLOW_PRIVATE_URLS": "true"
  }
}
```

Only enable this if you understand the security implications.

The calculator uses a restricted evaluator with limits on expression size, complexity, exponentiation, and numeric magnitude.

## Notes

- Search and page results are cached in memory for faster repeated requests.
- Recent provider and host failures enter cooldowns instead of being retried immediately.
- Lookup limits repeated web activity to help smaller models avoid tool-call loops.
- Public SearXNG searches are sent to third-party instance operators.
- Ollama and Tavily are optional dependencies.

## Example Prompts

```text
What's the weather in Vancouver tomorrow?
```

```text
Find the latest LM Studio release.
```

```text
What changed in local LLM tooling this month?
```

```text
Research whether Wi-Fi 7 is worth upgrading to.
```

```text
Read https://example.com.
```

```text
Convert 100 km to miles.
```

## Philosophy

> **Lightweight, fast, and accurate.**

Lookup is designed to retrieve enough information to answer well without overwhelming the model with unnecessary context.
