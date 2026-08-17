import ast
import copy
import ipaddress
import json
import math
import os
import re
import socket
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import OrderedDict
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from datetime import datetime
from html.parser import HTMLParser
from threading import Lock
from typing import Any, Callable, Dict, Hashable, List, Optional, Tuple
from zoneinfo import ZoneInfo

VERSION = "2.0.0"
USER_AGENT = f"Lookup-MCP/{VERSION}"

OLLAMA_CLIENT: Optional[Any] = None
TAVILY_CLIENT: Optional[Any] = None
CACHE: "OrderedDict[Hashable, Tuple[float, Any]]" = OrderedDict()
PROVIDER_HEALTH: Dict[str, Dict[str, Any]] = {}
HEALTH_LOCK = Lock()

SEARCH_PROVIDERS: Tuple[str, ...] = ("auto", "brave", "ollama", "tavily", "searxng")
FETCH_PROVIDERS: Tuple[str, ...] = ("auto", "ollama", "tavily", "direct")

DEFAULT_SEARXNG_INSTANCES = [
    "https://search.mectov.my.id",
]
SEARX_SPACE_DIRECTORY_URL = "https://searx.space/data/instances.json"
SEARX_DIRECTORY_TTL = 3600
SEARX_DIRECTORY_STALE_TTL = 86400
SEARX_DIRECTORY_MAX_BYTES = 1_500_000
MAX_DISCOVERED_INSTANCES = 32
SEARX_RACE_SIZE = 3
SEARX_SEARCH_TIMEOUT = 4.0
SEARX_PREFERRED_TIMEOUT = 2.5
SEARX_PUBLIC_VALIDATION_BUDGET = 8.0
MAX_SEARX_VALIDATION_WAVES = 8
SEARX_DIRECTORY: Dict[str, Any] = {"instances": [], "expires": 0.0, "updated": 0.0}
SEARX_DIRECTORY_LOCK = Lock()

RECENCY_DAYS = {"day": 1, "week": 7, "month": 30, "year": 365}
BRAVE_FRESHNESS = {"day": "pd", "week": "pw", "month": "pm", "year": "py"}
BRAVE_WEB_SEARCH_URL = "https://api.search.brave.com/res/v1/web/search"
BRAVE_NEWS_SEARCH_URL = "https://api.search.brave.com/res/v1/news/search"

SEARCH_FAILURE_COOLDOWN = 30
PROVIDER_FAILURE_COOLDOWN = 90
AUTH_FAILURE_COOLDOWN = 600
RATE_LIMIT_COOLDOWN = 120
TIMEOUT_FAILURE_COOLDOWN = 30
CONNECTION_FAILURE_COOLDOWN = 45
NETWORK_TIMEOUT = 5

MAX_TOOL_OUTPUT_CHARS = 20000
MAX_CACHE_ENTRIES = 256
MAX_JSON_RESPONSE_BYTES = 512_000
MAX_HTML_RESPONSE_BYTES = 512_000
MAX_URL_CHARS = 4096
MAX_QUERY_CHARS = 1000

WEB_ACTIVITY_WINDOW = 60
MAX_WEB_ACTIVITY = 5
MAX_SIMILAR_WEB_ACTIVITY = 2
MAX_ACTIVITY_SCOPES = 128

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

TOOLS = {
    "web_search": {
        "description": "Find webpages about a topic. Use this when you need search results only. Do not repeatedly search the same question; use previous results when available.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 20, "default": 5},
                "provider": {"type": "string", "enum": ["auto", "brave", "ollama", "tavily", "searxng"], "default": "auto"},
                "domain": {"type": "string", "description": "Restrict to a domain like github.com"},
                "recency": {"type": "string", "enum": ["day", "week", "month", "year"]},
            },
            "required": ["query"],
        },
    },
    "search_and_fetch": {
        "description": "Search for a topic and read a few of the best results. Preferred for most web questions. The provider option controls search only; page fetching uses automatic fallback and reports the fetch provider.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 10, "default": 4},
                "fetch_results": {"type": "integer", "minimum": 1, "maximum": 5, "default": 2},
                "max_chars": {"type": "integer", "minimum": 500, "maximum": 30000, "default": 4000},
                "provider": {"type": "string", "enum": ["auto", "brave", "ollama", "tavily", "searxng"], "default": "auto", "description": "Search provider only. Page fetching uses automatic fallback."},
                "domain": {"type": "string"},
                "recency": {"type": "string", "enum": ["day", "week", "month", "year"]},
            },
            "required": ["query"],
        },
    },
    "read_url": {
        "description": "Read one specific URL that is already known. Do not use this to search the web.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "maxLength": MAX_URL_CHARS},
                "provider": {"type": "string", "enum": ["auto", "ollama", "tavily", "direct"], "default": "auto"},
                "max_chars": {"type": "integer", "minimum": 500, "maximum": 30000, "default": 6000},
                "include_links": {"type": "boolean", "default": False},
                "include_metadata": {"type": "boolean", "default": False},
            },
            "required": ["url"],
        },
    },
    "research": {
        "description": "Gather a small set of strong sources about a topic. Use when multiple sources are needed. The provider option controls search only; page fetching uses automatic fallback and reports the fetch provider.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS},
                "max_sources": {"type": "integer", "minimum": 1, "maximum": 10, "default": 3},
                "max_chars_per_source": {"type": "integer", "minimum": 500, "maximum": 50000, "default": 5000},
                "recency": {"type": "string", "enum": ["day", "week", "month", "year"]},
                "provider": {"type": "string", "enum": ["auto", "brave", "ollama", "tavily", "searxng"], "default": "auto", "description": "Search provider only. Page fetching uses automatic fallback."},
            },
            "required": ["query"],
        },
    },
    "news_search": {
        "description": "Find recent news articles about a topic. Use this for current events and recent developments.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS},
                "max_results": {"type": "integer", "minimum": 1, "maximum": 10, "default": 5},
                "recency": {"type": "string", "enum": ["day", "week", "month", "year"], "default": "week"},
                "provider": {"type": "string", "enum": ["auto", "brave", "ollama", "tavily", "searxng"], "default": "auto"},
            },
            "required": ["query"],
        },
    },
    "page_links": {
        "description": "List useful links from a specific webpage. Use this to navigate from a page you already have.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "maxLength": MAX_URL_CHARS},
                "max_links": {"type": "integer", "minimum": 1, "maximum": 25, "default": 10},
            },
            "required": ["url"],
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
    "current_time": {
        "description": "Get the current date and time in a timezone.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "timezone": {"type": "string", "description": "For example America/Vancouver. Defaults to local time."}
            },
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
    "convert_units": {
        "description": "Convert common units such as kilometers to miles, Celsius to Fahrenheit, or kilograms to pounds.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "value": {"type": "number"},
                "from_unit": {"type": "string"},
                "to_unit": {"type": "string"},
            },
            "required": ["value", "from_unit", "to_unit"],
        },
    },
}


def _purge_cache(now: float) -> None:
    expired = [key for key, (expires, _) in CACHE.items() if expires <= now]
    for key in expired:
        CACHE.pop(key, None)
    while len(CACHE) > MAX_CACHE_ENTRIES:
        CACHE.popitem(last=False)


def cache_get(key: Hashable) -> Tuple[bool, Any]:
    now = time.monotonic()
    _purge_cache(now)
    entry = CACHE.get(key)
    if entry is None:
        return False, None
    CACHE.move_to_end(key)
    return True, entry[1]


def cache_put(key: Hashable, ttl_seconds: int, value: Any) -> Any:
    now = time.monotonic()
    CACHE[key] = (now + ttl_seconds, value)
    CACHE.move_to_end(key)
    _purge_cache(now)
    return value


def cached(key: Hashable, ttl_seconds: int, produce: Callable[[], Any]) -> Any:
    found, value = cache_get(key)
    if found:
        return value
    value = produce()
    return cache_put(key, ttl_seconds, value)


def _health_available(name: str) -> bool:
    with HEALTH_LOCK:
        return time.monotonic() >= PROVIDER_HEALTH.get(name, {}).get("cooldown_until", 0.0)


def _failure_kind(exc: Exception) -> Tuple[str, int]:
    message = str(exc).lower()
    if any(token in message for token in ("unauthorized", "invalid key", "api key", "401", "403", "forbidden")):
        return "authentication", AUTH_FAILURE_COOLDOWN
    if "429" in message or "rate limit" in message or "too many requests" in message:
        return "rate_limit", RATE_LIMIT_COOLDOWN
    if "timeout" in message or "timed out" in message:
        return "timeout", TIMEOUT_FAILURE_COOLDOWN
    if any(token in message for token in ("could not reach", "connection", "dns", "resolve", "network", "json request failed")):
        return "connection", CONNECTION_FAILURE_COOLDOWN
    return "failure", PROVIDER_FAILURE_COOLDOWN


def _health_success(name: str, latency: float) -> None:
    with HEALTH_LOCK:
        previous = PROVIDER_HEALTH.get(name, {})
        PROVIDER_HEALTH[name] = {
            "healthy": True,
            "cooldown_until": 0.0,
            "last_latency": latency,
            "failure_count": 0,
            "success_count": previous.get("success_count", 0) + 1,
            "last_seen": time.monotonic(),
            "preferred": previous.get("preferred", False),
            "json_validated": previous.get("json_validated", False),
            "last_validated": previous.get("last_validated"),
        }


