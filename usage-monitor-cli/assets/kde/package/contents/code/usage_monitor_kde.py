#!/usr/bin/env python3
"""Data helper for the Usage Monitor KDE Plasma widget.

Ported from the CodexBar KDE helper. The Plasma UI stays simple QML; this helper
owns all JSON, CLI, cache and formatting logic so it can be tested without KDE
running. The presentation layer (summarize/tooltip/bar text/classify) is kept
from the original; only the data layer is adapted to drive `usage-monitor-cli`.
"""

from __future__ import annotations

import argparse
import configparser
import json
import math
import os
import re
import subprocess
import sys
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from shutil import which
from typing import Any, Callable


PLASMOID_VERSION = "0.10.3"

# usage-monitor has a single auto-detected source per provider, so the Source
# combo in the settings UI only ever offers "auto".
DEFAULT_AVAILABLE_SOURCES = ["auto"]

PROVIDER_NAMES = {
    "anthropic": "Anthropic",
    "claude": "Claude",
    "codex": "Codex",
    "openai": "OpenAI",
    "opencode-go": "OpenCode Go",
    "openrouter": "OpenRouter",
    "deepseek": "DeepSeek",
    "groq": "Groq",
    "llmproxy": "LLM Proxy",
    "deepgram": "Deepgram",
    "abacus": "Abacus",
    "minimax": "MiniMax",
    "kimik2": "Kimi K2",
    "kimi": "Kimi",
    "zai": "Z.ai",
    "elevenlabs": "ElevenLabs",
    "mistral": "Mistral",
    "cursor": "Cursor",
    "gemini": "Gemini",
}
WINDOW_LABELS = {"primary": "Session", "secondary": "Weekly", "tertiary": "Monthly"}
WINDOW_SLOTS = ("primary", "secondary", "tertiary")

CONNECT_HINTS = {
    "claude": "Run `claude` and sign in, then refresh. Credentials are auto-detected.",
    "codex": "Run `codex` and sign in, then refresh. Credentials are auto-detected.",
    "anthropic": "Set an API key: `usage-monitor-cli anthropic set api_key sk-…`.",
    "openai": "Set an API key: `usage-monitor-cli openai set api_key sk-…`.",
    "gemini": "Run `gcloud auth application-default login`, then refresh.",
    "opencode-go": "Set an API key: `usage-monitor-cli opencode-go set token <key>` (auto-detected from ~/.local/share/opencode/auth.json).",
    "kimi": "Set a kimi-auth token: `usage-monitor-cli kimi set token <kimi-auth-jwt>`.",
}

# --------------------------------------------------------------------------
# Per-provider "add account" metadata for the settings UI.
#
# authKind drives the form shape:
#   api_key / token / cookie -> paste-a-secret form (name + the listed fields)
#   oauth                    -> CLI-login required; show setupHint + path fields
# Each field: {key, label, secret, placeholder}.
# --------------------------------------------------------------------------


def _field(key: str, label: str, secret: bool = False, placeholder: str = "") -> dict[str, Any]:
    return {"key": key, "label": label, "secret": secret, "placeholder": placeholder}


_API_KEY = [_field("api_key", "API key", secret=True, placeholder="sk-…")]
_TOKEN = [_field("token", "Token", secret=True)]
_COOKIE = [_field("cookie", "Session cookie", secret=True)]

_OAUTH_SETUP = {
    "codex": (
        "Codex needs an isolated CLI login per account (you cannot copy auth.json):\n"
        "  CODEX_HOME=~/.codex-NAME codex login --device-auth\n"
        "Then set Credentials path to ~/.codex-NAME/auth.json."
    ),
    "claude": (
        "Claude Code uses ~/.claude. For a second account, keep a separate\n"
        ".credentials.json and set Credentials path to it."
    ),
    "gemini": (
        "Log in with `gemini` (~/.gemini/oauth_creds.json) and set Credentials path,\n"
        "or paste an access token from `gcloud auth print-access-token`."
    ),
    "antigravity": (
        "Point Credentials path at the account's oauth_creds.json. Token refresh may\n"
        "need client_id/client_secret (set them as extra account keys)."
    ),
}

PROVIDER_AUTH: dict[str, dict[str, Any]] = {
    # API-key providers
    **{p: {"kind": "api_key", "fields": _API_KEY} for p in (
        "openai", "anthropic", "openrouter", "groq", "deepseek", "kimik2",
        "minimax", "moonshot", "venice", "zai", "elevenlabs",
    )},
    "deepgram": {"kind": "api_key", "fields": [*_API_KEY, _field("project_id", "Project ID")]},
    "llmproxy": {
        "kind": "api_key",
        "fields": [*_API_KEY, _field("base_url", "Base URL", placeholder="https://…")],
    },
    # token providers
    **{p: {"kind": "token", "fields": _TOKEN} for p in ("grok", "kimi", "copilot", "windsurf")},
    "devin": {"kind": "token", "fields": [*_TOKEN, _field("org", "Organization")]},
    # cookie providers
    **{p: {"kind": "cookie", "fields": _COOKIE} for p in ("abacus", "mistral", "ollama", "cursor", "perplexity")},
    # OAuth / CLI providers (account added via terminal login + credentials path)
    "codex": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path", placeholder="~/.codex-NAME/auth.json")], "setupHint": _OAUTH_SETUP["codex"]},
    "claude": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path", placeholder="~/.claude/.credentials.json")], "setupHint": _OAUTH_SETUP["claude"]},
    "gemini": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path"), _field("access_token", "Access token", secret=True)], "setupHint": _OAUTH_SETUP["gemini"]},
    "antigravity": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path"), _field("access_token", "Access token", secret=True)], "setupHint": _OAUTH_SETUP["antigravity"]},
    # opencode-go: API key (auto-detected from the desktop login file)
    "opencode-go": {"kind": "token", "fields": [_field("token", "API key", secret=True)]},
}

_DEFAULT_AUTH = {"kind": "api_key", "fields": _API_KEY}


def provider_auth(provider_id: str) -> dict[str, Any]:
    return PROVIDER_AUTH.get(provider_id, _DEFAULT_AUTH)


@dataclass(frozen=True)
class Paths:
    state: Path
    cache_dir: Path
    last_good: Path


def xdg_path(env_name: str, fallback: Path) -> Path:
    raw = os.environ.get(env_name)
    return Path(raw).expanduser() if raw else fallback


def paths() -> Paths:
    home = Path.home()
    config_home = xdg_path("XDG_CONFIG_HOME", home / ".config")
    cache_home = xdg_path("XDG_CACHE_HOME", home / ".cache")
    cache_dir = cache_home / "usage-monitor-kde"
    return Paths(
        state=config_home / "usage-monitor-kde" / "state.json",
        cache_dir=cache_dir,
        last_good=cache_dir / "last.json",
    )


def load_json(path: Path, default: Any) -> Any:
    try:
        with path.open("r", encoding="utf-8") as fh:
            return json.load(fh)
    except FileNotFoundError:
        return default
    except json.JSONDecodeError:
        return default
    except OSError:
        return default


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8") as fh:
        json.dump(payload, fh, separators=(",", ":"))
        fh.write("\n")
    tmp.replace(path)


# --------------------------------------------------------------------------
# CLI discovery + execution
# --------------------------------------------------------------------------

# Plasma launches the plasmoid with a minimal PATH that usually omits
# ~/.cargo/bin and ~/.local/bin, so `which` alone is not enough.
_SEARCH_DIRS = (
    Path.home() / ".cargo" / "bin",
    Path("/usr/bin"),
    Path("/usr/local/bin"),
    Path.home() / ".local" / "bin",
)
_BINARY_NAMES = ("usage-monitor-cli", "usage-monitor")


def usage_monitor_binary(env: Mapping[str, str] | None = None) -> str:
    env = env or os.environ
    override = env.get("USAGE_MONITOR_BIN")
    if override:
        return override
    for name in _BINARY_NAMES:
        found = which(name, path=env.get("PATH"))
        if found:
            return found
    for directory in _SEARCH_DIRS:
        for name in _BINARY_NAMES:
            candidate = directory / name
            if candidate.exists():
                return str(candidate)
    return "usage-monitor-cli"


