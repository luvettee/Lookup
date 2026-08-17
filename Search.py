import json
import math
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime
from typing import Any, Callable, Dict, List, Optional, Tuple
from zoneinfo import ZoneInfo

from ollama import Client

OLLAMA = Client()
TAVILY_CLIENT: Optional[Any] = None
CACHE: Dict[str, Tuple[float, Any]] = {}

WEATHER_CODES = {
    0: "clear", 1: "mostly clear", 2: "partly cloudy", 3: "overcast",
    45: "fog", 48: "rime fog",
    51: "light drizzle", 53: "drizzle", 55: "heavy drizzle",
    56: "light freezing drizzle", 57: "freezing drizzle",
    61: "light rain", 63: "rain", 65: "heavy rain",
    66: "light freezing rain", 67: "freezing rain",
    71: "light snow", 73: "snow", 75: "heavy snow", 77: "snow grains",
    80: "light rain showers", 81: "rain showers", 82: "heavy rain showers",
    85: "light snow showers", 86: "heavy snow showers",
    95: "thunderstorm", 96: "thunderstorm with hail", 99: "thunderstorm with heavy hail",
}

MATH_NAMES = {
    "ceil": math.ceil, "floor": math.floor, "sqrt": math.sqrt,
    "sin": math.sin, "cos": math.cos, "tan": math.tan,
    "log": math.log, "log10": math.log10, "log2": math.log2,
    "exp": math.exp, "abs": abs, "round": round,
    "pi": math.pi, "e": math.e,
}
SAFE_EXPRESSION = re.compile(r"^[0-9a-zA-Z_+\-*/%.,()\s\*\*]*$")

DEFAULT_SEARXNG_INSTANCES = [
    "https://searx.be",
    "https://priv.au",
    "https://search.inetol.net",
]

SEARCH_PROVIDERS: Tuple[str, ...] = ("ollama", "tavily", "searxng")
FETCH_PROVIDERS: Tuple[str, ...] = ("ollama", "tavily")

TOOLS = {
    "current_time": {
        "description": "Get the current date and time in an IANA timezone.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "timezone": {"type": "string", "description": "For example America/Vancouver. Defaults to local time."}
            },
        },
    },
    "weather": {
        "description": "Get current weather and a forecast for a location.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "location": {"type": "string"},
                "days": {"type": "integer", "minimum": 1, "maximum": 7, "default": 3},
            },
            "required": ["location"],
        },
    },
    "calculate": {
        "description": "Calculate a mathematical expression.",
        "inputSchema": {
            "type": "object",
            "properties": {"expression": {"type": "string"}},
            "required": ["expression"],
        },
    },
    "web_search": {
        "description": "Search the web for current information and return relevant results.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 10, "default": 5},
                "provider": {"type": "string", "enum": ["ollama", "tavily", "searxng"], "default": "ollama"},
            },
            "required": ["query"],
        },
    },
    "web_fetch": {
        "description": "Read and extract useful text from a webpage URL.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "provider": {"type": "string", "enum": ["ollama", "tavily"], "default": "ollama"},
            },
            "required": ["url"],
        },
    },
}


def cached(key: str, ttl_seconds: int, produce: Callable[[], Any]) -> Any:
    entry = CACHE.get(key)
    now = time.monotonic()
    if entry and now - entry[0] < ttl_seconds:
        return entry[1]
    value = produce()
    CACHE[key] = (now, value)
    return value


def get_json(url: str) -> Dict[str, Any]:
    request = urllib.request.Request(url, headers={"User-Agent": "Lookup-MCP/1.0.0"})
    try:
        with urllib.request.urlopen(request, timeout=8) as response:
            return json.loads(response.read().decode())
    except urllib.error.URLError as exc:
        raise ValueError(f"Request to {url} failed: {exc}") from exc


def require_ollama_api_key() -> None:
    if not os.environ.get("OLLAMA_API_KEY"):
        raise ValueError("Set OLLAMA_API_KEY in mcp.json before using this tool.")


def get_tavily_client() -> Any:
    global TAVILY_CLIENT
    if TAVILY_CLIENT is not None:
        return TAVILY_CLIENT
    api_key = os.environ.get("TAVILY_API_KEY")
    if not api_key:
        raise ValueError("Set TAVILY_API_KEY in mcp.json before using the tavily provider.")
    try:
        from tavily import TavilyClient
    except ImportError as exc:
        raise ValueError("Install tavily-python (pip install tavily-python) to use the tavily provider.") from exc
    TAVILY_CLIENT = TavilyClient(api_key=api_key)
    return TAVILY_CLIENT