def _health_failure(name: str, exc: Exception) -> None:
    reason, cooldown = _failure_kind(exc)
    with HEALTH_LOCK:
        previous = PROVIDER_HEALTH.get(name, {})
        PROVIDER_HEALTH[name] = {
            "healthy": False,
            "cooldown_until": time.monotonic() + cooldown,
            "last_latency": previous.get("last_latency"),
            "failure_count": previous.get("failure_count", 0) + 1,
            "success_count": previous.get("success_count", 0),
            "failure_reason": reason,
            "last_seen": time.monotonic(),
            "preferred": False,
            "json_validated": previous.get("json_validated", False),
            "last_validated": previous.get("last_validated"),
        }


def _attempt_provider(name: str, call: Callable[[], Any]) -> Any:
    started = time.monotonic()
    try:
        value = call()
    except Exception as exc:
        _health_failure(name, exc)
        raise
    _health_success(name, time.monotonic() - started)
    return value


def _normalize_query(query: str) -> str:
    return " ".join(query.lower().split())


class _SearchGuard:
    """Bounds recent search activity per available client/session scope."""

    def __init__(self) -> None:
        self.activity: Dict[str, List[Tuple[float, str, str]]] = {}
        self.failure_until: Dict[str, float] = {}

    @staticmethod
    def _tokens(query: str) -> set:
        return set(re.findall(r"[a-z0-9]+", _normalize_query(query)))

    @classmethod
    def _similar(cls, left: str, right: str) -> bool:
        a, b = cls._tokens(left), cls._tokens(right)
        if not a or not b:
            return left == right
        overlap = len(a & b)
        return (overlap / len(a | b) >= 0.8 or
                overlap / min(len(a), len(b)) >= 0.7)

    def before_search(self, scope: str, tool: str, query: str) -> Optional[str]:
        now = time.monotonic()
        for name, items in list(self.activity.items()):
            active = [item for item in items if now - item[0] < WEB_ACTIVITY_WINDOW]
            if active:
                self.activity[name] = active
            else:
                self.activity.pop(name, None)
        for name, until in list(self.failure_until.items()):
            if until <= now:
                self.failure_until.pop(name, None)
        if scope not in self.activity and len(self.activity) >= MAX_ACTIVITY_SCOPES:
            oldest_scope = min(self.activity, key=lambda name: self.activity[name][-1][0])
            self.activity.pop(oldest_scope, None)
            self.failure_until.pop(oldest_scope, None)
        recent = [item for item in self.activity.get(scope, [])
                  if now - item[0] < WEB_ACTIVITY_WINDOW]
        self.activity[scope] = recent
        if now < self.failure_until.get(scope, 0.0):
            return ("Web search is temporarily unavailable because all providers "
                    "recently failed. Do not retry immediately.")
        norm = _normalize_query(query)
        similar = sum(1 for _, _, previous in recent if self._similar(norm, previous))
        if similar >= MAX_SIMILAR_WEB_ACTIVITY:
            return ("Similar web searches were already performed recently. Use the "
                    "results already gathered before searching again.")
        if len(recent) >= MAX_WEB_ACTIVITY:
            return ("Web activity limit reached. Use the results already gathered "
                    "before calling another search tool.")
        recent.append((now, tool, norm))
        return None

    def mark_failure(self, scope: str) -> None:
        self.failure_until[scope] = time.monotonic() + SEARCH_FAILURE_COOLDOWN


SEARCH_GUARD = _SearchGuard()


def clamp(value: Any, lo: int, hi: int) -> int:
    try:
        v = int(value)
    except (TypeError, ValueError):
        v = lo
    return max(lo, min(v, hi))


def string_arg(args: Dict[str, Any], name: str, default: str = "") -> str:
    value = args.get(name, default)
    if value is None:
        value = default
    if not isinstance(value, str):
        raise ValueError(f"{name} must be a string")
    return value.strip()


def bool_arg(args: Dict[str, Any], name: str, default: bool = False) -> bool:
    value = args.get(name, default)
    if not isinstance(value, bool):
        raise ValueError(f"{name} must be true or false")
    return value


def concise(exc: Exception) -> str:
    msg = str(exc).strip()
    return _truncate(msg or type(exc).__name__, 500)


def provider_fail_msg(kind: str, errors: Dict[str, str]) -> str:
    lines = [f"All {kind} failed."]
    for name, msg in errors.items():
        lines.append(f"{name}: {msg}")
    return "\n".join(lines)


def validate_provider(provider: str, allowed: Tuple[str, ...]) -> str:
    if provider not in allowed:
        raise ValueError(f"provider must be one of: {', '.join(allowed)}")
    return provider


def _allow_private_urls() -> bool:
    return os.environ.get("LOOKUP_ALLOW_PRIVATE_URLS", "").lower() in ("1", "true", "yes")


def validate_url(raw: Any, resolve_dns: bool = True) -> str:
    if not isinstance(raw, str):
        raise ValueError("url must be a string")
    url = raw.strip()
    if not url:
        raise ValueError("url must not be empty")
    if len(url) > MAX_URL_CHARS:
        raise ValueError("url is too long")
    try:
        parsed = urllib.parse.urlparse(url)
        host = (parsed.hostname or "").rstrip(".").lower()
        port = parsed.port or (443 if parsed.scheme.lower() == "https" else 80)
    except ValueError as exc:
        raise ValueError("url must be a valid http or https URL") from exc
    if parsed.scheme.lower() not in ("http", "https") or not host:
        raise ValueError("url must be a valid http or https URL")
    if parsed.username or parsed.password:
        raise ValueError("URLs containing credentials are not allowed")
    if _allow_private_urls():
        return url
    if host == "localhost" or host.endswith(".localhost"):
        raise ValueError("private or local URLs are not allowed")

    addresses: List[str] = []
    try:
        addresses.append(str(ipaddress.ip_address(host)))
    except ValueError:
        if resolve_dns:
            try:
                addresses.extend({item[4][0] for item in socket.getaddrinfo(
                    host, port,
                    type=socket.SOCK_STREAM)})
            except (OSError, UnicodeError) as exc:
                raise ValueError("URL hostname could not be resolved") from exc
    for address in addresses:
        try:
            ip = ipaddress.ip_address(address)
        except ValueError as exc:
            raise ValueError("URL resolved to an invalid address") from exc
        if not ip.is_global:
            raise ValueError("private or local URLs are not allowed")
    return url


class _SafeRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str,
                         headers: Any, newurl: str) -> Any:
        validate_url(newurl, resolve_dns=True)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


SAFE_OPENER = urllib.request.build_opener(_SafeRedirectHandler())


def _open_public(request: urllib.request.Request, timeout: int) -> Any:
    validate_url(request.full_url, resolve_dns=True)
    return SAFE_OPENER.open(request, timeout=timeout)


def _content_type(response: Any) -> str:
    return (response.headers.get_content_type() or "").lower()


def _declared_length(response: Any) -> Optional[int]:
    value = response.headers.get("Content-Length")
    if not value:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def get_json(url: str, timeout: float = NETWORK_TIMEOUT,
             max_bytes: int = MAX_JSON_RESPONSE_BYTES,
             headers: Optional[Dict[str, str]] = None) -> Dict[str, Any]:
    request_headers = {
        "User-Agent": USER_AGENT,
        "Accept": "application/json",
        "Accept-Encoding": "identity",
    }
    if headers:
        request_headers.update(headers)
    request = urllib.request.Request(url, headers=request_headers)
    try:
        with _open_public(request, timeout=timeout) as response:
            content_type = _content_type(response)
            if content_type not in ("application/json", "text/json") and not content_type.endswith("+json"):
                raise ValueError("Expected a JSON response")
            if (_declared_length(response) or 0) > max_bytes:
                raise ValueError("JSON response is too large")
            raw = response.read(max_bytes + 1)
            if len(raw) > max_bytes:
                raise ValueError("JSON response is too large")
            charset = response.headers.get_content_charset() or "utf-8"
            payload = json.loads(raw.decode(charset, errors="strict"))
            if not isinstance(payload, dict):
                raise ValueError("JSON response must be an object")
            return payload
    except urllib.error.HTTPError as exc:
        raise ValueError(f"HTTP {exc.code} from JSON provider") from exc
    except urllib.error.URLError as exc:
        raise ValueError("JSON request failed") from exc


def require_brave_api_key() -> str:
    api_key = os.environ.get("BRAVE_API_KEY", "").strip()
    if not api_key:
        raise ValueError("Brave API key not configured")
    return api_key


def require_ollama_api_key() -> None:
    if not os.environ.get("OLLAMA_API_KEY"):
        raise ValueError("Ollama API key not configured")


def get_ollama_client() -> Any:
    global OLLAMA_CLIENT
    require_ollama_api_key()
    if OLLAMA_CLIENT is not None:
        return OLLAMA_CLIENT
    try:
        from ollama import Client
    except ImportError as exc:
        raise ValueError("ollama package not installed") from exc
    OLLAMA_CLIENT = Client()
    return OLLAMA_CLIENT


def get_tavily_client() -> Any:
    global TAVILY_CLIENT
    if TAVILY_CLIENT is not None:
        return TAVILY_CLIENT
    api_key = os.environ.get("TAVILY_API_KEY")
    if not api_key:
        raise ValueError("Tavily API key not configured")
    try:
        from tavily import TavilyClient
    except ImportError as exc:
        raise ValueError("tavily-python not installed") from exc
    TAVILY_CLIENT = TavilyClient(api_key=api_key)
    return TAVILY_CLIENT


# ---------------- Direct HTML extraction ----------------