def run_cli(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess[str]:
    cmd = [usage_monitor_binary(), *args]
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, check=False)
    except FileNotFoundError:
        return subprocess.CompletedProcess(
            cmd,
            127,
            "",
            "usage-monitor-cli not found.\n\n"
            "Build it (`cargo install --path usage-monitor-cli`) or set USAGE_MONITOR_BIN "
            "to the full path of the binary.",
        )
    except Exception as exc:  # pragma: no cover - defensive
        return subprocess.CompletedProcess(cmd, 1, "", str(exc))


def cli_output(args: list[str]) -> str:
    proc = run_cli(args)
    return proc.stdout if proc.returncode == 0 else ""


# --------------------------------------------------------------------------
# Widget-only state
# --------------------------------------------------------------------------


def _state_path(state_path: Path | None = None) -> Path:
    return state_path or paths().state


def state_full(state_path: Path | None = None) -> dict[str, Any]:
    state = load_json(_state_path(state_path), {})
    return state if isinstance(state, dict) else {}


def state_value(state_path: Path | None = None, key: str = "barProvider", default: str = "") -> str:
    state = state_full(state_path)
    value = state.get(key, default)
    return str(value) if value is not None else default


def _write_state(payload: dict[str, Any], state_path: Path | None = None) -> None:
    write_json(_state_path(state_path), payload)


def _provider_order() -> list[str]:
    raw = state_value(key="providerOrder", default="")
    if not raw:
        return []
    try:
        order = json.loads(raw)
        return [str(x) for x in order] if isinstance(order, list) else []
    except (json.JSONDecodeError, TypeError):
        return []


def _sort_by_order(providers: list[dict[str, Any]]) -> list[dict[str, Any]]:
    order = _provider_order()
    if not order:
        return providers
    ranked: list[dict[str, Any]] = []
    rest: list[dict[str, Any]] = []
    seen = set()
    for oid in order:
        for entry in providers:
            if entry.get("provider") == oid and oid not in seen:
                ranked.append(entry)
                seen.add(oid)
    for entry in providers:
        if entry.get("provider") not in seen:
            rest.append(entry)
    return ranked + rest


# --------------------------------------------------------------------------
# Theming
#
# Four modes, all resolved here so the QML only ever reads final values:
#   plasma  -> follow the desktop theme (no colors emitted; QML uses Kirigami)
#   builtin -> one of BUILTIN_THEMES
#   scheme  -> a KDE color scheme already installed on the system (including
#              third-party ones from the KDE Store, e.g. the macOS look-alikes)
#   custom  -> per-key overrides stored in state.json (themeCustom* keys)
# --------------------------------------------------------------------------

THEME_MODES = ("plasma", "builtin", "scheme", "custom")

THEME_COLOR_KEYS = ("background", "text", "subtext", "accent", "warning", "critical", "track", "border")

DEFAULT_FONT = {"family": "", "size": 0, "headingSize": 0, "smallSize": 0}
DEFAULT_METRICS = {"barHeight": 6, "radius": 4, "opacity": 1.0}


def _builtin(name: str, dark: bool, colors: dict[str, str], **metrics: Any) -> dict[str, Any]:
    return {
        "name": name,
        "dark": dark,
        "colors": colors,
        "font": dict(DEFAULT_FONT),
        "metrics": {**DEFAULT_METRICS, **metrics},
    }


BUILTIN_THEMES: dict[str, dict[str, Any]] = {
    "macos-dark": _builtin(
        "macOS Dark",
        True,
        {
            "background": "#1c1c1e",
            "text": "#f5f5f7",
            "subtext": "#98989d",
            "accent": "#0a84ff",
            "warning": "#ff9f0a",
            "critical": "#ff453a",
            "track": "#3a3a3c",
            "border": "#48484a",
        },
        radius=8,
    ),
    "macos-light": _builtin(
        "macOS Light",
        False,
        {
            "background": "#f5f5f7",
            "text": "#1d1d1f",
            "subtext": "#6e6e73",
            "accent": "#007aff",
            "warning": "#ff9500",
            "critical": "#ff3b30",
            "track": "#d1d1d6",
            "border": "#c6c6c8",
        },
        radius=8,
    ),
    "nord": _builtin(
        "Nord",
        True,
        {
            "background": "#2e3440",
            "text": "#eceff4",
            "subtext": "#81a1c1",
            "accent": "#88c0d0",
            "warning": "#ebcb8b",
            "critical": "#bf616a",
            "track": "#3b4252",
            "border": "#4c566a",
        },
    ),
    "dracula": _builtin(
        "Dracula",
        True,
        {
            "background": "#282a36",
            "text": "#f8f8f2",
            "subtext": "#6272a4",
            "accent": "#bd93f9",
            "warning": "#ffb86c",
            "critical": "#ff5555",
            "track": "#44475a",
            "border": "#44475a",
        },
    ),
    "tokyo-night": _builtin(
        "Tokyo Night",
        True,
        {
            "background": "#1a1b26",
            "text": "#c0caf5",
            "subtext": "#565f89",
            "accent": "#7aa2f7",
            "warning": "#e0af68",
            "critical": "#f7768e",
            "track": "#24283b",
            "border": "#292e42",
        },
    ),
}

DEFAULT_BUILTIN = "macos-dark"