def resolve_provider(args: Dict[str, Any], allowed: Tuple[str, ...]) -> str:
    provider = args.get("provider", "ollama")
    if provider not in allowed:
        raise ValueError(f"provider must be one of: {', '.join(allowed)}")
    return provider


def normalize_results(provider: str, query: str, payload: Any, count: int) -> Dict[str, Any]:
    raw = payload
    if isinstance(raw, dict):
        raw = raw.get("results") or raw.get("data") or raw.get("items") or raw
    if not isinstance(raw, list):
        raw = []
    results = []
    for item in raw[:count]:
        if not isinstance(item, dict):
            continue
        url = item.get("url") or item.get("link") or item.get("href")
        if not url:
            continue
        snippet = ""
        for key in ("content", "snippet", "description", "text", "abstract"):
            value = item.get(key)
            if isinstance(value, str) and value.strip():
                snippet = value.strip()
                break
        results.append({
            "title": str(item.get("title") or item.get("name") or item.get("headline") or ""),
            "url": url,
            "snippet": snippet,
        })
    return {"provider": provider, "query": query, "results": results}


def normalize_fetch(provider: str, url: str, payload: Any) -> Dict[str, Any]:
    content = ""
    raw = payload
    if isinstance(raw, dict):
        for item in (raw.get("results") or raw.get("data") or []):
            if isinstance(item, dict) and item.get("url") == url:
                content = item.get("raw_content") or item.get("content") or item.get("text") or ""
                break
        if not content:
            content = raw.get("raw_content") or raw.get("content") or raw.get("text") or ""
    return {"provider": provider, "url": url, "content": content}


def searxng_instances() -> List[str]:
    configured = os.environ.get("SEARXNG_URL")
    if configured:
        instances = [url.strip().rstrip("/") for url in configured.split(",") if url.strip()]
        if instances:
            return instances
    return DEFAULT_SEARXNG_INSTANCES


def searxng_search(query: str) -> Dict[str, Any]:
    params = urllib.parse.urlencode({"q": query, "format": "json", "language": "en"})
    last_error: Optional[Exception] = None
    for base_url in searxng_instances():
        try:
            data = get_json(f"{base_url}/search?{params}")
            return {"results": data.get("results", [])}
        except Exception as exc:
            last_error = exc
            continue
    raise ValueError(f"All SearXNG instances failed, last error: {last_error}")


def local_timezone_name() -> str:
    tz = datetime.now().astimezone().tzinfo
    return getattr(tz, "key", None) or os.environ.get("TZ", "UTC")


def current_time(args: Dict[str, Any]) -> Dict[str, Any]:
    zone_name = args.get("timezone") or local_timezone_name()
    try:
        zone = ZoneInfo(zone_name)
    except Exception as exc:
        raise ValueError(f"Unknown timezone: {zone_name}") from exc
    now = datetime.now(zone)
    return {
        "timezone": zone_name,
        "iso": now.isoformat(),
        "date": now.strftime("%A, %B %-d, %Y"),
        "time": now.strftime("%-I:%M %p"),
    }


def geocode(location: str) -> Dict[str, Any]:
    params = urllib.parse.urlencode({"name": location, "count": 1, "language": "en", "format": "json"})
    results = get_json(f"https://geocoding-api.open-meteo.com/v1/search?{params}").get("results", [])
    if not results:
        raise ValueError(f"Location not found: {location}")
    return results[0]


def weather(args: Dict[str, Any]) -> Dict[str, Any]:
    location = args["location"].strip()
    if not location:
        raise ValueError("location must not be empty")
    days = max(1, min(int(args.get("days", 3)), 7))

    def fetch() -> Dict[str, Any]:
        place = geocode(location)
        fields = {
            "latitude": place["latitude"],
            "longitude": place["longitude"],
            "timezone": "auto",
            "forecast_days": days,
            "current": "temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,weather_code,wind_speed_10m",
            "daily": "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
        }
        data = get_json(f"https://api.open-meteo.com/v1/forecast?{urllib.parse.urlencode(fields)}")
        current, daily = data["current"], data["daily"]
        return {
            "location": ", ".join(filter(None, [place.get("name"), place.get("admin1"), place.get("country")])),
            "timezone": data["timezone"],
            "current": {
                "temperature_c": current["temperature_2m"],
                "feels_like_c": current["apparent_temperature"],
                "humidity_percent": current["relative_humidity_2m"],
                "wind_kmh": current["wind_speed_10m"],
                "precipitation_mm": current["precipitation"],
                "condition": WEATHER_CODES.get(current["weather_code"], "unknown"),
            },
            "forecast": [
                {
                    "date": daily["time"][i],
                    "condition": WEATHER_CODES.get(daily["weather_code"][i], "unknown"),
                    "high_c": daily["temperature_2m_max"][i],
                    "low_c": daily["temperature_2m_min"][i],
                    "rain_chance_percent": daily["precipitation_probability_max"][i],
                }
                for i in range(days)
            ],
        }

    return cached(f"weather:{location.lower()}:{days}", ttl_seconds=600, produce=fetch)