_NAV_JUNK = re.compile(
    r"^(home|menu|search|about|contact|login|log in|sign in|register|cart|share|"
    r"follow us|more|back to top|skip to content|privacy policy|terms of service|"
    r"terms of use|read more|continue reading)$",
    re.I,
)
_COOKIE_JUNK = re.compile(
    r"(accept|cookie|privacy|subscribe|newsletter|sign ?up|agree|consent|"
    r"manage .*preferences|this website uses|by continuing you)",
    re.I,
)


def _clean_blocks(blocks: List[str]) -> List[str]:
    seen = set()
    out: List[str] = []
    for b in blocks:
        text = b.strip()
        if not text:
            continue
        if len(text) < 60 and _NAV_JUNK.match(text):
            continue
        if len(text) < 140 and _COOKIE_JUNK.search(text):
            continue
        key = text.lower()
        if key in seen:
            continue
        seen.add(key)
        out.append(text)
    return out


def _truncate(text: str, max_chars: int) -> str:
    if not text:
        return ""
    if len(text) <= max_chars:
        return text
    cut = text[:max_chars]
    boundary = None
    for sep in ("\n\n", "\n", ". "):
        idx = cut.rfind(sep)
        if idx >= int(max_chars * 0.5):
            boundary = idx
            break
    if boundary is not None:
        cut = cut[:boundary]
    return cut.rstrip() + " [content truncated]"


def _serialized_chars(payload: Any) -> int:
    return len(json.dumps(payload, separators=(",", ":"), ensure_ascii=False))


def _string_fields(value: Any, key_name: str) -> List[Tuple[Dict[str, Any], str]]:
    found: List[Tuple[Dict[str, Any], str]] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key == key_name and isinstance(item, str):
                found.append((value, key))
            else:
                found.extend(_string_fields(item, key_name))
    elif isinstance(value, list):
        for item in value:
            found.extend(_string_fields(item, key_name))
    return found


def _shrink_field(payload: Any, key: str, minimum: int, target: int) -> bool:
    candidates = [(parent, name) for parent, name in _string_fields(payload, key)
                  if len(parent[name]) > minimum]
    if not candidates:
        return False
    parent, name = max(candidates, key=lambda item: len(item[0][item[1]]))
    overflow = max(1, _serialized_chars(payload) - target)
    new_size = max(minimum, len(parent[name]) - overflow - 24)
    if new_size > len(" [truncated]"):
        marker = " [truncated]"
        keep = max(0, new_size - len(marker))
        parent[name] = parent[name][:keep].rstrip() + marker
    else:
        parent[name] = ""
    return True


def _prune_last_list_item(value: Any) -> bool:
    if isinstance(value, dict):
        for key in ("links", "results", "sources"):
            items = value.get(key)
            if isinstance(items, list) and len(items) > 1:
                items.pop()
                return True
        return any(_prune_last_list_item(item) for item in value.values())
    if isinstance(value, list):
        return any(_prune_last_list_item(item) for item in value)
    return False


def enforce_output_budget(payload: Any, max_chars: int = MAX_TOOL_OUTPUT_CHARS) -> Any:
    """Return a JSON-safe copy whose serialized tool payload fits the hard budget."""
    out = copy.deepcopy(payload)
    if _serialized_chars(out) <= max_chars:
        return out
    # Preserve titles and URLs longest; optional metadata and bulk page text go first.
    for key, minimum in (("description", 0), ("content", 256), ("snippet", 120),
                         ("error", 80), ("text", 40)):
        while _serialized_chars(out) > max_chars and _shrink_field(out, key, minimum, max_chars):
            pass
    while _serialized_chars(out) > max_chars and _prune_last_list_item(out):
        pass
    for key, minimum in (("content", 0), ("snippet", 0), ("title", 40),
                         ("url", 80), ("query", 40)):
        while _serialized_chars(out) > max_chars and _shrink_field(out, key, minimum, max_chars):
            pass
    if _serialized_chars(out) > max_chars:
        return {"error": "Tool output exceeded the configured character budget"}
    return out


def _validate_query(raw: Any) -> str:
    if not isinstance(raw, str):
        raise ValueError("query must be a string")
    query = raw.strip()
    if not query:
        raise ValueError("query must not be empty")
    if len(query) > MAX_QUERY_CHARS:
        raise ValueError("query is too long")
    return query


def _normalize_url_key(url: str) -> str:
    parsed = urllib.parse.urlsplit(url)
    host = (parsed.hostname or "").lower()
    port = f":{parsed.port}" if parsed.port else ""
    return urllib.parse.urlunsplit((parsed.scheme.lower(), host + port,
                                   parsed.path or "/", parsed.query, ""))


def _activity_scope(args: Dict[str, Any]) -> str:
    return str(args.get("__activity_scope") or "stdio")[:200]


def _fetch_error(exc: Exception) -> str:
    msg = concise(exc)
    m = re.search(r"HTTP error (\d+)", msg)
    if m:
        return f"HTTP {m.group(1)}"
    if "Could not reach" in msg or "Request to" in msg or "failed" in msg.lower():
        return "failed"
    return msg[:60] or "failed"


class _PageParser(HTMLParser):
    SKIP = {"script", "style", "noscript", "template", "nav", "header", "footer"}
    BREAKS = {"p", "div", "br", "li", "h1", "h2", "h3", "h4", "h5", "h6",
              "tr", "blockquote", "section", "article", "table", "pre", "hr"}

    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.title = ""
        self.description = ""
        self.links: List[Dict[str, Any]] = []
        self.blocks: List[str] = []
        self._cur: List[str] = []
        self._skip = 0
        self._in_title = False
        self._link: Optional[List[Any]] = None

    def handle_starttag(self, tag: str, attrs: Any) -> None:
        a = dict(attrs)
        if tag in self.SKIP:
            self._skip += 1
        elif tag == "title":
            self._in_title = True
        elif tag == "meta":
            name = (a.get("name") or "").lower()
            if name in ("description", "og:description"):
                self.description = a.get("content", "") or self.description
        elif tag == "a":
            href = a.get("href")
            if href:
                self._link = [href, []]
        elif tag in self.BREAKS:
            self._flush()

    def handle_endtag(self, tag: str) -> None:
        if tag in self.SKIP and self._skip > 0:
            self._skip -= 1
        elif tag == "title":
            self._in_title = False
        elif tag == "a" and self._link is not None:
            self._finish_link()
        elif tag in self.BREAKS:
            self._flush()

    def handle_data(self, data: str) -> None:
        if self._skip:
            return
        if self._in_title:
            self.title = (self.title + data).strip()
        elif self._link is not None:
            self._link[1].append(data)
            self._cur.append(data)
        else:
            self._cur.append(data)

    def _flush(self) -> None:
        text = " ".join("".join(self._cur).split())
        if text:
            self.blocks.append(text)
        self._cur = []

    def _finish_link(self) -> None:
        href, parts = self._link
        text = " ".join("".join(parts).split())
        self._link = None
        self.links.append({"text": text, "url": href})


def direct_fetch(url: str, max_chars: int) -> Dict[str, Any]:
    url = validate_url(url, resolve_dns=False)
    request = urllib.request.Request(url, headers={
        "User-Agent": USER_AGENT,
        "Accept": "text/html,application/xhtml+xml,*/*",
        "Accept-Encoding": "identity",
    })
    max_bytes = min(MAX_HTML_RESPONSE_BYTES, max(64_000, max_chars * 8))
    final_url = url
    charset = "utf-8"
    try:
        with _open_public(request, timeout=10) as response:
            final_url = validate_url(response.geturl(), resolve_dns=True)
            content_type = _content_type(response)
            if content_type and content_type not in (
                    "text/html", "application/xhtml+xml", "text/plain"):
                raise ValueError(f"Unsupported content type: {content_type}")
            if (_declared_length(response) or 0) > max_bytes:
                raise ValueError("HTML response is too large")
            charset = response.headers.get_content_charset() or "utf-8"
            raw = response.read(max_bytes + 1)
            if len(raw) > max_bytes:
                raise ValueError("HTML response is too large")
    except urllib.error.HTTPError as exc:
        raise ValueError(f"HTTP error {exc.code} fetching {url}") from exc
    except urllib.error.URLError as exc:
        raise ValueError(f"Could not reach {url}") from exc
    except OSError as exc:
        raise ValueError(f"Could not reach {url}: {concise(exc)}") from exc

    parser = _PageParser()
    parser.feed(raw.decode(charset, errors="replace"))
    parser.close()

    content = _truncate("\n\n".join(_clean_blocks(parser.blocks)), max_chars)
    links = _resolve_links(parser.links, final_url or url)
    return {
        "title": parser.title,
        "description": parser.description,
        "content": content,
        "links": links,
        "final_url": final_url or url,
        "provider": "direct",
    }


def _resolve_links(raw_links: List[Dict[str, Any]], base_url: str) -> List[Dict[str, Any]]:
    seen = set()
    links: List[Dict[str, Any]] = []
    for link in raw_links:
        href = link.get("url") or ""
        text = (link.get("text") or "").strip()
        if not href:
            continue
        if href.startswith(("javascript:", "mailto:", "tel:", "data:", "ftp:")):
            continue
        resolved = urllib.parse.urljoin(base_url, href)
        if not resolved.startswith(("http://", "https://")):
            continue
        canonical = _normalize_url_key(resolved)
        if canonical in seen:
            continue
        seen.add(canonical)
        links.append({"text": text, "url": resolved})
        if len(links) >= 50:
            break
    return links


# ---------------- Provider fetching helpers ----------------