# Only the forms QColor actually parses: #rgb, #rrggbb, #aarrggbb. A 4-digit
# "#argb" is not one of them, so it must not be let through.
_HEX_RE = re.compile(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$")


def color_value(raw: Any) -> str:
    """Return a QML-safe color string, or "" when the value is unusable.

    Accepts `#rgb`, `#argb`, `#rrggbb`, `#aarrggbb` and KDE's `R,G,B` triples.
    Anything else is dropped so QML never gets an invalid color assignment.
    """
    text = str(raw or "").strip()
    if not text:
        return ""
    if _HEX_RE.match(text):
        return text.lower()
    parts = [p.strip() for p in text.split(",")]
    if len(parts) in (3, 4) and all(p.isdigit() for p in parts[:3]):
        try:
            r, g, b = (max(0, min(255, int(p))) for p in parts[:3])
        except ValueError:
            return ""
        return f"#{r:02x}{g:02x}{b:02x}"
    return ""


def is_dark(color: str) -> bool:
    hex_text = color.lstrip("#")
    if len(hex_text) == 8:  # #aarrggbb -> drop the alpha channel
        hex_text = hex_text[2:]
    if len(hex_text) == 3:
        hex_text = "".join(ch * 2 for ch in hex_text)
    if len(hex_text) != 6:
        return True
    try:
        r, g, b = (int(hex_text[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        return True
    return (0.299 * r + 0.587 * g + 0.114 * b) < 128


def _data_dirs() -> list[Path]:
    home = Path.home()
    dirs = [xdg_path("XDG_DATA_HOME", home / ".local" / "share")]
    raw = os.environ.get("XDG_DATA_DIRS") or "/usr/local/share:/usr/share"
    dirs += [Path(part).expanduser() for part in raw.split(":") if part.strip()]
    seen: set[Path] = set()
    unique: list[Path] = []
    for directory in dirs:
        if directory not in seen:
            seen.add(directory)
            unique.append(directory)
    return unique


def _read_ini(path: Path) -> configparser.ConfigParser | None:
    parser = configparser.ConfigParser(strict=False, interpolation=None)
    parser.optionxform = str  # KDE keys are case-sensitive
    try:
        with path.open("r", encoding="utf-8", errors="replace") as fh:
            parser.read_file(fh)
    except (OSError, configparser.Error):
        return None
    return parser


def read_color_scheme(path: Path) -> dict[str, str]:
    """Map a KDE `.colors` file (or a desktop theme `colors` file) to our tokens."""
    parser = _read_ini(path)
    return _scheme_colors(parser) if parser else {}


def _scheme_colors(parser: configparser.ConfigParser) -> dict[str, str]:
    def get(section: str, key: str) -> str:
        try:
            return color_value(parser.get(section, key))
        except (configparser.NoSectionError, configparser.NoOptionError):
            return ""

    background = get("Colors:Window", "BackgroundNormal") or get("Colors:View", "BackgroundNormal")
    text = get("Colors:Window", "ForegroundNormal") or get("Colors:View", "ForegroundNormal")
    if not background or not text:
        return {}
    subtext = get("Colors:Window", "ForegroundInactive") or text
    colors = {
        "background": background,
        "text": text,
        "subtext": subtext,
        "accent": get("Colors:Window", "DecorationFocus") or get("Colors:Window", "ForegroundLink") or "#0a84ff",
        "warning": get("Colors:Window", "ForegroundNeutral") or "#ff9f0a",
        "critical": get("Colors:Window", "ForegroundNegative") or "#ff453a",
        "track": get("Colors:Button", "BackgroundNormal") or subtext,
        "border": subtext,
    }
    return colors


def _desktop_theme_name(theme_dir: Path) -> str:
    meta = load_json(theme_dir / "metadata.json", {})
    plugin = meta.get("KPlugin") if isinstance(meta, dict) else None
    if isinstance(plugin, dict):
        name = str(plugin.get("Name") or "").strip()
        if name:
            return name
    return theme_dir.name


def _colors_scheme_entry(path: Path) -> dict[str, Any] | None:
    """One entry from a `.colors` file — colors and display name in one parse."""
    parser = _read_ini(path)
    if parser is None:
        return None
    colors = _scheme_colors(parser)
    if not colors:
        return None
    name = parser.get("General", "Name", fallback="").strip() or path.stem
    return {
        "id": f"colors:{path.stem}",
        "name": name,
        "path": str(path),
        "dark": is_dark(colors["background"]),
        "colors": colors,
    }


def _desktop_theme_entry(theme_dir: Path) -> dict[str, Any] | None:
    colors_file = theme_dir / "colors"
    if not colors_file.is_file():
        return None
    colors = read_color_scheme(colors_file)
    if not colors:
        return None
    return {
        "id": f"desktoptheme:{theme_dir.name}",
        "name": _desktop_theme_name(theme_dir),
        "path": str(colors_file),
        "dark": is_dark(colors["background"]),
        "colors": colors,
    }


def installed_schemes() -> list[dict[str, Any]]:
    """Every KDE color scheme / Plasma desktop theme installed for this user."""
    found: dict[str, dict[str, Any]] = {}
    for base in _data_dirs():
        for path in sorted((base / "color-schemes").glob("*.colors")):
            entry = _colors_scheme_entry(path) if f"colors:{path.stem}" not in found else None
            if entry:
                found[entry["id"]] = entry
        for theme_dir in sorted((base / "plasma" / "desktoptheme").glob("*")):
            entry = _desktop_theme_entry(theme_dir) if f"desktoptheme:{theme_dir.name}" not in found else None
            if entry:
                found[entry["id"]] = entry
    return sorted(found.values(), key=lambda item: str(item["name"]).lower())


def find_scheme(scheme_id: str) -> dict[str, Any] | None:
    """Resolve one scheme id straight to its file.

    The id carries the file stem, so the widget's refresh path never has to scan
    (and re-parse) every installed scheme just to render the selected one.
    """
    scheme_id = str(scheme_id or "")
    kind, _, name = scheme_id.partition(":")
    if not name or "/" in name or name in (".", ".."):
        return None
    for base in _data_dirs():
        if kind == "colors":
            path = base / "color-schemes" / f"{name}.colors"
            entry = _colors_scheme_entry(path) if path.is_file() else None
        elif kind == "desktoptheme":
            theme_dir = base / "plasma" / "desktoptheme" / name
            entry = _desktop_theme_entry(theme_dir) if theme_dir.is_dir() else None
        else:
            return None
        if entry:
            return entry
    return None


def _scheme_entry(scheme_id: str, schemes: list[dict[str, Any]] | None = None) -> dict[str, Any] | None:
    if schemes is None:
        return find_scheme(scheme_id)
    return next((scheme for scheme in schemes if scheme.get("id") == scheme_id), None)


def _number(raw: Any, fallback: float) -> float:
    """Parse a number, rejecting nan/inf.

    `float("inf")` and `float("nan")` are serialized by json.dump as bare
    Infinity/NaN, which QML's JSON.parse rejects — a single such value in
    state.json would break every helper command, with no way back from the UI.
    """
    try:
        value = float(str(raw).strip())
    except (TypeError, ValueError):
        return fallback
    return value if math.isfinite(value) else fallback


def _custom_colors(sf: Mapping[str, Any]) -> dict[str, str]:
    overrides: dict[str, str] = {}
    for key in THEME_COLOR_KEYS:
        value = color_value(sf.get(f"themeCustom{key.capitalize()}", ""))
        if value:
            overrides[key] = value
    return overrides


def _legacy_custom_theme(sf: Mapping[str, Any]) -> dict[str, Any] | None:
    """The pre-`customThemes` single custom palette, as a named theme.

    Widgets configured before named custom themes existed keep their colors: the
    flat `themeCustom*` keys are surfaced as one theme called "Custom", which the
    user can then rename, duplicate or delete like any other.
    """
    colors = _custom_colors(sf)
    font = {
        "family": str(sf.get("themeCustomFontFamily", "") or ""),
        "size": _number(sf.get("themeCustomFontSize", 0), 0),
        "headingSize": _number(sf.get("themeCustomHeadingSize", 0), 0),
        "smallSize": _number(sf.get("themeCustomSmallSize", 0), 0),
    }
    metrics = {
        "barHeight": _number(sf.get("themeCustomBarHeight", DEFAULT_METRICS["barHeight"]), DEFAULT_METRICS["barHeight"]),
        "radius": _number(sf.get("themeCustomRadius", DEFAULT_METRICS["radius"]), DEFAULT_METRICS["radius"]),
        "opacity": max(0.1, min(1.0, _number(sf.get("themeCustomOpacity", 1.0), 1.0))),
    }
    touched = bool(colors) or bool(font["family"]) or any(font[k] for k in ("size", "headingSize", "smallSize"))
    touched = touched or metrics != dict(DEFAULT_METRICS)
    if not touched:
        return None
    return sanitize_custom_theme({
        "id": "custom",
        "name": "Custom",
        "base": str(sf.get("themeBuiltin", "") or DEFAULT_BUILTIN),
        "colors": colors,
        "font": font,
        "metrics": metrics,
    })


def sanitize_custom_theme(raw: Any, index: int = 0) -> dict[str, Any] | None:
    """Normalize a user-defined theme; unusable values are dropped, not trusted."""
    if not isinstance(raw, Mapping):
        return None
    theme_id = str(raw.get("id") or "").strip() or f"custom-{index + 1}"
    name = str(raw.get("name") or "").strip() or "Custom theme"
    base = str(raw.get("base") or "")
    if base not in BUILTIN_THEMES:
        base = DEFAULT_BUILTIN
    raw_colors = raw.get("colors") if isinstance(raw.get("colors"), Mapping) else {}
    colors = {}
    for key in THEME_COLOR_KEYS:
        value = color_value(raw_colors.get(key, ""))
        if value:
            colors[key] = value
    raw_font = raw.get("font") if isinstance(raw.get("font"), Mapping) else {}
    font = {
        "family": str(raw_font.get("family", "") or ""),
        "size": max(0.0, _number(raw_font.get("size", 0), 0)),
        "headingSize": max(0.0, _number(raw_font.get("headingSize", 0), 0)),
        "smallSize": max(0.0, _number(raw_font.get("smallSize", 0), 0)),
    }
    raw_metrics = raw.get("metrics") if isinstance(raw.get("metrics"), Mapping) else {}
    metrics = {
        "barHeight": _number(raw_metrics.get("barHeight", DEFAULT_METRICS["barHeight"]), DEFAULT_METRICS["barHeight"]),
        "radius": _number(raw_metrics.get("radius", DEFAULT_METRICS["radius"]), DEFAULT_METRICS["radius"]),
        "opacity": max(0.1, min(1.0, _number(raw_metrics.get("opacity", 1.0), 1.0))),
    }
    return {"id": theme_id, "name": name, "base": base, "colors": colors, "font": font, "metrics": metrics}


def custom_themes(sf: Mapping[str, Any] | None = None) -> list[dict[str, Any]]:
    """Every named custom theme, oldest first (ids deduplicated)."""
    sf = sf if isinstance(sf, Mapping) else state_full()
    raw = sf.get("customThemes", "")
    parsed: Any = raw
    # An explicit (even empty) list means the user has managed their themes in
    # the config page; only a missing/unparsable key falls back to the legacy
    # flat keys, so deleting the last theme does not resurrect the old palette.
    stored_list = isinstance(raw, list)
    if isinstance(raw, str):
        try:
            parsed = json.loads(raw) if raw.strip() else None
            stored_list = isinstance(parsed, list)
        except json.JSONDecodeError:
            parsed = None
    themes: list[dict[str, Any]] = []
    seen: set[str] = set()
    for index, item in enumerate(parsed if isinstance(parsed, list) else []):
        theme = sanitize_custom_theme(item, index)
        if theme and theme["id"] not in seen:
            seen.add(theme["id"])
            themes.append(theme)
    if not themes and not stored_list:
        legacy = _legacy_custom_theme(sf)
        if legacy:
            themes.append(legacy)
    return themes


def custom_theme_colors(theme: Mapping[str, Any]) -> dict[str, str]:
    base = BUILTIN_THEMES.get(str(theme.get("base") or DEFAULT_BUILTIN), BUILTIN_THEMES[DEFAULT_BUILTIN])
    overrides = theme.get("colors") if isinstance(theme.get("colors"), Mapping) else {}
    return {**base["colors"], **{k: v for k, v in overrides.items() if v}}


def theme_payload(mode: str, theme_id: str, name: str, dark: bool, colors: dict[str, str],
                  font: dict[str, Any], metrics: dict[str, Any]) -> dict[str, Any]:
    return {
        "mode": mode,
        "id": theme_id,
        "name": name,
        "dark": dark,
        "colors": colors,
        "font": font,
        "metrics": metrics,
    }


def global_opacity(sf: Mapping[str, Any], fallback: float = 1.0) -> float:
    """Background opacity for every mode (the Theme page's transparency slider).

    Kept outside the themes themselves so it also applies to "follow the desktop
    theme", where the widget has no palette of its own.
    """
    raw = sf.get("themeOpacity", "")
    if raw is None or str(raw).strip() == "":
        return max(0.1, min(1.0, fallback))
    return max(0.1, min(1.0, _number(raw, fallback)))


def resolve_theme(sf: Mapping[str, Any] | None = None,
                  schemes: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    """Turn the theme state keys into the final palette the QML renders with."""
    sf = sf if isinstance(sf, Mapping) else state_full()
    mode = str(sf.get("themeMode", "plasma") or "plasma")
    if mode not in THEME_MODES:
        mode = "plasma"

    def with_opacity(payload: dict[str, Any], fallback: float = 1.0) -> dict[str, Any]:
        payload["metrics"] = {**payload["metrics"], "opacity": global_opacity(sf, fallback)}
        return payload

    if mode == "plasma":
        return with_opacity(theme_payload("plasma", "", "Current desktop theme", False, {},
                                          dict(DEFAULT_FONT), dict(DEFAULT_METRICS)))

    if mode == "builtin":
        theme_id = str(sf.get("themeBuiltin", DEFAULT_BUILTIN) or DEFAULT_BUILTIN)
        theme = BUILTIN_THEMES.get(theme_id) or BUILTIN_THEMES[DEFAULT_BUILTIN]
        if theme_id not in BUILTIN_THEMES:
            theme_id = DEFAULT_BUILTIN
        return with_opacity(theme_payload("builtin", theme_id, theme["name"], theme["dark"],
                                          dict(theme["colors"]), dict(theme["font"]),
                                          dict(theme["metrics"])))

    if mode == "scheme":
        scheme_id = str(sf.get("themeScheme", "") or "")
        entry = _scheme_entry(scheme_id, schemes)
        colors = dict(entry["colors"]) if entry and entry.get("colors") else {}
        if not colors:
            # The scheme was uninstalled (or never resolved) — fall back to the
            # desktop theme rather than rendering an unreadable palette.
            return with_opacity(theme_payload("plasma", "", "Current desktop theme", False, {},
                                              dict(DEFAULT_FONT), dict(DEFAULT_METRICS)))
        name = str(entry.get("name") or scheme_id)
        return with_opacity(theme_payload("scheme", scheme_id, name, is_dark(colors["background"]),
                                          colors, dict(DEFAULT_FONT), dict(DEFAULT_METRICS)))

    themes = custom_themes(sf)
    if not themes:
        # Custom selected but nothing defined yet: show the base theme rather
        # than an empty palette, so the widget stays readable.
        base = BUILTIN_THEMES.get(str(sf.get("themeBuiltin", "") or ""), BUILTIN_THEMES[DEFAULT_BUILTIN])
        return with_opacity(theme_payload("custom", "", "Custom", base["dark"], dict(base["colors"]),
                                          dict(DEFAULT_FONT), dict(DEFAULT_METRICS)))
    wanted = str(sf.get("themeCustomId", "") or "")
    theme = next((t for t in themes if t["id"] == wanted), themes[0])
    colors = custom_theme_colors(theme)
    # A theme saved before the global slider existed keeps its own opacity until
    # the slider is touched.
    return with_opacity(
        theme_payload("custom", theme["id"], theme["name"], is_dark(colors["background"]),
                      colors, dict(theme["font"]), dict(theme["metrics"])),
        fallback=_number(theme["metrics"].get("opacity", 1.0), 1.0),
    )


def theme_catalog(schemes: list[dict[str, Any]] | None = None,
                  sf: Mapping[str, Any] | None = None) -> dict[str, Any]:
    catalog = schemes if schemes is not None else installed_schemes()
    return {
        # font/metrics travel with each built-in so the settings preview can
        # mirror exactly what resolve_theme() will apply (corner radius, sizes).
        "builtin": [
            {
                "id": theme_id,
                "name": theme["name"],
                "dark": theme["dark"],
                "colors": dict(theme["colors"]),
                "font": dict(theme["font"]),
                "metrics": dict(theme["metrics"]),
            }
            for theme_id, theme in BUILTIN_THEMES.items()
        ],
        "schemes": catalog,
        "custom": custom_themes(sf),
        "colorKeys": list(THEME_COLOR_KEYS),
    }


# --------------------------------------------------------------------------
# Fetch + entry shaping (usage-monitor-cli widget JSON -> codexbar entry shape)
# --------------------------------------------------------------------------

Runner = Callable[[list[str]], subprocess.CompletedProcess[str]]


def provider_name(provider_id: str | None) -> str:
    if not provider_id:
        return "Provider"
    return PROVIDER_NAMES.get(provider_id, provider_id.replace("-", " ").replace("_", " ").title())


def _percent(value: Any) -> float | None:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _entry_from_widget_provider(item: dict[str, Any]) -> dict[str, Any]:
    provider_id = str(item.get("provider_id") or "")
    windows = item.get("windows") if isinstance(item.get("windows"), list) else []
    usage: dict[str, Any] = {}
    extras: list[dict[str, Any]] = []
    # Match windows to slots by id: the CLI appends named extra windows
    # (e.g. Codex "Additional" rate limits) after the standard ones, and
    # mapping positionally mislabels them as Monthly.
    for window in [w for w in windows if isinstance(w, dict)]:
        wid = str(window.get("id") or "")
        slot = {
            "usedPercent": _percent(window.get("percentage")) or 0.0,
            "resetsAt": None,
            "resetDescription": str(window.get("resets_at") or ""),
        }
        if wid in WINDOW_SLOTS and wid not in usage:
            usage[wid] = slot
        else:
            extras.append(
                {
                    "label": str(window.get("label") or wid or "Additional"),
                    "usedPercent": slot["usedPercent"],
                    "resetDescription": slot["resetDescription"],
                }
            )
    if not usage and extras:
        # Legacy fallback: a provider emitting only unnamed windows keeps the
        # old positional mapping so its cards are not blank.
        for key, extra in zip(WINDOW_SLOTS, extras, strict=False):
            usage[key] = {
                "usedPercent": extra["usedPercent"],
                "resetsAt": None,
                "resetDescription": extra["resetDescription"],
            }
        extras = extras[len(usage):]
    if extras:
        usage["extra"] = extras
    # Prefer the real account email (e.g. Codex id_token) for identification,
    # falling back to a configured label/id, then the plan as a last resort so
    # every provider shows something under its name.
    identity = item.get("account_email") or item.get("account_label") or item.get("account_id") or ""
    usage["identity"] = {
        "accountEmail": str(identity or ""),
        "accountOrganization": str(item.get("plan") or ""),
    }
    entry: dict[str, Any] = {
        "provider": provider_id,
        "displayName": str(item.get("display_name") or provider_name(provider_id)),
        # Explicit account id ("" for the implicit auto-detected default), so
        # the panel bar can pin one account of a multi-account provider.
        "account": str(item.get("account_id") or ""),
        "usage": usage,
    }
    if item.get("error"):
        entry["error"] = {"message": str(item.get("error"))}
    # Carry the CLI's own max so providers whose windows don't map cleanly to the
    # three slots still report a sane headline percentage.
    cli_max = _percent(item.get("max_percentage"))
    if cli_max is not None:
        entry["_cliMaxPercent"] = cli_max
    return entry


def fetch_entries(runner: Runner = run_cli) -> list[dict[str, Any]]:
    proc = runner(["widget", "kde"])
    if proc.returncode != 0 and not proc.stdout.strip():
        return [{"provider": "", "error": {"message": (proc.stderr or "usage-monitor-cli failed").strip()}}]
    try:
        payload = json.loads(proc.stdout or "{}")
    except json.JSONDecodeError:
        return [{"provider": "", "error": {"message": "invalid JSON from usage-monitor-cli"}}]
    providers = payload.get("providers", []) if isinstance(payload, dict) else []
    return [_entry_from_widget_provider(p) for p in providers if isinstance(p, dict)]


def successful_entries(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [dict(entry, stale=False) for entry in entries if not entry.get("error")]


def has_provider_error(entries: list[dict[str, Any]]) -> bool:
    return bool(entries) and all(bool(entry.get("error")) for entry in entries)


def merge_with_cache(entries: list[dict[str, Any]], requested: list[str], last_good_path: Path) -> list[dict[str, Any]]:
    fresh = successful_entries(entries)
    previous = load_json(last_good_path, [])
    previous_ok = {
        _cache_key(entry): entry
        for entry in previous
        if isinstance(entry, dict) and entry.get("provider") and not entry.get("error")
    }

    if fresh:
        merged_cache = dict(previous_ok)
        for entry in fresh:
            clean = dict(entry)
            clean.pop("stale", None)
            merged_cache[_cache_key(clean)] = clean
        write_json(last_good_path, list(merged_cache.values()))
        previous_ok = merged_cache

    seen_keys = {_cache_key(entry) for entry in entries if isinstance(entry, dict) and entry.get("provider")}
    seen_providers = {entry.get("provider") for entry in entries if isinstance(entry, dict) and entry.get("provider")}
    output: list[dict[str, Any]] = []
    for entry in entries:
        provider = entry.get("provider")
        key = _cache_key(entry)
        if entry.get("error") and key in previous_ok:
            output.append(dict(previous_ok[key], stale=True))
        else:
            output.append(entry)
    for wanted in requested:
        if wanted not in seen_keys and wanted not in seen_providers and wanted in previous_ok:
            output.append(dict(previous_ok[wanted], stale=True))
    return output


# --------------------------------------------------------------------------
# Presentation (kept from the original CodexBar helper)
# --------------------------------------------------------------------------


def identity_text(entry: dict[str, Any], include_email: bool = True) -> str:
    usage_obj = entry.get("usage")
    usage = usage_obj if isinstance(usage_obj, dict) else {}
    identity_obj = usage.get("identity")
    identity = identity_obj if isinstance(identity_obj, dict) else {}
    parts = []
    if include_email:
        email = identity.get("accountEmail")
        if email:
            parts.append(str(email))
    org = identity.get("accountOrganization")
    login = identity.get("loginMethod")
    if org:
        parts.append(str(org))
    if login:
        parts.append(str(login))
    return " · ".join(parts)


def window_percent(entry: dict[str, Any], key: str) -> float | None:
    usage_obj = entry.get("usage")
    usage = usage_obj if isinstance(usage_obj, dict) else {}
    window_obj = usage.get(key)
    window = window_obj if isinstance(window_obj, dict) else None
    return _percent(window.get("usedPercent")) if window else None


def max_percent(entry: dict[str, Any]) -> float:
    values = [window_percent(entry, key) for key in WINDOW_LABELS]
    best = max([value for value in values if value is not None], default=0.0)
    cli_max = entry.get("_cliMaxPercent")
    if isinstance(cli_max, (int, float)) and cli_max > best:
        return float(cli_max)
    return best


def pct_label(value: float, decimals: bool = True) -> str:
    if not decimals:
        return f"{round(value)}%"
    return f"{int(value)}%" if float(value).is_integer() else f"{value:.1f}%"


def reset_text(window: dict[str, Any] | None) -> str:
    if not isinstance(window, dict):
        return ""
    desc = str(window.get("resetDescription") or "").strip()
    if desc:
        return desc
    iso = window.get("resetsAt")
    if not iso:
        return ""
    try:
        dt = datetime.fromisoformat(str(iso).replace("Z", "+00:00")).astimezone()
    except ValueError:
        return ""
    return dt.strftime("%b %d at %H:%M %Z")


def tooltip_lines(entries: list[dict[str, Any]], decimals: bool = True) -> list[str]:
    lines: list[str] = []
    for entry in entries:
        name = provider_name(entry.get("provider"))
        account = identity_text(entry)
        if account:
            name = f"{name} ({account})"
        if entry.get("error"):
            message = entry.get("error", {}).get("message", "unknown error")
            lines.append(f"{name}: error — {message}")
            continue
        usage_obj = entry.get("usage")
        usage = usage_obj if isinstance(usage_obj, dict) else {}
        for key, label in WINDOW_LABELS.items():
            window_obj = usage.get(key)
            window = window_obj if isinstance(window_obj, dict) else None
            percent = window_percent(entry, key)
            if percent is None:
                continue
            suffix = reset_text(window)
            stale = " (stale)" if entry.get("stale") else ""
            lines.append(f"{name} {label.lower()}: {pct_label(percent, decimals)}" + (f" — {suffix}" if suffix else "") + stale)
        for extra in usage.get("extra") or []:
            if not isinstance(extra, dict):
                continue
            percent = _percent(extra.get("usedPercent"))
            if percent is None:
                continue
            suffix = str(extra.get("resetDescription") or "").strip()
            stale = " (stale)" if entry.get("stale") else ""
            lines.append(f"{name} {str(extra.get('label') or 'additional').lower()}: {pct_label(percent, decimals)}" + (f" — {suffix}" if suffix else "") + stale)
    return lines


def pin_key_for_entry(entry: dict[str, Any]) -> str:
    """Pin key of a usage entry: `provider` for the implicit default login,
    `provider/account` for a named account (e.g. `codex/work`)."""
    provider = str(entry.get("provider") or "")
    account = str(entry.get("account") or "")
    return f"{provider}/{account}" if account else provider


def _cache_key(entry: dict[str, Any]) -> str:
    """Cache key of a usage entry: the pin key, so two accounts of one
    provider no longer overwrite each other in the last-good cache."""
    return pin_key_for_entry(entry)


# State keys behind the "Session / Weekly / Monthly" bar-text checkboxes.
BAR_WINDOW_STATE_KEYS = {"primary": "barSession", "secondary": "barWeekly", "tertiary": "barMonthly"}


def enabled_bar_windows(state: dict[str, Any] | None = None) -> list[str]:
    """Slots shown in the bar text, from widget state (all on by default)."""
    sf = state or {}
    return [slot for slot in WINDOW_SLOTS if sf.get(BAR_WINDOW_STATE_KEYS[slot], "true") != "false"]


def _match_pinned(entries: list[dict[str, Any]], pinned_provider: str = "") -> dict[str, Any] | None:
    if not pinned_provider:
        return None
    pinned = next((entry for entry in entries if pin_key_for_entry(entry) == pinned_provider), None)
    if pinned is None and "/" not in pinned_provider:
        # Legacy provider-level pin: first entry of that provider.
        pinned = next((entry for entry in entries if entry.get("provider") == pinned_provider), None)
    return pinned


def bar_text(
    entries: list[dict[str, Any]],
    pinned_provider: str = "",
    decimals: bool = True,
    windows: list[str] | None = None,
) -> str:
    slots = [slot for slot in WINDOW_SLOTS if windows is None or slot in windows]
    pinned = _match_pinned(entries, pinned_provider)
    if pinned and not pinned.get("error"):
        values = [window_percent(pinned, key) for key in slots]
        values = [value for value in values if value is not None]
        if values:
            return " • ".join(pct_label(value, decimals) for value in values)
        if slots:
            return pct_label(max_percent(pinned), decimals)
        return ""
    usable = [max_percent(entry) for entry in entries if not entry.get("error")]
    if usable:
        return pct_label(max(usable), decimals)
    return "⚠"


def pinned_percent(
    entries: list[dict[str, Any]],
    pinned_provider: str = "",
    windows: list[str] | None = None,
) -> float | None:
    """Headline number for the pinned entry over the enabled windows, driving
    the bar colour/bold in QML. None when nothing is pinned or usable."""
    slots = [slot for slot in WINDOW_SLOTS if windows is None or slot in windows]
    pinned = _match_pinned(entries, pinned_provider)
    if not pinned or pinned.get("error"):
        return None
    values = [window_percent(pinned, key) for key in slots]
    values = [value for value in values if value is not None]
    if values:
        return max(values)
    if slots:
        return max_percent(pinned)
    return None


def classify(entries: list[dict[str, Any]]) -> str:
    if not entries or all(entry.get("error") for entry in entries):
        return "stale"
    pct = max([max_percent(entry) for entry in entries if not entry.get("error")], default=0.0)
    if pct >= 90:
        return "critical"
    if pct >= 70:
        return "warning"
    if any(entry.get("stale") for entry in entries):
        return "stale"
    return "ok"


def enrich_entries(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    enriched = []
    for entry in entries:
        item = dict(entry)
        provider_id = str(item.get("provider") or "")
        item["displayName"] = item.get("displayName") or provider_name(provider_id)
        item["accountText"] = identity_text(item)
        item["accountPlan"] = identity_text(item, include_email=False)
        item["maxPercent"] = max_percent(item)
        enriched.append(item)
    return enriched


def summarize(
    entries: list[dict[str, Any]],
    pinned_provider: str = "",
    decimals: bool = True,
    bar_windows: list[str] | None = None,
) -> dict[str, Any]:
    pct = max([max_percent(entry) for entry in entries if not entry.get("error")], default=0.0)
    lines = tooltip_lines(entries, decimals)
    providers = enrich_entries(entries)
    windows = [slot for slot in WINDOW_SLOTS if bar_windows is None or slot in bar_windows]
    return {
        "text": bar_text(providers, pinned_provider, decimals, windows),
        "tooltip": "\n".join(lines) if lines else "Usage Monitor: no provider data",
        "class": classify(providers),
        "percentage": pct,
        "barProvider": pinned_provider,
        "pinnedPercent": pinned_percent(providers, pinned_provider, windows),
        "providers": _sort_by_order(providers),
        "updatedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }


def connect_hint(provider_id: str) -> str:
    return CONNECT_HINTS.get(
        provider_id,
        f"Configure credentials with `usage-monitor-cli {provider_id} set api_key …`, then refresh.",
    )


# --------------------------------------------------------------------------
# Provider/account discovery for the settings view (via `list` + `show`)
# --------------------------------------------------------------------------


def parse_list() -> list[dict[str, Any]]:
    out = cli_output(["list"])
    return [provider_from_list_line(line) for line in out.splitlines() if line.strip()]


def provider_from_list_line(raw: str) -> dict[str, Any]:
    line = raw.strip()
    provider_id = line.split(None, 1)[0]
    rest = line[len(provider_id):].strip()
    state_label = "unknown"
    for candidate in ["enabled (auto)", "disabled (auto)", "enabled", "disabled"]:
        if rest.startswith(candidate):
            state_label = candidate
            rest = rest[len(candidate):].strip()
            break
    return {
        "id": provider_id,
        "displayName": rest.split(" — ", 1)[0].strip() or provider_name(provider_id),
        "enabled": state_label.startswith("enabled"),
        "state": state_label,
    }


def parse_accounts(provider_id: str) -> list[dict[str, Any]]:
    # `<provider> show` already lists the auto-detected default plus every
    # configured account, so it is a superset of `account list`.
    accounts: list[dict[str, Any]] = []
    _parse_account_lines(cli_output([provider_id, "show"]), accounts)
    return accounts


def _parse_account_lines(text: str, accounts: list[dict[str, Any]]) -> None:
    current: dict[str, Any] | None = None
    for raw in text.splitlines():
        stripped = raw.strip()
        if not stripped:
            continue
        match = re.match(r"^\[([^\]]+)\](?:\s+(.*))?$", stripped)
        if match:
            account_id = match.group(1)
            label = (match.group(2) or account_id).strip()
            auto = "auto-detected" in label
            current = next((a for a in accounts if a.get("id") == account_id), None)
            if current is None:
                current = {
                    "id": account_id,
                    "label": label,
                    "active": "true",
                    "removable": "false" if auto else "true",
                }
                accounts.append(current)
            elif auto:
                current["removable"] = "false"
        elif current is not None and stripped == "disabled":
            current["active"] = "false"


def account_text_for(accounts: list[dict[str, Any]]) -> str:
    for account in accounts:
        if account.get("active") != "false":
            return str(account.get("label") or account.get("id") or "")
    return str(accounts[0].get("label") or accounts[0].get("id") or "") if accounts else ""


def _is_implicit_default(account: dict[str, Any]) -> bool:
    return account.get("id") == "default" and "auto-detected" in str(account.get("label") or "")


def pinnable_targets(provider_id: str, display_name: str, accounts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Pin targets for the panel bar: one per active account.

    A lone auto-detected login keeps the legacy provider-level id (`codex`);
    otherwise each account gets `provider/account`, with the implicit default
    keeping the bare provider id for backward compatibility.
    """
    active = [a for a in accounts if isinstance(a, dict) and a.get("active") != "false"]
    named = [a for a in active if not _is_implicit_default(a)]
    if not named:
        return [{"id": provider_id, "displayName": display_name}]
    targets = []
    for account in active:
        if _is_implicit_default(account):
            targets.append({"id": provider_id, "displayName": display_name})
        else:
            label = str(account.get("label") or account.get("id") or "")
            targets.append(
                {
                    "id": f"{provider_id}/{account.get('id')}",
                    "displayName": f"{display_name} — {label}",
                    "providerId": provider_id,
                    "accountId": account.get("id"),
                }
            )
    return targets


def settings_payload(state_path: Path | None = None) -> dict[str, Any]:
    sf = state_full(state_path)
    items = [item for item in parse_list() if item.get("id")]
    ids = [str(item["id"]) for item in items]

    # Each provider's accounts need a separate `<provider> show` call. Run them
    # (plus the version) concurrently — subprocess.run releases the GIL while
    # waiting, so this collapses ~30 sequential spawns into one wave.
    with ThreadPoolExecutor(max_workers=16) as pool:
        accounts_iter = pool.map(parse_accounts, ids)
        version_future = pool.submit(cli_version)
        schemes_future = pool.submit(installed_schemes)
        accounts_by = dict(zip(ids, accounts_iter, strict=False))
        cli_ver = version_future.result()
        schemes = schemes_future.result()

    providers = []
    for item in items:
        provider_id = str(item["id"])
        accounts = accounts_by.get(provider_id, [])
        auth = provider_auth(provider_id)
        entry = {
            "id": provider_id,
            "displayName": item.get("displayName") or provider_name(provider_id),
            "enabled": bool(item.get("enabled")),
            "source": "auto",
            "userSource": sf.get(f"source:{provider_id}", ""),
            "availableSources": list(DEFAULT_AVAILABLE_SOURCES),
            "userAccount": sf.get(f"account:{provider_id}", ""),
            "linuxSupported": True,
            "linuxUnsupportedMessage": "",
            "accountText": account_text_for(accounts),
            "accounts": accounts,
            "connectHint": connect_hint(provider_id),
            "authKind": auth["kind"],
            "accountFields": auth["fields"],
            "setupHint": auth.get("setupHint", ""),
        }
        providers.append(entry)
    pinnable = [
        target
        for p in providers
        if p["enabled"]
        for target in pinnable_targets(
            p["id"], p["displayName"], accounts_by.get(p["id"], [])
        )
    ]
    return {
        "providers": providers,
        "pinnableProviders": pinnable,
        "pinnedProvider": sf.get("barProvider", ""),
        "refreshIntervalSeconds": int(sf.get("refreshIntervalSeconds", "30") or "30"),
        "allAccounts": sf.get("allAccounts", "true") != "false",
        "statusPages": sf.get("statusPages", "false") == "true",
        "noCredits": sf.get("noCredits", "false") == "true",
        "showBarText": sf.get("showBarText", "true") != "false",
        "showAccountEmail": sf.get("showAccountEmail", "true") != "false",
        "showDecimals": sf.get("showDecimals", "true") != "false",
        "barSession": sf.get("barSession", "true") != "false",
        "barWeekly": sf.get("barWeekly", "true") != "false",
        "barMonthly": sf.get("barMonthly", "true") != "false",
        "providerOrder": sf.get("providerOrder", "[]"),
        "theme": resolve_theme(sf, schemes),
        "themeCatalog": theme_catalog(schemes, sf),
        "themeState": {key: str(value) for key, value in sf.items() if str(key).startswith("theme")},
        "plasmoidVersion": PLASMOID_VERSION,
        "cliVersion": cli_ver,
        "update": update_info(PLASMOID_VERSION, cli_ver, sf),
        "updatedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }


def cli_version() -> str:
    proc = run_cli(["--version"], timeout=10)
    if proc.returncode == 0 and proc.stdout.strip():
        version = proc.stdout.strip()
        for prefix in ("usage-monitor-cli ", "usage-monitor "):
            if version.startswith(prefix):
                return version[len(prefix):]
        return version
    return "unknown"


# --------------------------------------------------------------------------
# Self-update: the widget compares its baked-in version with `cli --version`
# (local, no network). Changelogs come from `widget changelog`: the per-version
# release file in the repo first, GitHub Releases API next, embedded
# CHANGELOG.md fallback offline. Cached a day.
# --------------------------------------------------------------------------

UPDATE_DISMISS_KEY = "dismissedUpdateVersion"
RELEASE_CACHE_TTL_SECONDS = 24 * 3600
RELEASES_REPO = "prodesert22/UsageMonitor"


def _version_tuple(raw: Any) -> list[int]:
    parts = str(raw or "").strip().lstrip("vV= ").split(".")
    numbers: list[int] = []
    for part in parts:
        digits = ""
        for ch in part:
            if ch.isdigit():
                digits += ch
            else:
                break
        numbers.append(int(digits) if digits else 0)
    return numbers


def compare_versions(left: Any, right: Any) -> int:
    """Numeric version comparison: -1 when left < right, 0 when equal, 1 above."""
    a, b = _version_tuple(left), _version_tuple(right)
    width = max(len(a), len(b), 1)
    a += [0] * (width - len(a))
    b += [0] * (width - len(b))
    return (a > b) - (a < b)


def is_outdated(installed: Any, current: Any) -> bool:
    """True when an installed widget version is older than the CLI binary."""
    if not installed or not current or current == "unknown":
        return False
    return compare_versions(installed, current) < 0


def release_url(version: Any) -> str:
    return f"https://github.com/{RELEASES_REPO}/releases/tag/v{version}"


def update_info(widget_version: str, cli_ver: str, state: Mapping[str, Any] | None = None) -> dict[str, Any]:
    """The `update` object carried by the settings payload."""
    sf = state if isinstance(state, Mapping) else {}
    dismissed = str(sf.get(UPDATE_DISMISS_KEY, "") or "")
    outdated = is_outdated(widget_version, cli_ver)
    return {
        "installed": widget_version,
        "available": cli_ver,
        "outdated": outdated,
        "dismissed": bool(dismissed) and dismissed == cli_ver,
        "url": release_url(cli_ver) if outdated else "",
    }


def _release_cache_path(version: str) -> Path:
    # `version` reaches the filesystem: slug it so `../../` (from --version or
    # a crafted CLI reply) cannot escape the cache dir.
    slug = re.sub(r"[^A-Za-z0-9._-]", "_", version) or "unknown"
    return paths().cache_dir / f"release-{slug}.json"


def _cached_release(version: str, max_age: float = RELEASE_CACHE_TTL_SECONDS) -> dict[str, Any] | None:
    raw = load_json(_release_cache_path(version), None)
    if not isinstance(raw, dict) or not isinstance(raw.get("payload"), dict):
        return None
    try:
        stamp = datetime.fromisoformat(str(raw.get("fetchedAt", "")).replace("Z", "+00:00"))
    except ValueError:
        return None
    age = (datetime.now(timezone.utc) - stamp).total_seconds()
    if age < 0 or age > max_age:
        return None
    return raw["payload"]


def fetch_changelog(version: str, runner: Runner = run_cli) -> dict[str, Any]:
    """Release notes for `version`, cached for a day.

    Served from cache when fresh; a failed fetch falls back to a stale cache,
    and only with neither does it report `unavailable` (the UI then links to
    the release page instead).
    """
    version = str(version or "").strip().lstrip("vV")
    if not version:
        return {"version": "", "url": "", "body": "", "source": "unavailable"}
    cached = _cached_release(version)
    if cached is not None:
        return cached
    payload: dict[str, Any] | None = None
    try:
        proc = runner(["widget", "changelog", version])
    except (OSError, subprocess.SubprocessError):
        # Missing binary, timeout, denied exec: fall through to stale/unavailable.
        proc = None
    if proc is not None and proc.returncode == 0:
        try:
            parsed = json.loads(proc.stdout or "{}")
            if isinstance(parsed, dict) and parsed.get("version"):
                payload = {
                    "version": str(parsed.get("version") or version),
                    "url": str(parsed.get("url") or release_url(version)),
                    "body": str(parsed.get("body") or ""),
                    "source": str(parsed.get("source") or "unknown"),
                }
        except json.JSONDecodeError:
            payload = None
    if payload is None:
        stale = _cached_release(version, max_age=float("inf"))
        if stale is not None:
            return stale
        return {"version": version, "url": release_url(version), "body": "", "source": "unavailable"}
    write_json(_release_cache_path(version), {
        "fetchedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "payload": payload,
    })
    return payload


def apply_update(target: str = "kde", runner: Runner = run_cli) -> subprocess.CompletedProcess[str]:
    """Reinstall one widget from the current binary.

    Uses `install`, not `sync`: the banner reads the baked-in helper version
    while `sync` trusts the stamp file, so `sync` could no-op on divergence.
    """
    return runner(["widget", "install", target])


# --------------------------------------------------------------------------
# Cost (usage-monitor exposes per-provider cost in the widget payload)
# --------------------------------------------------------------------------


def cost_entries(runner: Runner = run_cli) -> list[dict[str, Any]]:
    proc = runner(["widget", "kde"])
    if proc.returncode != 0:
        return []
    try:
        payload = json.loads(proc.stdout or "{}")
    except json.JSONDecodeError:
        return []
    out = []
    for item in payload.get("providers", []) if isinstance(payload, dict) else []:
        cost = item.get("cost") if isinstance(item, dict) else None
        if isinstance(cost, dict) and cost.get("total_cost") is not None:
            out.append({"provider": str(item.get("provider_id") or ""), "last30DaysCostUSD": cost.get("total_cost")})
    return out


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------


def command_summary(args: argparse.Namespace) -> int:
    p = paths()
    entries = fetch_entries()
    requested = [_cache_key(e) for e in entries if isinstance(e, dict) and e.get("provider")]
    merged = merge_with_cache(entries, requested, p.last_good)
    sf = state_full(p.state)
    decimals = sf.get("showDecimals", "true") != "false"
    payload = summarize(
        merged,
        str(sf.get("barProvider", "") or ""),
        decimals=decimals,
        bar_windows=enabled_bar_windows(sf),
    )
    # The panel bar is themed from the summary so it does not have to wait for
    # the (much slower) settings payload.
    payload["theme"] = resolve_theme(sf)
    _dump(payload)
    return 0


def command_cache(args: argparse.Namespace) -> int:
    cached = load_json(paths().last_good, [])
    sf = state_full(paths().state)
    decimals = sf.get("showDecimals", "true") != "false"
    payload = summarize(
        cached if isinstance(cached, list) else [],
        str(sf.get("barProvider", "") or ""),
        decimals=decimals,
        bar_windows=enabled_bar_windows(sf),
    )
    payload["theme"] = resolve_theme(sf)
    _dump(payload)
    return 0


def command_themes(args: argparse.Namespace) -> int:
    schemes = installed_schemes()
    sf = state_full()
    _dump({"theme": resolve_theme(sf, schemes), "catalog": theme_catalog(schemes, sf)})
    return 0


def command_settings(args: argparse.Namespace) -> int:
    _dump(settings_payload())
    return 0


def command_state(args: argparse.Namespace) -> int:
    _dump(state_full())
    return 0


def command_set_state(args: argparse.Namespace) -> int:
    sf = state_full()
    if args.key:
        sf[args.key] = args.value
    _write_state(sf)
    _dump({"status": "ok", "key": args.key, "value": args.value})
    return 0


def command_batch_set_state(args: argparse.Namespace) -> int:
    sf = state_full()
    for key, value in json.loads(args.json):
        sf[str(key)] = value
    _write_state(sf)
    _dump({"status": "ok"})
    return 0


def command_set_provider(args: argparse.Namespace) -> int:
    enabled = args.enabled if isinstance(args.enabled, bool) else str(args.enabled).lower() == "true"
    action = "enable" if enabled else "disable"
    proc = run_cli([action, args.provider])
    if proc.returncode != 0:
        print((proc.stderr or proc.stdout or f"{action} failed").strip(), file=sys.stderr)
        return proc.returncode or 1
    return command_settings(args)


def command_cost(args: argparse.Namespace) -> int:
    payload = {
        "cost": cost_entries(),
        "updatedAt": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    }
    _dump(payload)
    return 0


def command_cache_clear(args: argparse.Namespace) -> int:
    write_json(paths().last_good, [])
    _dump({})
    return 0


def command_update_check(args: argparse.Namespace) -> int:
    _dump(update_info(PLASMOID_VERSION, cli_version(), state_full()))
    return 0


def command_update_changelog(args: argparse.Namespace) -> int:
    _dump(fetch_changelog(args.version))
    return 0


def command_update_apply(args: argparse.Namespace) -> int:
    _dump(command_result(apply_update(args.target or "kde")))
    return 0


def command_update_dismiss(args: argparse.Namespace) -> int:
    sf = state_full()
    sf[UPDATE_DISMISS_KEY] = args.version
    _write_state(sf)
    _dump({"status": "ok", "dismissed": args.version})
    return 0


def command_account_save(args: argparse.Namespace) -> int:
    """Add a named account (idempotent) and set its config keys in one call."""
    add_cmd = [args.provider, "account", "add", args.name]
    if args.label:
        add_cmd += ["--label", args.label]
    run_cli(add_cmd)  # tolerates "already exists"
    values = json.loads(args.json) if args.json else {}
    last = subprocess.CompletedProcess([], 0, "", "")
    for key, value in values.items():
        if value is None or str(value) == "":
            continue
        last = run_cli([args.provider, "account", "set", args.name, str(key), str(value)])
        if last.returncode != 0:
            print((last.stderr or last.stdout or "account set failed").strip(), file=sys.stderr)
            return last.returncode or 1
    _dump(command_result(last))
    return 0


def command_account_remove(args: argparse.Namespace) -> int:
    proc = run_cli([args.provider, "account", "remove", args.name])
    if proc.returncode != 0:
        print((proc.stderr or proc.stdout or "account remove failed").strip(), file=sys.stderr)
        return proc.returncode or 1
    _dump(command_result(proc))
    return 0


def command_result(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    return {"status": "ok" if proc.returncode == 0 else "error", "stdout": proc.stdout, "stderr": proc.stderr}


def _dump(payload: Any) -> None:
    json.dump(payload, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Usage Monitor KDE data helper")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("summary", help="Fetch providers and print the Plasma summary JSON").set_defaults(func=command_summary)
    sub.add_parser("cache", help="Print a summary from the last-good cache only").set_defaults(func=command_cache)
    sub.add_parser("settings", help="Print provider settings for the Plasma settings view").set_defaults(func=command_settings)
    sub.add_parser("state", help="Print the current state.json contents").set_defaults(func=command_state)
    sub.add_parser("cost", help="Print per-provider cost data").set_defaults(func=command_cost)
    sub.add_parser("themes", help="Print the resolved theme and the theme catalog").set_defaults(func=command_themes)
    sub.add_parser("cache-clear", help="Clear the widget last-good cache").set_defaults(func=command_cache_clear)
    sub.add_parser("update-check", help="Compare the installed widget version with the CLI version").set_defaults(func=command_update_check)
    changelog = sub.add_parser("update-changelog", help="Print the release notes for a version")
    changelog.add_argument("--version", required=True)
    changelog.set_defaults(func=command_update_changelog)
    apply = sub.add_parser("update-apply", help="Reinstall the widget from the current CLI binary")
    apply.add_argument("--target", default="kde", choices=["kde", "waybar", "all"])
    apply.set_defaults(func=command_update_apply)
    dismiss = sub.add_parser("update-dismiss", help="Hide the update banner for a version")
    dismiss.add_argument("--version", required=True)
    dismiss.set_defaults(func=command_update_dismiss)
    set_state = sub.add_parser("set-state", help="Update a key in the widget state file")
    set_state.add_argument("--key", required=True)
    set_state.add_argument("--value", required=True)
    set_state.set_defaults(func=command_set_state)
    batch = sub.add_parser("batch-set-state", help="Apply multiple state changes at once")
    batch.add_argument("--json", required=True)
    batch.set_defaults(func=command_batch_set_state)
    set_provider = sub.add_parser("set-provider", help="Enable or disable a provider")
    set_provider.add_argument("--provider", required=True)
    set_provider.add_argument("--enabled", choices=["true", "false"], required=True)
    set_provider.set_defaults(func=command_set_provider)
    account_save = sub.add_parser("account-save", help="Add a named account and set its config keys")
    account_save.add_argument("--provider", required=True)
    account_save.add_argument("--name", required=True)
    account_save.add_argument("--label", default="")
    account_save.add_argument("--json", default="{}", help="JSON object of config key/value pairs")
    account_save.set_defaults(func=command_account_save)
    account_remove = sub.add_parser("account-remove", help="Remove a named account")
    account_remove.add_argument("--provider", required=True)
    account_remove.add_argument("--name", required=True)
    account_remove.set_defaults(func=command_account_remove)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
