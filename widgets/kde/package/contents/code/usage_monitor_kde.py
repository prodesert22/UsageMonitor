#!/usr/bin/env python3
"""Data helper for the Usage Monitor KDE Plasma widget.

Ported from the CodexBar KDE helper. The Plasma UI stays simple QML; this helper
owns all JSON, CLI, cache and formatting logic so it can be tested without KDE
running. The presentation layer (summarize/tooltip/bar text/classify) is kept
from the original; only the data layer is adapted to drive `usage-monitor-cli`.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from shutil import which
from typing import Any, Callable


PLASMOID_VERSION = "0.6.0"

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
    "zai": "Z.ai",
    "elevenlabs": "ElevenLabs",
    "mistral": "Mistral",
    "cursor": "Cursor",
    "gemini": "Gemini",
}
WINDOW_LABELS = {"primary": "Session", "secondary": "Weekly", "tertiary": "Monthly"}

CONNECT_HINTS = {
    "claude": "Run `claude` and sign in, then refresh. Credentials are auto-detected.",
    "codex": "Run `codex` and sign in, then refresh. Credentials are auto-detected.",
    "anthropic": "Set an API key: `usage-monitor-cli anthropic set api_key sk-…`.",
    "openai": "Set an API key: `usage-monitor-cli openai set api_key sk-…`.",
    "gemini": "Run `gcloud auth application-default login`, then refresh.",
    "opencode-go": "Configure workspaces: `usage-monitor-cli opencode-go workspace add <id>`.",
}

# --------------------------------------------------------------------------
# Per-provider "add account" metadata for the settings UI.
#
# authKind drives the form shape:
#   api_key / token / cookie -> paste-a-secret form (name + the listed fields)
#   oauth                    -> CLI-login required; show setupHint + path fields
#   opencode                 -> token + workspace management
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
    "deepgram": {"kind": "api_key", "fields": _API_KEY + [_field("project_id", "Project ID")]},
    "llmproxy": {"kind": "api_key", "fields": _API_KEY + [_field("base_url", "Base URL", placeholder="https://…")]},
    # token providers
    **{p: {"kind": "token", "fields": _TOKEN} for p in ("grok", "kimi", "copilot", "windsurf")},
    "devin": {"kind": "token", "fields": _TOKEN + [_field("org", "Organization")]},
    # cookie providers
    **{p: {"kind": "cookie", "fields": _COOKIE} for p in ("abacus", "mistral", "ollama", "cursor", "perplexity")},
    # OAuth / CLI providers (account added via terminal login + credentials path)
    "codex": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path", placeholder="~/.codex-NAME/auth.json")], "setupHint": _OAUTH_SETUP["codex"]},
    "claude": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path", placeholder="~/.claude/.credentials.json")], "setupHint": _OAUTH_SETUP["claude"]},
    "gemini": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path"), _field("access_token", "Access token", secret=True)], "setupHint": _OAUTH_SETUP["gemini"]},
    "antigravity": {"kind": "oauth", "fields": [_field("credentials_path", "Credentials path"), _field("access_token", "Access token", secret=True)], "setupHint": _OAUTH_SETUP["antigravity"]},
    # opencode-go: cookie token + workspace management
    "opencode-go": {"kind": "opencode", "fields": [_field("token", "Session cookie", secret=True)]},
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
    slots = ["primary", "secondary", "tertiary"]
    # usage-monitor already emits windows in primary/secondary/tertiary order;
    # map positionally so the QML's fixed Session/Weekly/Monthly labels line up.
    for slot, window in zip(slots, [w for w in windows if isinstance(w, dict)]):
        usage[slot] = {
            "usedPercent": _percent(window.get("percentage")) or 0.0,
            "resetsAt": None,
            "resetDescription": str(window.get("resets_at") or ""),
        }
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
        entry.get("provider"): entry
        for entry in previous
        if isinstance(entry, dict) and entry.get("provider") and not entry.get("error")
    }

    if fresh:
        merged_cache = dict(previous_ok)
        for entry in fresh:
            clean = dict(entry)
            clean.pop("stale", None)
            merged_cache[clean.get("provider")] = clean
        write_json(last_good_path, list(merged_cache.values()))
        previous_ok = merged_cache

    seen = {entry.get("provider") for entry in entries if entry.get("provider")}
    output: list[dict[str, Any]] = []
    for entry in entries:
        provider = entry.get("provider")
        if entry.get("error") and provider in previous_ok:
            output.append(dict(previous_ok[provider], stale=True))
        else:
            output.append(entry)
    for provider in requested:
        if provider not in seen and provider in previous_ok:
            output.append(dict(previous_ok[provider], stale=True))
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


def pct_label(value: float) -> str:
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


def tooltip_lines(entries: list[dict[str, Any]]) -> list[str]:
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
            lines.append(f"{name} {label.lower()}: {pct_label(percent)}" + (f" — {suffix}" if suffix else "") + stale)
    return lines


def bar_text(entries: list[dict[str, Any]], pinned_provider: str = "") -> str:
    pinned = next((entry for entry in entries if pinned_provider and entry.get("provider") == pinned_provider), None)
    if pinned and not pinned.get("error"):
        values = [window_percent(pinned, "primary"), window_percent(pinned, "secondary")]
        values = [value for value in values if value is not None]
        if len(values) >= 2:
            return f"{pct_label(values[0])} • {pct_label(values[1])}"
        if values:
            return pct_label(values[0])
        return pct_label(max_percent(pinned))
    usable = [max_percent(entry) for entry in entries if not entry.get("error")]
    if usable:
        return pct_label(max(usable))
    return "⚠"


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


def summarize(entries: list[dict[str, Any]], pinned_provider: str = "") -> dict[str, Any]:
    pct = max([max_percent(entry) for entry in entries if not entry.get("error")], default=0.0)
    lines = tooltip_lines(entries)
    providers = enrich_entries(entries)
    return {
        "text": bar_text(providers, pinned_provider),
        "tooltip": "\n".join(lines) if lines else "Usage Monitor: no provider data",
        "class": classify(providers),
        "percentage": pct,
        "barProvider": pinned_provider,
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


def list_workspaces(account: str | None = None) -> list[dict[str, str]]:
    cmd = ["opencode-go", "workspace", "list"]
    if account:
        cmd += ["--account", account]
    workspaces: list[dict[str, str]] = []
    for raw in cli_output(cmd).splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("("):
            continue
        parts = stripped.split(None, 1)
        workspaces.append({"id": parts[0], "name": parts[1].strip() if len(parts) > 1 else ""})
    return workspaces


def settings_payload(state_path: Path | None = None) -> dict[str, Any]:
    sf = state_full(state_path)
    items = [item for item in parse_list() if item.get("id")]
    ids = [str(item["id"]) for item in items]

    # Each provider's accounts need a separate `<provider> show` call. Run them
    # (plus the workspace list and version) concurrently — subprocess.run releases
    # the GIL while waiting, so this collapses ~30 sequential spawns into one wave.
    with ThreadPoolExecutor(max_workers=16) as pool:
        accounts_iter = pool.map(parse_accounts, ids)
        version_future = pool.submit(cli_version)
        workspaces_future = pool.submit(list_workspaces) if "opencode-go" in ids else None
        accounts_by = dict(zip(ids, accounts_iter))
        cli_ver = version_future.result()
        workspaces = workspaces_future.result() if workspaces_future else []

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
        if provider_id == "opencode-go":
            entry["workspaces"] = workspaces
        providers.append(entry)
    pinnable = [
        {"id": p["id"], "displayName": p["displayName"]}
        for p in providers
        if p["enabled"]
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
        "providerOrder": sf.get("providerOrder", "[]"),
        "plasmoidVersion": PLASMOID_VERSION,
        "cliVersion": cli_version(),
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
    requested = [e.get("provider") for e in entries if e.get("provider")]
    merged = merge_with_cache(entries, requested, p.last_good)
    payload = summarize(merged, state_value(p.state))
    _dump(payload)
    return 0


def command_cache(args: argparse.Namespace) -> int:
    cached = load_json(paths().last_good, [])
    payload = summarize(cached if isinstance(cached, list) else [], state_value(paths().state))
    _dump(payload)
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


def command_workspace_add(args: argparse.Namespace) -> int:
    cmd = ["opencode-go", "workspace", "add", args.workspace]
    if args.name:
        cmd.append(args.name)
    if args.account:
        cmd += ["--account", args.account]
    proc = run_cli(cmd)
    if proc.returncode != 0:
        print((proc.stderr or proc.stdout or "workspace add failed").strip(), file=sys.stderr)
        return proc.returncode or 1
    _dump(command_result(proc))
    return 0


def command_workspace_remove(args: argparse.Namespace) -> int:
    cmd = ["opencode-go", "workspace", "remove", args.workspace]
    if args.account:
        cmd += ["--account", args.account]
    proc = run_cli(cmd)
    if proc.returncode != 0:
        print((proc.stderr or proc.stdout or "workspace remove failed").strip(), file=sys.stderr)
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
    sub.add_parser("cache-clear", help="Clear the widget last-good cache").set_defaults(func=command_cache_clear)
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
    workspace_add = sub.add_parser("workspace-add", help="Add an opencode-go workspace")
    workspace_add.add_argument("--workspace", required=True)
    workspace_add.add_argument("--name", default="")
    workspace_add.add_argument("--account", default="")
    workspace_add.set_defaults(func=command_workspace_add)
    workspace_remove = sub.add_parser("workspace-remove", help="Remove an opencode-go workspace")
    workspace_remove.add_argument("--workspace", required=True)
    workspace_remove.add_argument("--account", default="")
    workspace_remove.set_defaults(func=command_workspace_remove)
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