def _extract_fetch(payload: Any, url: str, max_chars: int) -> Dict[str, Any]:
    title = ""
    description = ""
    content = ""
    links: List[Any] = []
    final_url = url
    if isinstance(payload, dict):
        results = payload.get("results") or payload.get("data")
        matched = None
        if isinstance(results, list):
            for item in results:
                if isinstance(item, dict) and item.get("url"):
                    matched = item
                    break
        if matched:
            final_url = matched.get("url", url)
            title = matched.get("title") or ""
            description = matched.get("description") or ""
            content = matched.get("raw_content") or matched.get("content") or matched.get("text") or ""
            links = matched.get("links") or matched.get("anchors") or []
        if not content:
            content = payload.get("raw_content") or payload.get("content") or payload.get("text") or ""
        title = title or payload.get("title") or ""
        description = description or payload.get("description") or ""
        links = links or payload.get("links") or payload.get("anchors") or []
    norm_links = _resolve_links(
        [{"url": l if isinstance(l, str) else (l.get("url") or l.get("href") or ""),
          "text": l.get("text") if isinstance(l, dict) else ""}
         for l in links],
        final_url or url,
    )
    final_url = validate_url(final_url, resolve_dns=True)
    return {
        "title": title,
        "description": description,
        "content": _truncate(" ".join(content.split()), max_chars),
        "links": norm_links,
        "final_url": final_url,
    }


def _fetch_ollama(url: str, max_chars: int) -> Dict[str, Any]:
    validate_url(url, resolve_dns=True)
    client = get_ollama_client()
    result = _extract_fetch(client.web_fetch(url=url).model_dump(), url, max_chars)
    result["provider"] = "ollama"
    return result


def _fetch_tavily(url: str, max_chars: int) -> Dict[str, Any]:
    validate_url(url, resolve_dns=True)
    client = get_tavily_client()
    result = _extract_fetch(client.extract(urls=[url]), url, max_chars)
    result["provider"] = "tavily"
    return result


def _fetch_provider(provider: str, url: str, max_chars: int) -> Dict[str, Any]:
    url = validate_url(url, resolve_dns=False)
    if provider == "auto":
        errors: Dict[str, str] = {}
        if os.environ.get("OLLAMA_API_KEY"):
            try:
                return _fetch_ollama(url, max_chars)
            except Exception as exc:
                errors["Ollama"] = concise(exc)
        else:
            errors["Ollama"] = "API key not configured"
        if os.environ.get("TAVILY_API_KEY"):
            try:
                return _fetch_tavily(url, max_chars)
            except Exception as exc:
                errors["Tavily"] = concise(exc)
        else:
            errors["Tavily"] = "API key not configured"
        try:
            return direct_fetch(url, max_chars)
        except Exception as exc:
            errors["Direct"] = concise(exc)
        raise ValueError(provider_fail_msg("fetch providers", errors))
    if provider == "ollama":
        return _fetch_ollama(url, max_chars)
    if provider == "tavily":
        return _fetch_tavily(url, max_chars)
    return direct_fetch(url, max_chars)


# ---------------- Search helpers ----------------

def _configured_searxng_instances() -> List[str]:
    configured = os.environ.get("SEARXNG_URL", "")
    return list(dict.fromkeys(url.strip().rstrip("/") for url in configured.split(",")
                              if url.strip()))


def _number(value: Any, default: float = 0.0) -> float:
    try:
        number = float(value)
        return number if math.isfinite(number) else default
    except (TypeError, ValueError, OverflowError):
        return default


def _mapping(value: Any) -> Dict[str, Any]:
    return value if isinstance(value, dict) else {}


def _parse_searx_directory(payload: Dict[str, Any]) -> List[Dict[str, Any]]:
    raw_instances = payload.get("instances")
    if not isinstance(raw_instances, dict):
        raise ValueError("Invalid searx.space directory")
    candidates: List[Dict[str, Any]] = []
    for raw_url, detail in raw_instances.items():
        if not isinstance(raw_url, str) or not isinstance(detail, dict):
            continue
        try:
            url = validate_url(raw_url.rstrip("/"), resolve_dns=False)
        except ValueError:
            continue
        parsed = urllib.parse.urlsplit(url)
        if (parsed.scheme.lower() != "https" or parsed.query or parsed.fragment or
                detail.get("network_type") != "normal"):
            continue
        if detail.get("main") is False or detail.get("analytics") is True or detail.get("error"):
            continue
        http = _mapping(detail.get("http"))
        tls = _mapping(detail.get("tls"))
        network = _mapping(detail.get("network"))
        search = _mapping(_mapping(detail.get("timing")).get("search"))
        uptime = _mapping(detail.get("uptime"))
        if (http.get("status_code") != 200 or http.get("error") or
                tls.get("error") or not tls.get("version") or network.get("error") or
                not detail.get("version") or search.get("error")):
            continue
        success = _number(search.get("success_percentage"))
        timings = _mapping(search.get("all"))
        latency = _number(timings.get("median") or timings.get("mean") or
                          timings.get("value"), 99.0)
        week_uptime = _number(uptime.get("uptimeWeek"))
        month_uptime = _number(uptime.get("uptimeMonth"))
        if success < 90 or not 0 < latency <= 3.0 or week_uptime < 90 or month_uptime < 90:
            continue
        candidates.append({"url": url, "latency": latency, "success": success,
                           "uptime": min(week_uptime, month_uptime)})
    candidates.sort(key=lambda item: (-item["success"], item["latency"],
                                      -item["uptime"], item["url"]))
    if not candidates:
        raise ValueError("searx.space listed no suitable instances")
    return candidates[:MAX_DISCOVERED_INSTANCES]


def _cached_directory(allow_stale: bool = False) -> List[Dict[str, Any]]:
    now = time.monotonic()
    with SEARX_DIRECTORY_LOCK:
        expires = SEARX_DIRECTORY.get("expires", 0.0)
        updated = SEARX_DIRECTORY.get("updated", 0.0)
        instances = copy.deepcopy(SEARX_DIRECTORY.get("instances", []))
    if instances and (now < expires or
                      (allow_stale and now - updated < SEARX_DIRECTORY_STALE_TTL)):
        return instances
    return []


def _emergency_searxng_instances() -> List[Dict[str, Any]]:
    return [{"url": url.rstrip("/"), "latency": 99.0, "success": 0.0,
             "uptime": 0.0} for url in DEFAULT_SEARXNG_INSTANCES]


def _discover_searxng_instances() -> List[Dict[str, Any]]:
    cached_instances = _cached_directory()
    if cached_instances:
        return cached_instances
    health_name = "directory:searx.space"
    if not _health_available(health_name):
        return _cached_directory(allow_stale=True) or _emergency_searxng_instances()
    try:
        payload = _attempt_provider(health_name, lambda: get_json(
            SEARX_SPACE_DIRECTORY_URL, timeout=NETWORK_TIMEOUT,
            max_bytes=SEARX_DIRECTORY_MAX_BYTES))
        instances = _parse_searx_directory(payload)
        now = time.monotonic()
        with SEARX_DIRECTORY_LOCK:
            SEARX_DIRECTORY.update({"instances": copy.deepcopy(instances),
                                    "expires": now + SEARX_DIRECTORY_TTL,
                                    "updated": now})
        return instances
    except Exception:
        return _cached_directory(allow_stale=True) or _emergency_searxng_instances()


def _public_searxng_candidates() -> List[str]:
    directory = _discover_searxng_instances()
    metadata = {item["url"]: item for item in directory}
    urls = list(DEFAULT_SEARXNG_INSTANCES) + [item["url"] for item in directory]
    with HEALTH_LOCK:
        health = copy.deepcopy(PROVIDER_HEALTH)
    for name, status in health.items():
        if name.startswith("searxng:") and status.get("healthy"):
            url = name[len("searxng:"):]
            if url not in urls:
                urls.append(url)
    safe_urls: List[str] = []
    for url in urls:
        try:
            safe_url = validate_url(url.rstrip("/"), resolve_dns=False)
        except ValueError:
            continue
        if (safe_url.startswith("https://") and
                _health_available(f"searxng:{safe_url}")):
            safe_urls.append(safe_url)
    safe_urls = list(dict.fromkeys(safe_urls))
    safe_urls.sort(key=lambda url: (
        0 if health.get(f"searxng:{url}", {}).get("preferred") else 1,
        0 if health.get(f"searxng:{url}", {}).get("healthy") else 1,
        0 if health.get(f"searxng:{url}", {}).get("json_validated") else 1,
        0 if url in DEFAULT_SEARXNG_INSTANCES else 1,
        health.get(f"searxng:{url}", {}).get(
            "last_latency", metadata.get(url, {}).get("latency", 99.0)),
        health.get(f"searxng:{url}", {}).get("failure_count", 0),
        -metadata.get(url, {}).get("success", 0.0),
        url,
    ))
    return safe_urls


def searxng_instances() -> List[str]:
    return list(dict.fromkeys(_configured_searxng_instances() +
                              _public_searxng_candidates()))


def _mark_searxng_preferred(url: str) -> None:
    with HEALTH_LOCK:
        for name, status in PROVIDER_HEALTH.items():
            if name.startswith("searxng:"):
                status["preferred"] = name == f"searxng:{url}"


class _EmptySearxResults(ValueError):
    """The endpoint worked and returned valid JSON, but supplied no results."""