def calculate(args: Dict[str, Any]) -> Dict[str, Any]:
    expression = args["expression"]
    if not expression or len(expression) > 200:
        raise ValueError("Expression must be between 1 and 200 characters")
    if not SAFE_EXPRESSION.match(expression) and not all(
        token in MATH_NAMES or token.strip() == "" for token in re.findall(r"[a-zA-Z_]+", expression)
    ):
        raise ValueError("Unsupported expression")
    try:
        result = eval(expression, {"__builtins__": {}}, MATH_NAMES)
    except ZeroDivisionError as exc:
        raise ValueError("Division by zero") from exc
    except (SyntaxError, NameError, TypeError) as exc:
        raise ValueError(f"Invalid expression: {exc}") from exc
    if not isinstance(result, (int, float)) or isinstance(result, bool):
        raise ValueError("Expression did not produce a number")
    return {"expression": expression, "result": result}


def web_search(args: Dict[str, Any]) -> Any:
    provider = resolve_provider(args, SEARCH_PROVIDERS)
    query = (args.get("query") or "").strip()
    if not query:
        raise ValueError("query must not be empty")
    count = max(1, min(int(args.get("max_results", 5)), 10))
    cache_key = f"search:{provider}:{query}:{count}"

    def produce() -> Any:
        if provider == "tavily":
            client = get_tavily_client()
            return client.search(query=query, max_results=count)
        if provider == "searxng":
            return searxng_search(query)
        require_ollama_api_key()
        return OLLAMA.web_search(query=query, max_results=count).model_dump()

    return cached(cache_key, ttl_seconds=300, produce=lambda: normalize_results(provider, query, produce(), count))


def web_fetch(args: Dict[str, Any]) -> Any:
    provider = resolve_provider(args, FETCH_PROVIDERS)
    url = (args.get("url") or "").strip()
    if not url:
        raise ValueError("url must not be empty")
    cache_key = f"fetch:{provider}:{url}"

    def produce() -> Any:
        if provider == "tavily":
            client = get_tavily_client()
            return client.extract(urls=[url])
        require_ollama_api_key()
        return OLLAMA.web_fetch(url=url).model_dump()

    return cached(cache_key, ttl_seconds=300, produce=lambda: normalize_fetch(provider, url, produce()))


HANDLERS: Dict[str, Callable[[Dict[str, Any]], Any]] = {
    "current_time": current_time,
    "weather": weather,
    "calculate": calculate,
    "web_search": web_search,
    "web_fetch": web_fetch,
}


def send(request_id: Any, result: Dict[str, Any]) -> None:
    payload = {"jsonrpc": "2.0", "id": request_id, "result": result}
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def send_error_text(request_id: Any, message: str) -> None:
    send(request_id, {"content": [{"type": "text", "text": f"Error: {message}"}], "isError": True})


def handle_request(request: Dict[str, Any]) -> None:
    method = request.get("method")
    request_id = request.get("id")

    if method == "initialize":
        send(request_id, {
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "Lookup", "version": "1.0.0"},
        })
        return

    if method == "tools/list":
        send(request_id, {"tools": [{"name": name, **spec} for name, spec in TOOLS.items()]})
        return

    if method == "tools/call":
        params = request.get("params", {})
        name = params.get("name")
        handler = HANDLERS.get(name)
        if handler is None:
            send_error_text(request_id, f"Unknown tool: {name}")
            return
        try:
            result = handler(params.get("arguments", {}))
        except Exception as exc:
            send_error_text(request_id, str(exc))
            return
        send(request_id, {"content": [{"type": "text", "text": json.dumps(result, separators=(",", ":"))}]})
        return

    if request_id is not None:
        send(request_id, {})


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        request: Optional[Dict[str, Any]] = None
        try:
            request = json.loads(line)
            handle_request(request)
        except json.JSONDecodeError:
            continue
        except Exception as exc:
            request_id = request.get("id") if isinstance(request, dict) else None
            if request_id is not None:
                send_error_text(request_id, str(exc))


if __name__ == "__main__":
    main()
