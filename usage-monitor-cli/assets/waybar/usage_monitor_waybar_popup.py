#!/usr/bin/env python3
"""Qt Quick popup for the Usage Monitor Waybar module.

The QML under `ui/` is the KDE Plasma widget's interface, ported to plain Qt
Quick: same popup layout, same usage cards/bars, same settings pages and the
same theme engine — minus Plasma. Waybar draws the bar itself, so the compact
panel representation stays a Waybar module (`usage-monitor-waybar`) and this
process only provides what a Waybar module cannot: the popup window opened by
`on-click`.

Portability notes (Waybar runs on very different systems):

* Bindings: PySide6 first, PyQt6 as a fallback. Neither installed is a normal
  situation, so it fails with a per-distribution install hint instead of a
  traceback.
* Session: on X11 the popup is placed next to the pointer/bar edge by this
  process. Wayland does not let a client position its own toplevel, so there the
  compositor decides and the docs give the (one-line) Hyprland/Sway/river rules;
  `app_id`/`WM_CLASS` is `usage-monitor-waybar` for exactly that reason.
* Single instance: the first launch binds a socket under `$XDG_RUNTIME_DIR` and
  stays resident; every later `on-click` connects to it and toggles the existing
  window, which is both instant and prevents a stack of popups.
* Data: helper functions are called in-process on a worker thread (no `python3
  helper.py …` per action like in Plasma, where the QML engine had no other way
  to run code).

`--doctor` prints what was detected; every pure-python helper in this file is
unit-tested without Qt.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path
from typing import Any

_HERE = Path(__file__).resolve().parent
if str(_HERE) not in sys.path:
    sys.path.insert(0, str(_HERE))

import usage_monitor_waybar_data as data

ANCHORS = (
    "auto",
    "cursor",
    "center",
    "top",
    "bottom",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
)

DEFAULT_WIDTH = 420
DEFAULT_HEIGHT = 520
DEFAULT_MARGIN = 8

_QT_BINDINGS = ("PySide6", "PyQt6")

_DISTRO_QT_PACKAGES = {
    "arch": "sudo pacman -S pyside6",
    "manjaro": "sudo pacman -S pyside6",
    "endeavouros": "sudo pacman -S pyside6",
    "cachyos": "sudo pacman -S pyside6",
    "fedora": "sudo dnf install python3-pyside6",
    "rhel": "sudo dnf install python3-pyside6",
    "centos": "sudo dnf install python3-pyside6",
    "debian": "sudo apt install python3-pyside6.qtquick python3-pyside6.qtqml",
    "ubuntu": "sudo apt install python3-pyside6.qtquick python3-pyside6.qtqml",
    "pop": "sudo apt install python3-pyside6.qtquick python3-pyside6.qtqml",
    "linuxmint": "sudo apt install python3-pyside6.qtquick python3-pyside6.qtqml",
    "opensuse": "sudo zypper install python3-pyside6",
    "opensuse-tumbleweed": "sudo zypper install python3-pyside6",
    "nixos": "nix-shell -p python3Packages.pyside6  # or add it to your system packages",
    "void": "sudo xbps-install -S python3-pyside6",
    "alpine": "sudo apk add py3-pyside6",
    "gentoo": "sudo emerge dev-python/pyside6",
    "freebsd": "sudo pkg install py311-pyside6",
}

_GENERIC_QT_HINT = "pip install --user PySide6   # or install your distribution's PySide6/PyQt6 package"


def os_release_ids(path: Path = Path("/etc/os-release")) -> list[str]:
    """`ID` plus `ID_LIKE` from os-release, most specific first."""
    ids: list[str] = []
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ids
    values: dict[str, str] = {}
    for line in text.splitlines():
        key, _, value = line.partition("=")
        if key and value:
            values[key.strip()] = value.strip().strip('"\'')
    if values.get("ID"):
        ids.append(values["ID"].lower())
    for like in values.get("ID_LIKE", "").split():
        ids.append(like.lower())
    return ids


def qt_install_hint(ids: list[str] | None = None) -> str:
    """Install command for this distribution, or a generic one."""
    if ids is None:
        ids = os_release_ids()
        if not ids and sys.platform.startswith("freebsd"):
            ids = ["freebsd"]
    for distro_id in ids:
        command = _DISTRO_QT_PACKAGES.get(distro_id)
        if command:
            return command
    return _GENERIC_QT_HINT


def missing_qt_message(hint: str | None = None) -> str:
    return (
        "The Usage Monitor popup needs Qt for Python (PySide6, or PyQt6).\n"
        f"Install it with:\n    {hint or qt_install_hint()}\n\n"
        "The Waybar module itself keeps working without it — only the popup "
        "window (on-click) is unavailable."
    )


def available_binding(names: tuple[str, ...] = _QT_BINDINGS) -> str | None:
    """First installed Qt binding, without importing it."""
    for name in names:
        try:
            if importlib.util.find_spec(name) is not None:
                return name
        except (ImportError, ValueError):
            continue
    return None


def runtime_dir() -> Path:
    """Where the single-instance socket lives.

    `XDG_RUNTIME_DIR` is the right place and is set by systemd/elogind sessions;
    plain X sessions, some minimal WM setups and FreeBSD may not have it, hence
    the per-uid /tmp fallback (never a shared, guessable path).
    """
    raw = os.environ.get("XDG_RUNTIME_DIR")
    if raw:
        return Path(raw)
    return Path("/tmp") / f"usage-monitor-{os.getuid()}"


def socket_path(display: str | None = None) -> Path:
    """Socket name, scoped per session so two seats never collide."""
    if display is None:
        display = os.environ.get("WAYLAND_DISPLAY") or os.environ.get("DISPLAY") or ""
    suffix = "".join(ch if ch.isalnum() else "-" for ch in display).strip("-")
    name = f"usage-monitor-waybar{'-' + suffix if suffix else ''}.sock"
    return runtime_dir() / name


def clamp(value: int, low: int, high: int) -> int:
    return max(low, min(high, value))


def popup_position(
    anchor: str,
    screen: tuple[int, int, int, int],
    size: tuple[int, int],
    cursor: tuple[int, int] | None = None,
    margin: int = DEFAULT_MARGIN,
) -> tuple[int, int]:
    """Top-left corner for the popup, in screen coordinates.

    `screen` is the available geometry (x, y, w, h) — already excluding the bar
    on X11 — so the popup never lands under Waybar. "auto" follows the pointer
    when there is one (X11), and falls back to the top-right corner, which is
    where the module usually sits.
    """
    screen_x, screen_y, screen_w, screen_h = screen
    width, height = size
    max_x = screen_x + max(0, screen_w - width)
    max_y = screen_y + max(0, screen_h - height)

    if anchor == "auto":
        anchor = "cursor" if cursor else "top-right"

    if anchor == "cursor" and cursor:
        cursor_x, cursor_y = cursor
        x = cursor_x - width // 2
        # Below the pointer in the upper half of the screen, above it otherwise:
        # the bar can be at either edge.
        if cursor_y - screen_y < screen_h // 2:
            y = cursor_y + margin
        else:
            y = cursor_y - height - margin
        return clamp(int(x), screen_x, max_x), clamp(int(y), screen_y, max_y)

    center_x = screen_x + (screen_w - width) // 2
    center_y = screen_y + (screen_h - height) // 2
    positions = {
        "center": (center_x, center_y),
        "top": (center_x, screen_y + margin),
        "bottom": (center_x, max_y - margin),
        "top-left": (screen_x + margin, screen_y + margin),
        "top-right": (max_x - margin, screen_y + margin),
        "bottom-left": (screen_x + margin, max_y - margin),
        "bottom-right": (max_x - margin, max_y - margin),
    }
    x, y = positions.get(anchor, (center_x, center_y))
    return clamp(int(x), screen_x, max_x), clamp(int(y), screen_y, max_y)


# --------------------------------------------------------------------------
# Wayland placement (wlr-layer-shell)
#
# A Wayland client cannot position its own xdg-toplevel — that is why the popup
# lands wherever the compositor decides, usually centred. Bars solve this with
# the wlr-layer-shell protocol: a layer surface anchors to a screen edge and the
# compositor keeps it clear of the exclusive zone Waybar reserves, so "anchored
# top-right" *is* "right under the bar", on KWin, Hyprland, Sway, niri and every
# other wlroots-style compositor.
#
# Qt implements the protocol in LayerShellQt: the shell integration plugin turns
# the app's windows into layer surfaces, and LayerShellQt::Window carries the
# per-window anchors/margins/layer. There are no Python bindings, but the
# per-window object is a plain QObject with settable properties, so it is
# reachable through one exported factory function.
# --------------------------------------------------------------------------

# LayerShellQt::Window::Anchor
ANCHOR_TOP = 1
ANCHOR_BOTTOM = 2
ANCHOR_LEFT = 4
ANCHOR_RIGHT = 8
# LayerShellQt::Window::Layer::LayerTop
LAYER_TOP = 2
# LayerShellQt::Window::KeyboardInteractivity::KeyboardInteractivityOnDemand
KEYBOARD_ON_DEMAND = 2

_LAYER_SHELL_PLUGIN = "wayland-shell-integration/liblayer-shell.so"
_LAYER_SHELL_LIBS = ("libLayerShellQtInterface.so.6", "libLayerShellQtInterface.so")
# LayerShellQt::Window::get(QWindow*)
_LAYER_SHELL_GET = "_ZN12LayerShellQt6Window3getEP7QWindow"


def layer_shell_plugin(plugins_dir: str | Path | None) -> Path | None:
    """The layer-shell integration inside *this* Qt install, if it is there.

    It has to be the same Qt the binding loads: pip-installed PySide6 ships its
    own Qt, and asking that one for a shell integration the distribution
    installed elsewhere makes the whole Wayland platform plugin fail to load.
    """
    if not plugins_dir:
        return None
    candidate = Path(plugins_dir) / _LAYER_SHELL_PLUGIN
    return candidate if candidate.is_file() else None


def use_layer_shell(session: str, plugins_dir: str | Path | None, env: dict[str, str] | None = None) -> bool:
    """Whether to ask Qt for layer surfaces on this session."""
    env = env if env is not None else dict(os.environ)
    override = str(env.get("USAGE_MONITOR_LAYER_SHELL", "")).strip().lower()
    if override in ("0", "false", "no", "off"):
        return False
    if session != "wayland":
        return False  # X11 positions the window directly
    return layer_shell_plugin(plugins_dir) is not None


def layer_anchor_flags(anchor: str) -> int:
    """Layer-shell anchor edges for one of our placement names.

    Nothing anchored means "let the compositor centre it". The pointer-based
    modes fall back to the top-right corner: Wayland does not hand out the
    pointer position, and that corner is where the module usually sits.
    """
    return {
        "auto": ANCHOR_TOP | ANCHOR_RIGHT,
        "cursor": ANCHOR_TOP | ANCHOR_RIGHT,
        "center": 0,
        "top": ANCHOR_TOP,
        "bottom": ANCHOR_BOTTOM,
        "top-left": ANCHOR_TOP | ANCHOR_LEFT,
        "top-right": ANCHOR_TOP | ANCHOR_RIGHT,
        "bottom-left": ANCHOR_BOTTOM | ANCHOR_LEFT,
        "bottom-right": ANCHOR_BOTTOM | ANCHOR_RIGHT,
    }.get(anchor, ANCHOR_TOP | ANCHOR_RIGHT)


def _cpp_pointer(obj) -> int | None:  # pragma: no cover - needs a live Qt object
    """Address of the C++ object behind a binding wrapper."""
    try:
        from shiboken6 import getCppPointer  # type: ignore[import-not-found]

        return int(getCppPointer(obj)[0])
    except (ImportError, TypeError, RuntimeError):
        pass
    try:
        from PyQt6 import sip  # type: ignore[import-not-found]

        return int(sip.unwrapinstance(obj))
    except (ImportError, TypeError, RuntimeError):
        return None


def _wrap_qobject(qt: dict[str, Any], pointer: int):  # pragma: no cover - needs Qt
    try:
        from shiboken6 import wrapInstance  # type: ignore[import-not-found]

        return wrapInstance(pointer, qt["QObject"])
    except (ImportError, TypeError, RuntimeError):
        pass
    try:
        from PyQt6 import sip  # type: ignore[import-not-found]

        return sip.wrapinstance(pointer, qt["QObject"])
    except (ImportError, TypeError, RuntimeError):
        return None


def _layer_shell_window(qt: dict[str, Any], window):  # pragma: no cover - needs Qt
    """LayerShellQt::Window for a QWindow, or None when unavailable."""
    import ctypes

    pointer = _cpp_pointer(window)
    if not pointer:
        return None
    for name in _LAYER_SHELL_LIBS:
        try:
            lib = ctypes.CDLL(name)
        except OSError:
            continue
        try:
            get = getattr(lib, _LAYER_SHELL_GET)
        except AttributeError:
            continue
        get.restype = ctypes.c_void_p
        get.argtypes = [ctypes.c_void_p]
        handle = get(ctypes.c_void_p(pointer))
        if handle:
            return _wrap_qobject(qt, int(handle))
    return None


def effective_anchor(cli_anchor: str, stored_anchor: str) -> str:
    """`--anchor` wins; otherwise the Settings → General choice; else "auto"."""
    if cli_anchor and cli_anchor != "auto" and cli_anchor in ANCHORS:
        return cli_anchor
    if stored_anchor in ANCHORS:
        return stored_anchor
    return "auto"


def palette_roles(theme: dict[str, Any]) -> dict[str, str]:
    """Qt palette roles for a resolved theme.

    Qt Quick Controls' Basic style ignores the palette and hardcodes a light
    look, which would render dark themes unreadable; the Fusion style follows
    the palette, so the popup ships its own instead of inheriting whatever
    (often nonexistent) platform theme the compositor session provides.
    """
    colors = theme.get("colors") if isinstance(theme.get("colors"), dict) else {}
    background = colors.get("background", "#1c1c1e")
    text = colors.get("text", "#f5f5f7")
    subtext = colors.get("subtext", "#98989d")
    accent = colors.get("accent", "#0a84ff")
    track = colors.get("track", "#3a3a3c")
    return {
        "Window": background,
        "WindowText": text,
        "Base": track,
        "AlternateBase": background,
        "Text": text,
        "Button": track,
        "ButtonText": text,
        "Highlight": accent,
        "HighlightedText": background,
        "PlaceholderText": subtext,
        "ToolTipBase": background,
        "ToolTipText": text,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="usage-monitor-waybar-popup",
        description="Usage Monitor popup window for the Waybar module",
    )
    parser.add_argument(
        "--anchor",
        choices=ANCHORS,
        default="auto",
        help="where to place the popup on X11 (Wayland leaves placement to the compositor)",
    )
    parser.add_argument("--width", type=int, default=DEFAULT_WIDTH)
    parser.add_argument("--height", type=int, default=DEFAULT_HEIGHT)
    parser.add_argument("--margin", type=int, default=DEFAULT_MARGIN, help="gap from the screen edge/pointer, in px")
    parser.add_argument(
        "--once",
        action="store_true",
        help="quit when the popup closes instead of staying resident for the next click",
    )
    parser.add_argument(
        "--no-single-instance",
        action="store_true",
        help="skip the toggle socket and always open a new window",
    )
    parser.add_argument("--settings", action="store_true", help="open the settings window straight away")
    parser.add_argument("--quit", action="store_true", help="tell a resident popup to exit")
    parser.add_argument("--doctor", action="store_true", help="print what was detected and exit")
    return parser


def _qt_plugins_dir(binding: str | None) -> str:
    """Plugin directory of the Qt the binding actually loads."""
    if binding is None:
        return ""
    try:
        qt = _load_qt(binding)
        return str(qt["QLibraryInfo"].path(qt["QLibraryInfo"].LibraryPath.PluginsPath))
    except (ImportError, AttributeError, OSError):
        return ""


def doctor_payload(args: argparse.Namespace | None = None) -> dict[str, Any]:
    binding = available_binding()
    plugins_dir = _qt_plugins_dir(binding)
    session = data.session_type()
    p = data.paths()
    return {
        "popupVersion": data.POPUP_VERSION,
        "python": sys.executable,
        "qtBinding": binding or "missing",
        "qtInstallHint": "" if binding else qt_install_hint(),
        "qmlDir": str(_HERE / "ui"),
        "qtPluginsDir": plugins_dir,
        # False here is why a popup opens centred instead of under the bar.
        "layerShell": use_layer_shell(session, plugins_dir),
        "socket": str(socket_path()),
        "desktop": data.desktop_environment(),
        "sessionType": session,
        "anchor": effective_anchor(
            str(getattr(args, "anchor", "auto")),
            str(data.state_value(key="popupAnchor", default="auto")),
        ),
        "binary": data.usage_monitor_binary(),
        "state": str(p.state),
        "cache": str(p.last_good),
        "cacheAgeSeconds": data.cache_age_seconds(),
        "minFetchIntervalSeconds": data.min_fetch_interval(),
        "theme": data.resolve_theme()["name"],
    }


# --------------------------------------------------------------------------
# Qt layer — imported only when a binding exists, so the helpers above (and the
# tests) stay importable on a machine without Qt.
# --------------------------------------------------------------------------


def _load_qt(binding: str):
    """Return the handful of Qt symbols used here, normalized across bindings."""
    if binding == "PySide6":
        from PySide6.QtCore import (  # type: ignore[import-not-found]
            Property,
            QLibraryInfo,
            QMargins,
            QObject,
            QRunnable,
            QThreadPool,
            QUrl,
            Signal,
            Slot,
        )
        from PySide6.QtGui import (  # type: ignore[import-not-found]
            QColor,
            QCursor,
            QGuiApplication,
            QIcon,
            QPalette,
        )
        from PySide6.QtNetwork import QLocalServer, QLocalSocket  # type: ignore[import-not-found]
        from PySide6.QtQml import QQmlApplicationEngine  # type: ignore[import-not-found]
        from PySide6.QtQuickControls2 import QQuickStyle  # type: ignore[import-not-found]
    else:  # PyQt6
        from PyQt6.QtCore import (  # type: ignore[import-not-found]
            QLibraryInfo,
            QMargins,
            QObject,
            QRunnable,
            QThreadPool,
            QUrl,
        )
        from PyQt6.QtCore import pyqtProperty as Property  # type: ignore[import-not-found]
        from PyQt6.QtCore import pyqtSignal as Signal  # type: ignore[import-not-found]
        from PyQt6.QtCore import pyqtSlot as Slot  # type: ignore[import-not-found]
        from PyQt6.QtGui import (  # type: ignore[import-not-found]
            QColor,
            QCursor,
            QGuiApplication,
            QIcon,
            QPalette,
        )
        from PyQt6.QtNetwork import QLocalServer, QLocalSocket  # type: ignore[import-not-found]
        from PyQt6.QtQml import QQmlApplicationEngine  # type: ignore[import-not-found]
        from PyQt6.QtQuickControls2 import QQuickStyle  # type: ignore[import-not-found]
    return {
        "Property": Property,
        "QLibraryInfo": QLibraryInfo,
        "QMargins": QMargins,
        "QObject": QObject,
        "QThreadPool": QThreadPool,
        "QRunnable": QRunnable,
        "QUrl": QUrl,
        "Signal": Signal,
        "Slot": Slot,
        "QColor": QColor,
        "QCursor": QCursor,
        "QGuiApplication": QGuiApplication,
        "QIcon": QIcon,
        "QPalette": QPalette,
        "QLocalServer": QLocalServer,
        "QLocalSocket": QLocalSocket,
        "QQmlApplicationEngine": QQmlApplicationEngine,
        "QQuickStyle": QQuickStyle,
    }


def _build_backend_class(qt: dict[str, Any]):
    QObject = qt["QObject"]
    Property = qt["Property"]
    Signal = qt["Signal"]
    Slot = qt["Slot"]
    QRunnable = qt["QRunnable"]

    class _Job(QRunnable):
        """One helper call on the thread pool; the result comes back as a signal."""

        def __init__(self, backend, kind: str, work) -> None:
            super().__init__()
            self._backend = backend
            self._kind = kind
            self._work = work

        def run(self) -> None:  # pragma: no cover - needs a Qt event loop
            try:
                payload = self._work()
                self._backend.jobFinished.emit(self._kind, json.dumps(payload, ensure_ascii=False), "")
            except Exception as exc:  # noqa: BLE001 - a helper crash must not kill the popup
                self._backend.jobFinished.emit(self._kind, "", f"{type(exc).__name__}: {exc}")

    class Backend(QObject):
        """QML-facing data layer.

        Payloads cross into QML as JSON strings on purpose: PySide6 and PyQt6
        convert Python containers to QML differently, and `JSON.parse` in QML
        behaves identically on both — the same contract the Plasma widget has
        with its helper process.
        """

        summaryJsonChanged = Signal()
        settingsJsonChanged = Signal()
        costJsonChanged = Signal()
        updateNotesChanged = Signal()
        updateResultChanged = Signal()
        busyChanged = Signal()
        errorChanged = Signal()
        jobFinished = Signal(str, str, str)
        # Emitted for the `--settings` launch and the "settings" socket command;
        # the QML connects to it to raise its settings window.
        settingsRequested = Signal()

        def __init__(self, ui_dir: Path, layer: dict[str, Any] | None = None, parent=None) -> None:
            super().__init__(parent)
            self._ui_dir = ui_dir
            self._layer = layer or {"enabled": False, "anchor": "auto", "margin": DEFAULT_MARGIN}
            self._summary = json.dumps({
                "text": "--", "tooltip": "Usage Monitor is loading…", "class": "stale",
                "percentage": 0, "providers": [], "theme": data.resolve_theme(),
            })
            self._settings = json.dumps({
                "providers": [], "pinnableProviders": [], "pinnedProvider": "",
                "refreshIntervalSeconds": 30, "showBarText": True, "showAccountEmail": True,
                "providerOrder": "[]", "theme": data.resolve_theme(),
                "themeCatalog": {"builtin": [], "schemes": [], "custom": []},
                "themeState": {}, "popupVersion": data.POPUP_VERSION, "cliVersion": "",
            })
            self._cost = json.dumps({"cost": [], "updatedAt": ""})
            self._update_notes = json.dumps({"version": "", "url": "", "body": "", "source": ""})
            self._update_result = json.dumps({"status": ""})
            self._busy = False
            self._pending = 0
            self._error_text = ""
            self._error_details = ""
            self.jobFinished.connect(self._on_job_finished)

        # ---- properties ------------------------------------------------

        def _get_summary(self) -> str:
            return self._summary

        def _get_settings(self) -> str:
            return self._settings

        def _get_cost(self) -> str:
            return self._cost

        def _get_update_notes(self) -> str:
            return self._update_notes

        def _get_update_result(self) -> str:
            return self._update_result

        def _get_busy(self) -> bool:
            return self._busy

        def _get_error_text(self) -> str:
            return self._error_text

        def _get_error_details(self) -> str:
            return self._error_details

        def _get_ui_dir(self) -> str:
            return self._ui_dir.as_uri()

        def _get_version(self) -> str:
            return data.POPUP_VERSION

        def _get_session_type(self) -> str:
            return data.session_type()

        def _get_desktop(self) -> str:
            return data.desktop_environment()

        summaryJson = Property(str, _get_summary, notify=summaryJsonChanged)
        settingsJson = Property(str, _get_settings, notify=settingsJsonChanged)
        costJson = Property(str, _get_cost, notify=costJsonChanged)
        updateNotesJson = Property(str, _get_update_notes, notify=updateNotesChanged)
        updateResultJson = Property(str, _get_update_result, notify=updateResultChanged)
        busy = Property(bool, _get_busy, notify=busyChanged)
        errorText = Property(str, _get_error_text, notify=errorChanged)
        errorDetails = Property(str, _get_error_details, notify=errorChanged)
        uiDir = Property(str, _get_ui_dir, constant=True)
        version = Property(str, _get_version, constant=True)
        sessionType = Property(str, _get_session_type, constant=True)
        desktop = Property(str, _get_desktop, constant=True)

        # ---- job plumbing ----------------------------------------------

        def _start(self, kind: str, work) -> None:
            self._pending += 1
            self._set_busy(True)
            qt["QThreadPool"].globalInstance().start(_Job(self, kind, work))

        def _set_busy(self, value: bool) -> None:
            if self._busy != value:
                self._busy = value
                self.busyChanged.emit()

        def _set_error(self, text: str, details: str = "") -> None:
            if (text, details) != (self._error_text, self._error_details):
                self._error_text = text
                self._error_details = details
                self.errorChanged.emit()

        def _on_job_finished(self, kind: str, payload: str, error: str) -> None:
            self._pending = max(0, self._pending - 1)
            if self._pending == 0:
                self._set_busy(False)
            if error:
                self._set_error("An error occurred.", error)
                return
            if kind in ("summary", "cache"):
                self._summary = payload
                self.summaryJsonChanged.emit()
            elif kind == "settings":
                self._settings = payload
                self.settingsJsonChanged.emit()
            elif kind == "cost":
                self._cost = payload
                self.costJsonChanged.emit()
            elif kind == "update-notes":
                # Release notes for the update banner; invalid JSON keeps the
                # previous notes so the banner links the release page instead.
                try:
                    notes = json.loads(payload or "{}")
                    if isinstance(notes, dict) and notes.get("version"):
                        self._update_notes = payload
                        self.updateNotesChanged.emit()
                except json.JSONDecodeError:
                    pass
            elif kind in ("mutation", "update-mutation"):
                if kind == "update-mutation":
                    # Kept for the banner's ok/error message (KDE parity).
                    try:
                        stored = json.loads(payload or "{}")
                        if isinstance(stored, dict) and stored.get("status"):
                            self._update_result = payload
                            self.updateResultChanged.emit()
                    except json.JSONDecodeError:
                        pass
                # A write (provider toggle, account, state) only reports the CLI
                # status; reload the settings so the UI reflects what landed.
                result = json.loads(payload or "{}")
                if result.get("status") == "error":
                    self._set_error("An error occurred.", result.get("stderr") or result.get("stdout") or "")
                    return
                self.loadSettings()
            self._set_error("", "")

        # ---- slots called from QML -------------------------------------

        @Slot()
        def refresh(self) -> None:
            """Periodic refresh: reuses the shared cache inside the fetch interval."""
            self._start("summary", data.summary_payload)

        @Slot()
        def forceRefresh(self) -> None:
            """The Refresh button: an explicit user action always hits providers."""
            self._start("summary", lambda: data.summary_payload(force=True))

        @Slot()
        def loadCache(self) -> None:
            self._start("cache", data.cache_payload)

        @Slot()
        def loadSettings(self) -> None:
            self._start("settings", data.settings_payload)

        @Slot()
        def fetchCost(self) -> None:
            self._start("cost", data.cost_payload)

        @Slot(str)
        def updateChangelog(self, version: str) -> None:
            """Release notes for the update banner (GitHub, cached, offline-safe)."""
            self._start("update-notes", lambda: data.fetch_changelog(version))

        @Slot()
        def applyUpdate(self) -> None:
            """Reinstall the widget from the current binary (same as `install`)."""
            self._start("update-mutation", lambda: _result(data.apply_update("waybar")))

        @Slot(str)
        def dismissUpdate(self, version: str) -> None:
            def work() -> dict[str, Any]:
                data.set_state_key(data.UPDATE_DISMISS_KEY, version)
                return {"status": "ok"}

            self._start("mutation", work)

        @Slot(str, str)
        def saveStateKey(self, key: str, value: str) -> None:
            def work() -> dict[str, Any]:
                data.set_state_key(key, value)
                return {"status": "ok"}

            self._start("mutation", work)

        @Slot(str)
        def batchSetState(self, pairs_json: str) -> None:
            def work() -> dict[str, Any]:
                pairs = json.loads(pairs_json or "[]")
                data.set_state_keys([(str(k), v) for k, v in pairs])
                return {"status": "ok"}

            self._start("mutation", work)

        @Slot(str, bool)
        def setProviderEnabled(self, provider_id: str, enabled: bool) -> None:
            self._start("mutation", lambda: _result(data.set_provider_enabled(provider_id, enabled)))

        @Slot(str, str, str, str)
        def accountSave(self, provider_id: str, name: str, label: str, fields_json: str) -> None:
            def work() -> dict[str, Any]:
                values = json.loads(fields_json or "{}")
                return _result(data.account_save(provider_id, name, label, values))

            self._start("mutation", work)

        @Slot(str, str)
        def accountRemove(self, provider_id: str, name: str) -> None:
            self._start("mutation", lambda: _result(data.account_remove(provider_id, name)))

        @Slot()
        def cacheClear(self) -> None:
            def work() -> dict[str, Any]:
                data.cache_clear()
                return {"status": "ok"}

            self._start("mutation", work)

        @Slot(str)
        def copyToClipboard(self, text: str) -> None:
            qt["QGuiApplication"].clipboard().setText(text)

        @Slot(qt["QObject"], str)
        def applyLayerShell(self, window, role: str) -> None:
            """Anchor a window as a Wayland layer surface.

            Called from each window's Component.onCompleted, before it is shown:
            LayerShellQt reads these properties when the surface is created. A
            no-op on X11 or without the integration plugin, where the launcher
            positions the window itself.
            """
            if not self._layer.get("enabled"):
                return
            layer_window = _layer_shell_window(qt, window)
            if layer_window is None:
                return
            margin = int(self._layer.get("margin", DEFAULT_MARGIN))
            if role == "settings":
                # The settings window is a dialog, not a bar popup: unanchored,
                # so the compositor centres it.
                anchors = 0
                margins = qt["QMargins"](0, 0, 0, 0)
            else:
                anchors = layer_anchor_flags(str(self._layer.get("anchor", "auto")))
                margins = qt["QMargins"](margin, margin, margin, margin)
            layer_window.setProperty("anchors", anchors)
            layer_window.setProperty("layer", LAYER_TOP)
            layer_window.setProperty("margins", margins)
            # OnDemand keeps the text fields in Settings usable while still
            # letting clicks reach the windows underneath.
            layer_window.setProperty("keyboardInteractivity", KEYBOARD_ON_DEMAND)
            layer_window.setProperty("exclusionZone", 0)
            layer_window.setProperty("scope", "usage-monitor")

        @Slot()
        def quitApp(self) -> None:
            qt["QGuiApplication"].instance().quit()

    return Backend


def _result(proc) -> dict[str, Any]:
    return {
        "status": "ok" if proc.returncode == 0 else "error",
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }


def _send_command(qt: dict[str, Any], path: Path, command: str, timeout_ms: int = 500) -> bool:
    """Hand a command to an already-running popup; False when none is listening."""
    socket = qt["QLocalSocket"]()
    socket.connectToServer(str(path))
    if not socket.waitForConnected(timeout_ms):
        return False
    socket.write(command.encode("utf-8"))
    socket.flush()
    socket.waitForBytesWritten(timeout_ms)
    socket.disconnectFromServer()
    return True


def run_app(args: argparse.Namespace) -> int:  # pragma: no cover - needs Qt + a display
    binding = available_binding()
    if binding is None:
        print(missing_qt_message(), file=sys.stderr)
        return 1
    qt = _load_qt(binding)

    session = data.session_type()
    stored_anchor = str(data.state_value(key="popupAnchor", default="auto"))
    anchor = effective_anchor(args.anchor, stored_anchor)
    plugins_dir = qt["QLibraryInfo"].path(qt["QLibraryInfo"].LibraryPath.PluginsPath)
    layer = {
        "enabled": use_layer_shell(session, plugins_dir),
        "anchor": anchor,
        "margin": args.margin,
    }
    if layer["enabled"]:
        # Must be set before the application is constructed: Qt picks the shell
        # integration when it initialises the Wayland platform.
        os.environ["QT_WAYLAND_SHELL_INTEGRATION"] = "layer-shell"

    QGuiApplication = qt["QGuiApplication"]
    QGuiApplication.setApplicationName("usage-monitor-waybar")
    QGuiApplication.setOrganizationName("usage-monitor")
    # app_id on Wayland / WM_CLASS on X11: what compositor float+position rules
    # match on (see docs/widgets/waybar.md).
    QGuiApplication.setDesktopFileName("usage-monitor-waybar")
    # Fusion follows the application palette; the default Basic style would paint
    # a fixed light chrome around a dark themed popup.
    qt["QQuickStyle"].setStyle("Fusion")
    # Created before the single-instance handshake: QLocalSocket needs a running
    # Qt application object for its event dispatcher.
    app = QGuiApplication(sys.argv[:1])

    path = socket_path()
    if not args.no_single_instance:
        command = "quit" if args.quit else ("settings" if args.settings else "toggle")
        if _send_command(qt, path, command):
            return 0
        if args.quit:
            return 0  # nothing running: already "quit"

    icon_path = _HERE / "ui" / "images" / "usage-monitor.png"
    if icon_path.is_file():
        app.setWindowIcon(qt["QIcon"](str(icon_path)))

    def apply_palette(theme: dict[str, Any]) -> None:
        palette = qt["QPalette"]()
        QColor = qt["QColor"]
        for role_name, value in palette_roles(theme).items():
            role = getattr(qt["QPalette"].ColorRole, role_name, None)
            if role is not None:
                palette.setColor(role, QColor(value))
        app.setPalette(palette)

    apply_palette(data.resolve_theme())

    Backend = _build_backend_class(qt)
    # Parented to the application so it outlives the QML engine (see the
    # teardown note at the end of this function).
    backend = Backend(_HERE / "ui", layer, app)

    def on_summary_changed() -> None:
        theme = json.loads(backend.summaryJson).get("theme")
        if isinstance(theme, dict):
            apply_palette(theme)

    backend.summaryJsonChanged.connect(on_summary_changed)

    engine = qt["QQmlApplicationEngine"]()
    engine.rootContext().setContextProperty("backend", backend)
    engine.load(qt["QUrl"].fromLocalFile(str(_HERE / "ui" / "Popup.qml")))
    roots = engine.rootObjects()
    if not roots:
        print("Failed to load the popup QML. Run with QT_LOGGING_RULES='qt.qml*=true' for details.", file=sys.stderr)
        return 1
    # The root object is driven through its QML properties rather than
    # QQuickWindow methods: PySide6 and PyQt6 downcast `rootObjects()` entries
    # differently, and x/y/width/height/visible exist on every binding.
    window = roots[0]
    window.setProperty("residentMode", not args.once)
    if args.width > 0:
        window.setProperty("width", args.width)
    if args.height > 0:
        window.setProperty("height", args.height)

    def place_window() -> None:
        """Position the popup on X11.

        On Wayland the surface is anchored by the compositor: as a layer surface
        when the integration is available (below the bar), otherwise wherever
        the compositor decides — a client cannot move its own toplevel there.
        """
        if session == "wayland":
            return
        cursor = qt["QCursor"].pos()
        screen = app.screenAt(cursor) or app.primaryScreen()
        if screen is None:
            return
        available = screen.availableGeometry()
        x, y = popup_position(
            anchor,
            (available.x(), available.y(), available.width(), available.height()),
            (int(window.property("width")), int(window.property("height"))),
            (cursor.x(), cursor.y()),
            args.margin,
        )
        window.setProperty("x", x)
        window.setProperty("y", y)

    def show_popup() -> None:
        place_window()
        window.setProperty("visible", True)
        if hasattr(window, "raise_"):
            window.raise_()
        if hasattr(window, "requestActivate"):
            window.requestActivate()

    def hide_popup() -> None:
        window.setProperty("visible", False)

    def toggle_popup() -> None:
        if window.property("visible"):
            hide_popup()
        else:
            backend.refresh()
            backend.loadSettings()
            show_popup()

    server = None
    if not args.no_single_instance:
        server = qt["QLocalServer"]()
        # A crashed instance leaves the socket file behind; removing a stale one
        # is the documented way to rebind.
        qt["QLocalServer"].removeServer(str(path))
        path.parent.mkdir(parents=True, exist_ok=True)
        server.listen(str(path))

        def on_new_connection() -> None:
            connection = server.nextPendingConnection()
            if connection is None:
                return

            def on_ready() -> None:
                command = bytes(connection.readAll()).decode("utf-8", "replace").strip()
                if command == "toggle":
                    toggle_popup()
                elif command == "show":
                    backend.refresh()
                    show_popup()
                elif command == "hide":
                    hide_popup()
                elif command == "settings":
                    backend.loadSettings()
                    show_popup()
                    backend.settingsRequested.emit()
                elif command == "refresh":
                    backend.refresh()
                elif command == "quit":
                    app.quit()

            connection.readyRead.connect(on_ready)

        server.newConnection.connect(on_new_connection)

    backend.loadCache()
    backend.refresh()
    backend.loadSettings()
    if args.settings:
        backend.settingsRequested.emit()
    show_popup()

    try:
        return app.exec()
    finally:
        if server is not None:
            server.close()
            qt["QLocalServer"].removeServer(str(path))
        # Tear the QML engine down before the backend it binds to. In the other
        # order every binding that reads `backend.…` is re-evaluated against a
        # destroyed object and Waybar's log fills with "Cannot read property
        # 'summaryJson' of null" on each exit.
        roots.clear()
        del engine


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.doctor:
        json.dump(doctor_payload(args), sys.stdout, ensure_ascii=False, indent=2)
        sys.stdout.write("\n")
        return 0
    return run_app(args)


if __name__ == "__main__":
    raise SystemExit(main())