def _race_searxng(candidates: List[str], encoded: str,
                   timeout: float, accept_empty: bool = True) -> Dict[str, Any]:
    candidates = candidates[:SEARX_RACE_SIZE]
    if not candidates:
        raise ValueError("no eligible instances")

    def search_instance(base_url: str) -> Dict[str, Any]:
        name = f"searxng:{base_url}"

        def request() -> Dict[str, Any]:
            data = get_json(f"{base_url}/search?{encoded}", timeout=timeout)
            if not isinstance(data.get("results"), list):
                raise ValueError("Invalid SearXNG JSON search response")
            return data

        data = _attempt_provider(name, request)
        with HEALTH_LOCK:
            PROVIDER_HEALTH[name]["json_validated"] = True
            PROVIDER_HEALTH[name]["last_validated"] = time.monotonic()
        return data

    executor = ThreadPoolExecutor(max_workers=len(candidates),
                                  thread_name_prefix="lookup-searxng")
    futures = {executor.submit(search_instance, url): url for url in candidates}
    pending = set(futures)
    empty: Optional[Tuple[str, Dict[str, Any]]] = None
    deadline = time.monotonic() + timeout + 0.25
    try:
        while pending:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                break
            done, pending = wait(pending, timeout=remaining,
                                 return_when=FIRST_COMPLETED)
            if not done:
                break
            for future in done:
                try:
                    data = future.result()
                except Exception:
                    continue
                url = futures[future]
                if data["results"]:
                    _mark_searxng_preferred(url)
                    return {"results": data["results"]}
                empty = (url, {"results": []})
        if empty is not None:
            if accept_empty:
                return empty[1]
            raise _EmptySearxResults("instances returned no results")
        raise ValueError("all instances failed")
    finally:
        for future in pending:
            future.cancel()
        executor.shutdown(wait=False)


def _search_searxng_waves(candidates: List[str], encoded: str,
                          budget: float,
                          stop_after_empty: bool = False) -> Dict[str, Any]:
    started = time.monotonic()
    saw_empty = False
    limit = min(len(candidates), SEARX_RACE_SIZE * MAX_SEARX_VALIDATION_WAVES)
    for offset in range(0, limit, SEARX_RACE_SIZE):
        remaining = budget - (time.monotonic() - started)
        if remaining <= 0:
            break
        wave = [url for url in candidates[offset:offset + SEARX_RACE_SIZE]
                if _health_available(f"searxng:{url}")]
        if not wave:
            continue
        try:
            return _race_searxng(wave, encoded,
                                 min(SEARX_SEARCH_TIMEOUT, remaining),
                                 accept_empty=False)
        except _EmptySearxResults:
            saw_empty = True
            if stop_after_empty:
                return {"results": []}
            continue
        except ValueError:
            continue
    if saw_empty:
        return {"results": []}
    raise ValueError("all ranked instance waves failed")


def searxng_search(params: Dict[str, str]) -> Dict[str, Any]:
    encoded = urllib.parse.urlencode(params)
    configured: List[str] = []
    for url in _configured_searxng_instances():
        try:
            configured.append(validate_url(url, resolve_dns=False))
        except ValueError:
            continue
    configured = [url for url in configured
                  if _health_available(f"searxng:{url}")]
    if configured:
        try:
            return _race_searxng(configured, encoded, SEARX_SEARCH_TIMEOUT)
        except ValueError:
            pass

    with HEALTH_LOCK:
        health = copy.deepcopy(PROVIDER_HEALTH)
    preferred = [name[len("searxng:"):] for name, status in health.items()
                 if name.startswith("searxng:") and status.get("preferred") and
                 status.get("healthy") and status.get("json_validated") and
                 _health_available(name)]
    if preferred:
        try:
            return _race_searxng(preferred[:1], encoded,
                                 SEARX_PREFERRED_TIMEOUT,
                                 accept_empty=False)
        except _EmptySearxResults:
            pass
        except ValueError:
            pass
    public = [url for url in _public_searxng_candidates() if url not in preferred]
    if not public:
        raise ValueError("No healthy public SearXNG instance is currently available. Do not retry immediately.")
    try:
        result = _search_searxng_waves(
            public, encoded, SEARX_PUBLIC_VALIDATION_BUDGET,
            stop_after_empty=bool(params.get("time_range")))
    except ValueError as exc:
        raise ValueError("No working public SearXNG instance was found. Do not retry immediately.") from exc
    if result.get("results") or not params.get("time_range"):
        return result

    # Public instances frequently expose JSON but do not support time_range in any
    # enabled engine. Preserve usefulness without pretending the filter succeeded.
    relaxed_params = dict(params)
    requested_recency = relaxed_params.pop("time_range")
    relaxed_encoded = urllib.parse.urlencode(relaxed_params)
    relaxed_public = _public_searxng_candidates()
    try:
        relaxed = _search_searxng_waves(
            relaxed_public, relaxed_encoded, SEARX_PUBLIC_VALIDATION_BUDGET)
    except ValueError:
        return result
    if relaxed.get("results"):
        relaxed["recency_relaxed"] = True
        relaxed["requested_recency"] = requested_recency
        relaxed["filter_notice"] = (
            "Public SearXNG instances returned no results with the requested "
            "recency filter, so these results are unfiltered by date."
        )
    return relaxed


_RELEVANCE_STOPWORDS = {
    "a", "an", "and", "are", "at", "best", "do", "for", "from", "how",
    "in", "is", "of", "on", "the", "things", "to", "what", "where", "with",
}


def _rank_searxng_results(query: str,
                          results: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    terms = [token for token in re.findall(r"[a-z0-9]+", query.lower())
             if len(token) > 2 and token not in _RELEVANCE_STOPWORDS]
    travel_intent = bool(re.search(r"\b(things? to do|places? to (visit|see))\b",
                                   query, flags=re.I))

    def score(item: Dict[str, Any]) -> int:
        title = str(item.get("title") or "").lower()
        rest = " ".join(str(item.get(key) or "").lower()
                        for key in ("content", "snippet", "description", "url"))
        value = sum(3 for term in terms if term in title)
        value += sum(1 for term in terms if term in rest)
        if travel_intent:
            value += sum(2 for term in ("attraction", "tourism", "travel", "visit",
                                        "activity", "activities", "holiday", "destination")
                         if term in title or term in rest)
        return value

    return [item for _, item in sorted(enumerate(results),
                                       key=lambda pair: (-score(pair[1]), pair[0]))]


def normalize_results(provider: str, query: str, payload: Any, count: int) -> Dict[str, Any]:
    metadata = payload if isinstance(payload, dict) else {}
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
        try:
            url = validate_url(url, resolve_dns=False)
        except ValueError:
            continue
        snippet = ""
        for key in ("content", "snippet", "description", "text", "abstract"):
            value = item.get(key)
            if isinstance(value, str) and value.strip():
                snippet = value.strip()
                break
        result: Dict[str, Any] = {
            "title": str(item.get("title") or item.get("name") or item.get("headline") or ""),
            "url": url,
            "snippet": snippet,
        }
        published = None
        for key in ("published_at", "publishedAt", "published_date", "publishedDate", "pubDate", "date"):
            if item.get(key):
                published = item.get(key)
                break
        if published:
            result["published_at"] = str(published)
        source = item.get("source") or item.get("site_name") or item.get("domain") or item.get("engine")
        if source:
            result["source"] = str(source)
        results.append(result)
    if provider == "searxng":
        results = _rank_searxng_results(query, results)
    normalized: Dict[str, Any] = {"provider": provider, "query": query,
                                  "results": results}
    for key in ("recency_relaxed", "requested_recency", "filter_notice"):
        if key in metadata:
            normalized[key] = metadata[key]
    return normalized


def _brave_items(payload: Dict[str, Any], news: bool) -> List[Dict[str, Any]]:
    raw = payload.get("results") if news else _mapping(payload.get("web")).get("results")
    if not isinstance(raw, list):
        return []
    items: List[Dict[str, Any]] = []
    for raw_item in raw:
        if not isinstance(raw_item, dict):
            continue
        item: Dict[str, Any] = {
            "title": raw_item.get("title") or "",
            "url": raw_item.get("url") or "",
            "description": raw_item.get("description") or "",
        }
        published = raw_item.get("page_age") or raw_item.get("age")
        if published:
            item["published_at"] = published
        profile = _mapping(raw_item.get("profile"))
        meta_url = _mapping(raw_item.get("meta_url"))
        source = (profile.get("long_name") or profile.get("name") or
                  meta_url.get("hostname") or meta_url.get("netloc"))
        if source:
            item["source"] = source
        items.append(item)
    return items


def _search_brave(query: str, count: int, domain: str,
                  recency: Optional[str], news: bool = False) -> Dict[str, Any]:
    q = f"{query} site:{domain}" if domain else query
    if len(q) > 400 or len(q.split()) > 50:
        raise ValueError("Brave queries must be at most 400 characters and 50 words")
    params: Dict[str, Any] = {
        "q": q,
        "count": count,
        "search_lang": "en",
        "safesearch": "strict" if news else "moderate",
    }
    if recency:
        params["freshness"] = BRAVE_FRESHNESS[recency]
    endpoint = BRAVE_NEWS_SEARCH_URL if news else BRAVE_WEB_SEARCH_URL
    payload = get_json(
        f"{endpoint}?{urllib.parse.urlencode(params)}",
        headers={"X-Subscription-Token": require_brave_api_key()},
    )
    return {"results": _brave_items(payload, news=news)}


def _search_ollama(query: str, count: int, domain: str, recency: Optional[str]) -> Any:
    if domain or recency:
        raise ValueError("Ollama search does not support domain or recency filters")
    return get_ollama_client().web_search(query=query, max_results=count).model_dump()


def _search_tavily(query: str, count: int, domain: str, recency: Optional[str]) -> Any:
    client = get_tavily_client()
    kwargs: Dict[str, Any] = {"query": query, "max_results": count}
    if domain:
        kwargs["include_domains"] = [domain]
    if recency:
        kwargs["days"] = RECENCY_DAYS[recency]
    return client.search(**kwargs)


def _searxng_query(query: str) -> str:
    """Lead short trailing `in …` qualifiers for engines that overweight term one."""
    if any(marker in query for marker in ('"', "site:", "http://", "https://")):
        return query
    match = re.match(r"^(.+?)\s+in\s+([^,;:!?]+)$", query, flags=re.I)
    if not match:
        return query
    subject, qualifier = match.group(1).strip(), match.group(2).strip()
    if not subject or not qualifier or len(qualifier.split()) > 6:
        return query
    return f"{qualifier} {subject}"


def _search_searxng(query: str, count: int, domain: str, recency: Optional[str]) -> Any:
    q = _searxng_query(query)
    if domain:
        q = f"{query} site:{domain}"
    params = {"q": q, "format": "json", "language": "en"}
    if recency:
        params["time_range"] = recency
    return searxng_search(params)


def _search_news_ollama(query: str, count: int, recency: Optional[str]) -> Any:
    if recency:
        raise ValueError("Ollama search cannot reliably apply the news recency filter")
    return get_ollama_client().web_search(query=query, max_results=count).model_dump()


def _search_news_tavily(query: str, count: int, recency: Optional[str]) -> Any:
    client = get_tavily_client()
    kwargs: Dict[str, Any] = {"query": query, "max_results": count, "topic": "news"}
    if recency:
        kwargs["days"] = RECENCY_DAYS[recency]
    return client.search(**kwargs)


def _search_news_searxng(query: str, count: int, recency: Optional[str]) -> Any:
    params = {"q": query, "format": "json", "language": "en", "categories": "news"}
    if recency:
        params["time_range"] = recency
    return searxng_search(params)


def _do_search(provider: str, query: str, count: int, domain: str,
               recency: Optional[str], news: bool = False) -> Dict[str, Any]:
    def searcher(name: str) -> Any:
        if news:
            if name == "brave":
                return _search_brave(query, count, "", recency, news=True)
            if name == "ollama":
                return _search_news_ollama(query, count, recency)
            if name == "tavily":
                return _search_news_tavily(query, count, recency)
            return _search_news_searxng(query, count, recency)
        if name == "brave":
            return _search_brave(query, count, domain, recency)
        if name == "ollama":
            return _search_ollama(query, count, domain, recency)
        if name == "tavily":
            return _search_tavily(query, count, domain, recency)
        return _search_searxng(query, count, domain, recency)

    def attempt(name: str) -> Dict[str, Any]:
        if name == "brave":
            health_name = "search:brave"
            if not _health_available(health_name):
                raise ValueError("Brave search is temporarily cooling down")
            payload = _attempt_provider(health_name, lambda: searcher(name))
        else:
            payload = searcher(name)
        return normalize_results(name, query, payload, count)

    if provider == "auto":
        errors: Dict[str, str] = {}
        empty_providers: List[str] = []
        if os.environ.get("OLLAMA_API_KEY"):
            try:
                result = attempt("ollama")
                if result["results"]:
                    return result
                empty_providers.append("ollama")
            except Exception as exc:
                errors["Ollama"] = concise(exc)
        else:
            errors["Ollama"] = "API key not configured"
        if os.environ.get("BRAVE_API_KEY"):
            try:
                result = attempt("brave")
                if result["results"]:
                    return result
                empty_providers.append("brave")
            except Exception as exc:
                errors["Brave"] = concise(exc)
        if os.environ.get("TAVILY_API_KEY"):
            try:
                result = attempt("tavily")
                if result["results"]:
                    return result
                empty_providers.append("tavily")
            except Exception as exc:
                errors["Tavily"] = concise(exc)
        else:
            errors["Tavily"] = "API key not configured"
        try:
            result = attempt("searxng")
            if result["results"]:
                return result
            empty_providers.append("searxng")
        except Exception as exc:
            errors["SearXNG"] = concise(exc)
        if empty_providers:
            return {
                "provider": "none",
                "query": query,
                "results": [],
                "status": "no_results",
                "providers_checked": empty_providers,
            }
        raise ValueError(provider_fail_msg("search providers", errors))
    return attempt(provider)


def _collect_sources(search_result: Dict[str, Any], limit: int) -> List[Dict[str, Any]]:
    seen = set()
    picked: List[Dict[str, Any]] = []
    for r in search_result.get("results", []):
        url = r.get("url")
        if not url:
            continue
        try:
            canonical = _normalize_url_key(url)
        except ValueError:
            continue
        if canonical in seen:
            continue
        seen.add(canonical)
        picked.append(r)
        if len(picked) >= limit:
            break
    return picked


def _read_sources(results: List[Dict[str, Any]], max_chars: int,
                  total_budget: int = MAX_TOOL_OUTPUT_CHARS) -> List[Dict[str, Any]]:
    sources = []
    used = 0
    for r in results:
        if used >= total_budget:
            break
        url = r.get("url", "")
        title = r.get("title", "")
        snippet = r.get("snippet", "")
        remaining = total_budget - used
        per = min(max_chars, remaining - 200)
        if per < 300:
            break
        try:
            data = _fetch_provider("auto", url, per)
            content = data.get("content", "")
            sources.append({
                "title": data.get("title") or title,
                "url": data.get("final_url") or url,
                "snippet": snippet,
                "content": content,
                "fetch_provider": data.get("provider", "unknown"),
            })
            used += len(content) + len(title) + len(snippet) + 30
        except Exception as exc:
            sources.append({"url": url, "error": _fetch_error(exc)})
            used += len(url) + 30
    return sources


# ---------------- Tools ----------------

def web_search(args: Dict[str, Any]) -> Any:
    query = _validate_query(args.get("query"))
    provider = validate_provider(string_arg(args, "provider", "auto") or "auto", SEARCH_PROVIDERS)
    count = clamp(args.get("max_results", 5), 1, 20)
    domain = string_arg(args, "domain")
    recency = string_arg(args, "recency").lower() or None
    if recency and recency not in RECENCY_DAYS:
        raise ValueError("recency must be one of: day, week, month, year")
    cache_key = ("search", provider, _normalize_query(query), count, domain.lower(), recency)
    found, value = cache_get(cache_key)
    if found:
        return value
    scope = _activity_scope(args)
    blocked = SEARCH_GUARD.before_search(scope, "web_search", query)
    if blocked:
        raise ValueError(blocked)
    try:
        value = enforce_output_budget(_do_search(provider, query, count, domain, recency))
        return cache_put(cache_key, 300, value)
    except Exception as exc:
        if provider == "auto" and str(exc).startswith("All search providers failed"):
            SEARCH_GUARD.mark_failure(scope)
        raise


def search_and_fetch(args: Dict[str, Any]) -> Any:
    query = _validate_query(args.get("query"))
    provider = validate_provider(string_arg(args, "provider", "auto") or "auto", SEARCH_PROVIDERS)
    max_results = clamp(args.get("max_results", 4), 1, 10)
    fetch_results = clamp(args.get("fetch_results", 2), 1, 5)
    max_chars = clamp(args.get("max_chars", 4000), 500, 30000)
    domain = string_arg(args, "domain")
    recency = string_arg(args, "recency").lower() or None
    if recency and recency not in RECENCY_DAYS:
        raise ValueError("recency must be one of: day, week, month, year")
    cache_key = ("search_and_fetch", provider, _normalize_query(query), max_results,
                 fetch_results, max_chars, domain.lower(), recency)
    found, value = cache_get(cache_key)
    if found:
        return value
    scope = _activity_scope(args)
    blocked = SEARCH_GUARD.before_search(scope, "search_and_fetch", query)
    if blocked:
        raise ValueError(blocked)

    def produce() -> Dict[str, Any]:
        search_result = _do_search(provider, query, max_results, domain, recency)
        picked = _collect_sources(search_result, fetch_results)
        return {"query": query, "search_provider": search_result.get("provider"),
                "sources": _read_sources(picked, max_chars)}

    return cache_put(cache_key, 300, enforce_output_budget(produce()))


def read_url(args: Dict[str, Any]) -> Any:
    url = validate_url(args.get("url"), resolve_dns=False)
    provider = validate_provider(string_arg(args, "provider", "auto") or "auto", FETCH_PROVIDERS)
    max_chars = clamp(args.get("max_chars", 6000), 500, 30000)
    include_links = bool_arg(args, "include_links")
    include_metadata = bool_arg(args, "include_metadata")
    cache_key = ("fetch", provider, _normalize_url_key(url), max_chars,
                 include_links, include_metadata)

    def produce() -> Dict[str, Any]:
        data = _fetch_provider(provider, url, max_chars)
        out: Dict[str, Any] = {
            "url": url,
            "final_url": data.get("final_url", url),
            "content": data.get("content", ""),
            "fetch_provider": data.get("provider", provider),
        }
        if include_metadata:
            out["title"] = data.get("title", "")
            out["description"] = data.get("description", "")
        if include_links:
            out["links"] = data.get("links", [])
        return out

    return cached(cache_key, 300, lambda: enforce_output_budget(produce()))


def research(args: Dict[str, Any]) -> Any:
    query = _validate_query(args.get("query"))
    provider = validate_provider(string_arg(args, "provider", "auto") or "auto", SEARCH_PROVIDERS)
    max_sources = clamp(args.get("max_sources", 3), 1, 10)
    max_chars = clamp(args.get("max_chars_per_source", 5000), 500, 50000)
    recency = string_arg(args, "recency").lower() or None
    if recency and recency not in RECENCY_DAYS:
        raise ValueError("recency must be one of: day, week, month, year")
    cache_key = ("research", provider, _normalize_query(query), max_sources,
                 max_chars, recency)
    found, value = cache_get(cache_key)
    if found:
        return value
    scope = _activity_scope(args)
    blocked = SEARCH_GUARD.before_search(scope, "research", query)
    if blocked:
        raise ValueError(blocked)

    def produce() -> Dict[str, Any]:
        search_result = _do_search(provider, query, max_sources * 3, "", recency)
        picked = _collect_sources(search_result, max_sources)
        return {"query": query, "search_provider": search_result.get("provider"),
                "sources": _read_sources(picked, max_chars)}

    return cache_put(cache_key, 300, enforce_output_budget(produce()))


def news_search(args: Dict[str, Any]) -> Any:
    query = _validate_query(args.get("query"))
    provider = validate_provider(string_arg(args, "provider", "auto") or "auto", SEARCH_PROVIDERS)
    count = clamp(args.get("max_results", 5), 1, 10)
    recency = (string_arg(args, "recency", "week") or "week").lower()
    if recency not in RECENCY_DAYS:
        raise ValueError("recency must be one of: day, week, month, year")
    cache_key = ("news", provider, _normalize_query(query), count, recency)
    found, value = cache_get(cache_key)
    if found:
        return value
    scope = _activity_scope(args)
    blocked = SEARCH_GUARD.before_search(scope, "news_search", query)
    if blocked:
        raise ValueError(blocked)
    value = enforce_output_budget(_do_search(provider, query, count, "", recency, news=True))
    return cache_put(cache_key, 180, value)


def page_links(args: Dict[str, Any]) -> Any:
    url = validate_url(args.get("url"), resolve_dns=False)
    max_links = clamp(args.get("max_links", 10), 1, 25)
    cache_key = ("links", _normalize_url_key(url), max_links)

    def produce() -> Dict[str, Any]:
        data = direct_fetch(url, 8000)
        return {"url": url, "links": data.get("links", [])[:max_links]}

    return cached(cache_key, 300, lambda: enforce_output_budget(produce()))


def local_timezone_name() -> str:
    tz = datetime.now().astimezone().tzinfo
    return getattr(tz, "key", None) or os.environ.get("TZ", "UTC")


def current_time(args: Dict[str, Any]) -> Dict[str, Any]:
    zone_name = string_arg(args, "timezone") or local_timezone_name()
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
    location = string_arg(args, "location")
    if not location:
        raise ValueError("location must not be empty")
    raw_days = args.get("days", 3)
    if isinstance(raw_days, bool):
        raise ValueError("days must be an integer from 1 to 7")
    try:
        days = int(raw_days)
    except (TypeError, ValueError, OverflowError):
        raise ValueError("days must be an integer from 1 to 7")
    if str(raw_days).strip() not in (str(days), f"{days}.0") or not 1 <= days <= 7:
        raise ValueError("days must be an integer from 1 to 7")

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

    return cached(("weather", " ".join(location.lower().split()), days),
                  ttl_seconds=600, produce=fetch)


# ---------------- Calculator (safe AST) ----------------

MATH_FUNCS = {
    "sqrt": math.sqrt, "sin": math.sin, "cos": math.cos, "tan": math.tan,
    "log": math.log, "log10": math.log10, "log2": math.log2,
    "ceil": math.ceil, "floor": math.floor, "round": round, "abs": abs,
}
MATH_CONSTS = {"pi": math.pi, "e": math.e}
MATH_ARITY = {
    "sqrt": (1, 1), "sin": (1, 1), "cos": (1, 1), "tan": (1, 1),
    "log": (1, 2), "log10": (1, 1), "log2": (1, 1),
    "ceil": (1, 1), "floor": (1, 1), "round": (1, 2), "abs": (1, 1),
}
MAX_AST_NODES = 64
MAX_AST_DEPTH = 12
MAX_EXPONENT = 1000
MAX_ABS_NUMBER = 1e100


def _ensure_finite_number(value: Any) -> Any:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError("Expression must contain only real numbers")
    if isinstance(value, float) and not math.isfinite(value):
        raise ValueError("Non-finite numbers are not allowed")
    if abs(value) > MAX_ABS_NUMBER:
        raise ValueError("Numeric magnitude is too large")
    return value


def _validate_ast(tree: ast.AST) -> None:
    count = 0

    def visit(node: ast.AST, depth: int) -> None:
        nonlocal count
        count += 1
        if count > MAX_AST_NODES:
            raise ValueError("Expression is too complex")
        if depth > MAX_AST_DEPTH:
            raise ValueError("Expression is nested too deeply")
        for child in ast.iter_child_nodes(node):
            visit(child, depth + 1)

    visit(tree, 1)


def _eval_node(node: ast.AST) -> Any:
    if isinstance(node, ast.Expression):
        return _eval_node(node.body)
    if isinstance(node, ast.Constant):
        if isinstance(node.value, (int, float)):
            return _ensure_finite_number(node.value)
        raise ValueError("Unsupported literal")
    if isinstance(node, ast.BinOp):
        left = _eval_node(node.left)
        right = _eval_node(node.right)
        op = type(node.op)
        if op is ast.Add:
            return _ensure_finite_number(left + right)
        if op is ast.Sub:
            return _ensure_finite_number(left - right)
        if op is ast.Mult:
            return _ensure_finite_number(left * right)
        if op is ast.Div:
            if right == 0:
                raise ValueError("Division by zero")
            return _ensure_finite_number(left / right)
        if op is ast.Mod:
            if right == 0:
                raise ValueError("Division by zero")
            return _ensure_finite_number(left % right)
        if op is ast.Pow:
            if abs(right) > MAX_EXPONENT:
                raise ValueError("Exponent is too large")
            if abs(left) > 1 and right > 0:
                estimated = right * math.log10(abs(left))
                if estimated > math.log10(MAX_ABS_NUMBER):
                    raise ValueError("Power result is too large")
            try:
                return _ensure_finite_number(left ** right)
            except (OverflowError, ZeroDivisionError):
                raise ValueError("Invalid power operation")
        raise ValueError("Unsupported operator")
    if isinstance(node, ast.UnaryOp):
        value = _eval_node(node.operand)
        if isinstance(node.op, ast.USub):
            return _ensure_finite_number(-value)
        if isinstance(node.op, ast.UAdd):
            return _ensure_finite_number(+value)
        raise ValueError("Unsupported operator")
    if isinstance(node, ast.Name):
        if node.id in MATH_CONSTS:
            return MATH_CONSTS[node.id]
        raise ValueError(f"Unknown name: {node.id}")
    if isinstance(node, ast.Call):
        if not isinstance(node.func, ast.Name):
            raise ValueError("Unsupported function")
        name = node.func.id
        fn = MATH_FUNCS.get(name)
        if fn is None:
            raise ValueError(f"Unknown function: {name}")
        if node.keywords:
            raise ValueError("Unsupported keyword arguments")
        args = [_eval_node(a) for a in node.args]
        lo, hi = MATH_ARITY[name]
        if not lo <= len(args) <= hi:
            raise ValueError(f"{name} expects {lo}" + (f" to {hi}" if hi != lo else "") + " arguments")
        try:
            return _ensure_finite_number(fn(*args))
        except (TypeError, ValueError, ZeroDivisionError, OverflowError) as exc:
            raise ValueError(f"Invalid input for {name}") from exc
    raise ValueError("Unsupported expression")


def calculate(args: Dict[str, Any]) -> Dict[str, Any]:
    expression = string_arg(args, "expression")
    if not expression or len(expression) > 200:
        raise ValueError("Expression must be between 1 and 200 characters")
    try:
        tree = ast.parse(expression, mode="eval")
    except SyntaxError as exc:
        raise ValueError("Invalid expression") from exc
    _validate_ast(tree)
    try:
        result = _eval_node(tree)
    except ValueError:
        raise
    except (OverflowError, ZeroDivisionError) as exc:
        raise ValueError("Calculation error") from exc
    result = _ensure_finite_number(result)
    if isinstance(result, float) and result == int(result) and abs(result) < 1e15:
        result = int(result)
    return {"expression": expression, "result": result}


# ---------------- Unit conversion ----------------

# Each unit maps to (factor, offset) where base = value*factor + offset.
UNITS: Dict[str, Dict[str, Tuple[float, float]]] = {
    "distance": {
        "mm": (0.001, 0.0), "millimeter": (0.001, 0.0), "millimeters": (0.001, 0.0),
        "cm": (0.01, 0.0), "centimeter": (0.01, 0.0), "centimeters": (0.01, 0.0),
        "m": (1.0, 0.0), "meter": (1.0, 0.0), "meters": (1.0, 0.0),
        "km": (1000.0, 0.0), "kilometer": (1000.0, 0.0), "kilometers": (1000.0, 0.0),
        "in": (0.0254, 0.0), "inch": (0.0254, 0.0), "inches": (0.0254, 0.0),
        "ft": (0.3048, 0.0), "foot": (0.3048, 0.0), "feet": (0.3048, 0.0),
        "yd": (0.9144, 0.0), "yard": (0.9144, 0.0), "yards": (0.9144, 0.0),
        "mi": (1609.344, 0.0), "mile": (1609.344, 0.0), "miles": (1609.344, 0.0),
        "nm": (1852.0, 0.0), "nauticalmile": (1852.0, 0.0),
    },
    "mass": {
        "mg": (0.001, 0.0), "milligram": (0.001, 0.0), "milligrams": (0.001, 0.0),
        "g": (1.0, 0.0), "gram": (1.0, 0.0), "grams": (1.0, 0.0),
        "kg": (1000.0, 0.0), "kilogram": (1000.0, 0.0), "kilograms": (1000.0, 0.0),
        "oz": (28.349523125, 0.0), "ounce": (28.349523125, 0.0), "ounces": (28.349523125, 0.0),
        "lb": (453.59237, 0.0), "lbs": (453.59237, 0.0), "pound": (453.59237, 0.0), "pounds": (453.59237, 0.0),
        "st": (6350.29318, 0.0), "stone": (6350.29318, 0.0),
        "t": (1000000.0, 0.0), "tonne": (1000000.0, 0.0), "tonnes": (1000000.0, 0.0),
    },
    "volume": {
        "ml": (0.001, 0.0), "milliliter": (0.001, 0.0), "milliliters": (0.001, 0.0),
        "l": (1.0, 0.0), "liter": (1.0, 0.0), "liters": (1.0, 0.0),
        "tsp": (0.00492892159375, 0.0), "tbsp": (0.01478676478125, 0.0),
        "cup": (0.2365882365, 0.0), "cups": (0.2365882365, 0.0),
        "floz": (0.0295735295625, 0.0), "fluidounce": (0.0295735295625, 0.0),
        "gal": (3.785411784, 0.0), "gallon": (3.785411784, 0.0), "gallons": (3.785411784, 0.0),
        "cm3": (0.001, 0.0), "m3": (1000.0, 0.0),
    },
    "speed": {
        "mps": (1.0, 0.0), "meterspersecond": (1.0, 0.0),
        "kmph": (0.2777777778, 0.0), "kmh": (0.2777777778, 0.0), "kph": (0.2777777778, 0.0), "kilometersperhour": (0.2777777778, 0.0),
        "mph": (0.44704, 0.0), "milesperhour": (0.44704, 0.0),
        "kn": (0.5144444444, 0.0), "knot": (0.5144444444, 0.0), "knots": (0.5144444444, 0.0),
    },
    "temperature": {
        "c": (1.0, 0.0), "celsius": (1.0, 0.0),
        "f": (5.0 / 9.0, -32.0 * 5.0 / 9.0), "fahrenheit": (5.0 / 9.0, -32.0 * 5.0 / 9.0),
        "k": (1.0, -273.15), "kelvin": (1.0, -273.15),
    },
    "storage": {
        "b": (1.0, 0.0), "byte": (1.0, 0.0), "bytes": (1.0, 0.0),
        "kb": (1000.0, 0.0), "mb": (1000000.0, 0.0), "gb": (1000000000.0, 0.0), "tb": (1000000000000.0, 0.0),
        "kib": (1024.0, 0.0), "mib": (1048576.0, 0.0), "gib": (1073741824.0, 0.0), "tib": (1099511627776.0, 0.0),
    },
    "area": {
        "m2": (1.0, 0.0), "sqm": (1.0, 0.0), "squaremeter": (1.0, 0.0), "squaremeters": (1.0, 0.0),
        "km2": (1000000.0, 0.0), "sqkm": (1000000.0, 0.0),
        "cm2": (0.0001, 0.0),
        "ft2": (0.09290304, 0.0), "sqft": (0.09290304, 0.0),
        "in2": (0.00064516, 0.0),
        "mi2": (2589988.110336, 0.0), "sqmi": (2589988.110336, 0.0),
        "ha": (10000.0, 0.0), "hectare": (10000.0, 0.0), "ac": (4046.8564224, 0.0), "acre": (4046.8564224, 0.0),
    },
    "pressure": {
        "pa": (1.0, 0.0), "pascal": (1.0, 0.0),
        "kpa": (1000.0, 0.0), "mpa": (1000000.0, 0.0),
        "bar": (100000.0, 0.0), "mbar": (100.0, 0.0), "millibar": (100.0, 0.0),
        "atm": (101325.0, 0.0), "atmosphere": (101325.0, 0.0),
        "psi": (6894.757293168361, 0.0), "mmhg": (133.322387415, 0.0),
    },
    "time": {
        "s": (1.0, 0.0), "sec": (1.0, 0.0), "second": (1.0, 0.0), "seconds": (1.0, 0.0),
        "m": (60.0, 0.0), "min": (60.0, 0.0), "minute": (60.0, 0.0), "minutes": (60.0, 0.0),
        "h": (3600.0, 0.0), "hr": (3600.0, 0.0), "hour": (3600.0, 0.0), "hours": (3600.0, 0.0),
        "d": (86400.0, 0.0), "day": (86400.0, 0.0), "days": (86400.0, 0.0),
        "w": (604800.0, 0.0), "week": (604800.0, 0.0), "weeks": (604800.0, 0.0),
    },
}


def _resolve_unit(alias: str) -> Tuple[Optional[str], Optional[str]]:
    key = alias.lower().replace("°", "").replace(" ", "")
    matches: List[Tuple[str, str]] = []
    for category, table in UNITS.items():
        if key in table:
            matches.append((category, key))
    if len(matches) > 1:
        raise ValueError(f"Ambiguous unit: {alias}; use an unambiguous name such as meter or min")
    if matches:
        return matches[0]
    return None, None


def convert_units(args: Dict[str, Any]) -> Dict[str, Any]:
    value = args.get("value")
    if value is None:
        raise ValueError("value is required")
    if isinstance(value, bool):
        raise ValueError("value must be a finite number")
    try:
        value = float(value)
    except (TypeError, ValueError, OverflowError):
        raise ValueError("value must be a finite number")
    if not math.isfinite(value):
        raise ValueError("value must be a finite number")
    from_unit = string_arg(args, "from_unit")
    to_unit = string_arg(args, "to_unit")
    if not from_unit or not to_unit:
        raise ValueError("from_unit and to_unit are required")
    fcat, fkey = _resolve_unit(from_unit)
    tcat, tkey = _resolve_unit(to_unit)
    if fcat is None:
        raise ValueError(f"Unknown unit: {from_unit}")
    if tcat is None:
        raise ValueError(f"Unknown unit: {to_unit}")
    if fcat != tcat:
        raise ValueError(f"Cannot convert {from_unit} ({fcat}) to {to_unit} ({tcat})")
    ff, fo = UNITS[fcat][fkey]
    tf, to_ = UNITS[tcat][tkey]
    base = value * ff + fo
    result = (base - to_) / tf
    if not math.isfinite(result):
        raise ValueError("conversion result is not finite")
    result = round(result, 4)
    if result == int(result):
        result = int(result)
    return {"value": value, "from_unit": from_unit, "to_unit": to_unit, "result": result}


# ---------------- MCP ----------------

HANDLERS: Dict[str, Callable[[Dict[str, Any]], Any]] = {
    "web_search": web_search,
    "search_and_fetch": search_and_fetch,
    "read_url": read_url,
    "research": research,
    "news_search": news_search,
    "page_links": page_links,
    "weather": weather,
    "current_time": current_time,
    "calculate": calculate,
    "convert_units": convert_units,
}


def send_payload(payload: Dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def send_result(request_id: Any, result: Dict[str, Any]) -> None:
    send_payload({"jsonrpc": "2.0", "id": request_id, "result": result})


def send_rpc_error(request_id: Any, code: int, message: str) -> None:
    send_payload({"jsonrpc": "2.0", "id": request_id,
                  "error": {"code": code, "message": message}})


def send_error_text(request_id: Any, message: str) -> None:
    send_result(request_id, {"content": [{"type": "text", "text": f"Error: {message}"}],
                             "isError": True})


def handle_request(request: Dict[str, Any]) -> None:
    if not isinstance(request, dict) or request.get("jsonrpc") != "2.0" or not isinstance(request.get("method"), str):
        request_id = request.get("id") if isinstance(request, dict) else None
        send_rpc_error(request_id, -32600, "Invalid Request")
        return
    method = request.get("method")
    request_id = request.get("id")
    has_id = "id" in request

    if method == "initialize":
        if has_id:
            send_result(request_id, {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "lookup", "version": VERSION},
            })
        return

    if method == "tools/list":
        if has_id:
            send_result(request_id, {"tools": [{"name": name, **spec} for name, spec in TOOLS.items()]})
        return

    if method == "ping":
        if has_id:
            send_result(request_id, {})
        return

    if method == "tools/call":
        params = request.get("params", {})
        if not isinstance(params, dict):
            if has_id:
                send_rpc_error(request_id, -32602, "Invalid params")
            return
        name = params.get("name")
        handler = HANDLERS.get(name)
        if handler is None:
            if has_id:
                send_error_text(request_id, f"Unknown tool: {name}")
            return
        arguments = params.get("arguments", {})
        if not isinstance(arguments, dict):
            if has_id:
                send_rpc_error(request_id, -32602, "Tool arguments must be an object")
            return
        arguments = dict(arguments)
        arguments.pop("__activity_scope", None)
        meta = params.get("_meta")
        if isinstance(meta, dict):
            scope = meta.get("sessionId") or meta.get("clientId")
            if scope is not None:
                arguments["__activity_scope"] = str(scope)
        try:
            result = handler(arguments)
        except Exception as exc:
            if has_id:
                send_error_text(request_id, concise(exc))
            return
        if has_id:
            send_result(request_id, {"content": [{"type": "text", "text": json.dumps(
                result, separators=(",", ":"), ensure_ascii=False)}]})
        return

    if has_id:
        send_rpc_error(request_id, -32601, "Method not found")


def main() -> None:
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        request: Any = None
        try:
            request = json.loads(line)
            handle_request(request)
        except json.JSONDecodeError:
            send_rpc_error(None, -32700, "Parse error")
        except Exception as exc:
            request_id = request.get("id") if isinstance(request, dict) else None
            send_rpc_error(request_id, -32603, "Internal error")


if __name__ == "__main__":
    main()
