/* Usage Monitor GNOME Shell extension (GNOME 45+, ESM).
 *
 * Top-bar indicator backed by the `usage-monitor-cli` binary: it spawns
 * `usage-monitor-cli widget gnome` (or `widget kde` with older binaries),
 * keeps the last-good payload as a stale fallback, and renders one card per
 * provider/account in the popup.
 *
 * When the CLI is missing (e.g. extension installed from extensions.gnome.org
 * without the binary), the panel shows "--" and the popup explains how to
 * install it instead of failing.
 */

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';
import { panelPercentages, windowList } from './panel_values.js';
import { resolveTheme } from './theme.js';

Gio._promisify(Gio.Subprocess.prototype, 'communicate_utf8_async');

function cliBin() {
    const override = GLib.getenv('USAGE_MONITOR_BIN');
    if (override) return override;
    for (const path of [
        GLib.build_filenamev([GLib.get_home_dir(), '.cargo', 'bin', 'usage-monitor-cli']),
        GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'usage-monitor-cli']),
    ]) {
        if (GLib.file_test(path, GLib.FileTest.IS_EXECUTABLE)) return path;
    }
    const onPath = GLib.find_program_in_path('usage-monitor-cli');
    if (onPath) return onPath;
    return 'usage-monitor-cli';
}

function cliFailureHint(message) {
    if (/not found|No such file|G_IO_ERROR_NOT_FOUND/i.test(message))
        return 'usage-monitor-cli was not found. Install the CLI to load live usage.';
    if (unsupportedGnomeCommand(message))
        return 'The installed usage-monitor-cli is too old for the GNOME widget. Reinstall the CLI to load live usage.';
    if (/unrecognized subcommand.*kde|unexpected argument.*kde/i.test(message))
        return 'The installed usage-monitor-cli has no widget data command. Update it with `cargo install --path usage-monitor-cli --force`.';
    return 'Could not refresh usage. Check the CLI in a terminal, then refresh again.';
}

function unsupportedGnomeCommand(message) {
    return /unrecognized subcommand.*gnome|unexpected argument.*gnome/i.test(message);
}

function levelFor(pct) {
    if (pct >= 90) return 'critical';
    if (pct >= 70) return 'warning';
    return 'ok';
}

function pctLabel(value, showDecimals) {
    const v = Number(value) || 0;
    if (showDecimals === false) return `${Math.round(v)}%`;
    return Number.isInteger(v) ? `${v}%` : `${v.toFixed(1)}%`;
}

function pinKey(entry) {
    if (!entry) return '';
    if (entry.account_id) return `${entry.provider_id}/${entry.account_id}`;
    return entry.provider_id || '';
}

function accountText(entry) {
    if (entry.account_email) return entry.account_email;
    if (entry.account_label) return `${entry.display_name} — ${entry.account_label}`;
    if (entry.plan) return entry.plan;
    return '';
}

// Same "$X.XX (30d)" label the KDE card shows for its cost entries.
function costLabel(cost) {
    if (!cost || typeof cost !== 'object') return '';
    const total = Number(cost.total_cost);
    if (!Number.isFinite(total)) return '';
    const cur = cost.currency ? `${cost.currency} ` : '';
    return `${cur}${total.toFixed(2)} (30d)`;
}

function rgba(color, opacity = 1) {
    if (typeof color === 'string') {
        const hex = color.slice(1);
        if (/^[0-9a-f]{3}$/i.test(hex)) {
            const [r, g, b] = [...hex].map(ch => parseInt(ch + ch, 16));
            return 'rgba(' + r + ', ' + g + ', ' + b + ', ' + opacity + ')';
        }
        if (/^[0-9a-f]{6}$/i.test(hex)) {
            return 'rgba(' + parseInt(hex.slice(0, 2), 16) + ', ' +
                parseInt(hex.slice(2, 4), 16) + ', ' +
                parseInt(hex.slice(4, 6), 16) + ', ' + opacity + ')';
        }
    }
    if (color && Number.isFinite(color.red)) {
        return 'rgba(' + color.red + ', ' + color.green + ', ' +
            color.blue + ', ' + opacity + ')';
    }
    return '';
}

function cacheFile() {
    const dir = GLib.build_filenamev([GLib.get_user_cache_dir(), 'usage-monitor-gnome']);
    GLib.mkdir_with_parents(dir, 0o755);
    return Gio.File.new_for_path(GLib.build_filenamev([dir, 'last.json']));
}

function readCache() {
    try {
        const [, bytes] = cacheFile().load_contents(null);
        const payload = JSON.parse(new TextDecoder().decode(bytes));
        payload._stale = true;
        return payload;
    } catch {
        return null;
    }
}

function writeCache(payload) {
    try {
        cacheFile().replace_contents(JSON.stringify(payload), null, false,
            Gio.FileCreateFlags.REPLACE_DESTINATION, null);
    } catch {
        /* cache is best-effort */
    }
}

async function spawnCli(argv) {
    const proc = Gio.Subprocess.new(argv,
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
    const [stdout, stderr] = await proc.communicate_utf8_async(null, null);
    const status = proc.get_exit_status();
    if (status !== 0)
        throw new Error((stderr || stdout || `exit ${status}`).trim());
    return stdout;
}

// GObject subclasses must go through registerClass: instantiating a plain
// JS subclass of a GObject class throws "Tried to construct an object
// without a GType" (the ButtonBox/PanelMenuButton frames in the journal).
const UsageMonitorIndicator = GObject.registerClass(
class UsageMonitorIndicator extends PanelMenu.Button {
    _init(ext) {
        super._init(0.0, 'Usage Monitor', false);
        this._ext = ext;
        this._settings = ext.getSettings();
        this._cli = cliBin();
        this._widgetTarget = 'gnome';
        this._summary = null;
        this._busy = false;
        this._missingCli = false;
        this._fetchError = '';

        const box = new St.BoxLayout({
            style_class: 'um-panel-box',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._icon = new St.Icon({
            gicon: Gio.FileIcon.new(Gio.File.new_for_path(
                GLib.build_filenamev([ext.path, 'icons', 'usage-monitor.png']))),
            style_class: 'um-panel-icon',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._label = new St.Label({
            text: '--',
            style_class: 'um-panel-text',
            y_align: Clutter.ActorAlign.CENTER,
        });
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);
        this.menu.box.add_style_class_name('um-popup');

        this._settingsChangedId = this._settings.connect('changed', () => {
            this._renderPanel();
            this._rebuildPopup();
            this._restartTimer();
        });
        this._timerId = 0;
        this._restartTimer();
        this.refresh();
    }

    _intervalSeconds() {
        return Math.max(10, this._settings.get_int('refresh-interval') || 30);
    }

    _theme() {
        let custom = {};
        try {
            custom = JSON.parse(this._settings.get_string('theme-custom') || '{}');
        } catch {
            custom = {};
        }
        return resolveTheme(
            this._settings.get_string('theme-mode'),
            this._settings.get_string('theme-builtin'),
            custom,
            this._settings.get_double('theme-opacity'),
            this._settings.get_int('bar-height'),
            this._settings.get_int('corner-radius'));
    }

    _setStyle(actor, rules) {
        actor.set_style(rules.filter(Boolean).join(' '));
    }

    _popupBackground(theme) {
        let color = theme.colors?.background;
        if (!color) {
            try {
                color = this.menu.box.get_theme_node().get_background_color();
            } catch {
                return '';
            }
        }
        return rgba(color, theme.opacity);
    }

    _applyPopupTheme(theme) {
        const colors = theme.colors;
        const background = this._popupBackground(theme);
        this._setStyle(this.menu.box, [
            background && 'background-color: ' + background + ';',
            colors && 'border: 1px solid ' + colors.border + ';',
            'border-radius: ' + theme.cornerRadius + 'px;',
        ]);
    }

    _restartTimer() {
        if (this._timerId) {
            GLib.Source.remove(this._timerId);
            this._timerId = 0;
        }
        this._timerId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT,
            this._intervalSeconds(), () => {
                this.refresh();
                return GLib.SOURCE_CONTINUE;
            });
    }

    async refresh() {
        if (this._busy) return;
        this._busy = true;
        this._renderBusy();
        try {
            let out;
            try {
                out = await spawnCli([this._cli, 'widget', this._widgetTarget]);
            } catch (e) {
                if (this._widgetTarget !== 'gnome' ||
                    !unsupportedGnomeCommand(String(e && e.message || e))) throw e;
                // The earlier CLI exposes the same widget JSON as `widget kde`.
                out = await spawnCli([this._cli, 'widget', 'kde']);
                this._widgetTarget = 'kde';
            }
            const payload = JSON.parse(out);
            this._missingCli = false;
            this._fetchError = '';
            this._summary = payload;
            writeCache(payload);
        } catch (e) {
            const message = String(e && e.message || e);
            this._fetchError = cliFailureHint(message);
            this._missingCli = /not found|No such file|G_IO_ERROR_NOT_FOUND/i.test(message);
            this._summary = this._summary || readCache();
            if (this._summary) this._summary._stale = true;
        } finally {
            this._busy = false;
            this._renderPanel();
            this._rebuildPopup();
        }
    }

    _pinnedPercent() {
        const s = this._summary;
        if (!s) return 0;
        if (s.pinnedPercent !== undefined && s.pinnedPercent !== null)
            return Number(s.pinnedPercent);
        const pinned = this._settings.get_string('pinned-provider');
        if (pinned) {
            for (const p of s.providers || []) {
                if (pinKey(p) === pinned) return Number(p.max_percentage || 0);
            }
            if (!pinned.includes('/')) {
                for (const p of s.providers || []) {
                    if (p.provider_id === pinned) return Number(p.max_percentage || 0);
                }
            }
        }
        return Number(s.percentage || 0);
    }

    _orderedProviders() {
        const providers = [...(this._summary?.providers || [])];
        const order = this._settings.get_strv('provider-order').filter(Boolean);
        if (!order.length) return providers;
        const rank = new Map(order.map((k, i) => [k, i]));
        return providers.sort((a, b) =>
            (rank.get(pinKey(a)) ?? 1e9) - (rank.get(pinKey(b)) ?? 1e9));
    }

    _renderBusy() {
        this._label.set_text('…');
    }

    _renderPanel() {
        const showText = this._settings.get_boolean('show-bar-text');
        const themeColors = this._theme().colors;
        this._label.visible = showText;
        if (!this._summary) {
            this._label.set_text('--');
            this._label.style_class = 'um-panel-text um-stale';
            this._setStyle(this._label, themeColors
                ? ['color: ' + themeColors.subtext + ';']
                : []);
            return;
        }
        const showDecimals = this._settings.get_boolean('show-decimals');
        const percentages = panelPercentages(
            this._summary,
            this._settings.get_string('pinned-provider'),
            this._settings.get_strv('bar-windows'));
        const values = percentages.length ? percentages : [this._pinnedPercent()];
        const pct = Math.max(...values);
        this._label.set_text(showText
            ? values.map(value => pctLabel(value, showDecimals)).join(' • ')
            : '');
        const stale = this._summary.class === 'stale' || this._summary._stale === true;
        this._label.style_class = `um-panel-text um-${levelFor(pct)}` +
            (stale ? ' um-stale' : '');
        this._setStyle(this._label, themeColors
            ? ['color: ' + themeColors[levelFor(pct)] + ';']
            : []);
    }

    _clearPopup() {
        for (const item of [...this.menu._getMenuItems()])
            item.destroy();
    }

    _rebuildPopup() {
        this._clearPopup();
        const showDecimals = this._settings.get_boolean('show-decimals');
        const showEmail = this._settings.get_boolean('show-account-email');
        const theme = this._theme();
        const colors = theme.colors;
        this._applyPopupTheme(theme);

        const header = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        const hbox = new St.BoxLayout({ style_class: 'um-header', x_expand: true });
        hbox.add_child(new St.Icon({
            gicon: this._icon.gicon,
            style_class: 'um-header-icon',
            y_align: Clutter.ActorAlign.CENTER,
        }));
        const title = new St.BoxLayout({ vertical: true, style_class: 'um-title-box' });
        const titleLabel = new St.Label({ text: 'Usage Monitor', style_class: 'um-title' });
        this._setStyle(titleLabel, colors ? ['color: ' + colors.text + ';'] : []);
        title.add_child(titleLabel);
        const staleSummary = this._summary &&
            (this._summary.class === 'stale' || this._summary._stale === true);
        const sub = this._summary
            ? `${this._summary.text}${staleSummary ? ' · cached/stale' : ''}`
            : 'No provider data yet';
        const subtitle = new St.Label({ text: sub, style_class: 'um-subtitle' });
        this._setStyle(subtitle, colors ? ['color: ' + colors.subtext + ';'] : []);
        title.add_child(subtitle);
        hbox.add_child(title);
        const spacer = new St.Bin({ x_expand: true });
        hbox.add_child(spacer);
        for (const [label, cb] of [['Refresh', () => this.refresh()],
            ['Settings', () => this._ext.openPreferences()]]) {
            const btn = new St.Button({ style_class: 'um-button' });
            this._setStyle(btn, [
                colors && 'color: ' + colors.text + ';',
                colors && 'border-color: ' + colors.border + ';',
                'border-radius: ' + theme.cornerRadius + 'px;',
            ]);
            btn.set_child(new St.Label({ text: label }));
            btn.connect('clicked', cb);
            hbox.add_child(btn);
        }
        header.add_child(hbox);
        this.menu.addMenuItem(header);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        if (this._fetchError) {
            const warn = new PopupMenu.PopupBaseMenuItem({ reactive: false });
            const wbox = new St.BoxLayout({ vertical: true, style_class: 'um-warning-box' });
            const warningTitle = new St.Label({
                text: this._missingCli ? 'usage-monitor-cli not found' : 'Live usage unavailable',
                style_class: 'um-warning-title',
            });
            this._setStyle(warningTitle, colors ? ['color: ' + colors.warning + ';'] : []);
            wbox.add_child(warningTitle);
            const detail = new St.Label({
                text: this._fetchError,
                style_class: 'um-warning-body',
            });
            detail.clutter_text.line_wrap = true;
            this._setStyle(detail, colors ? ['color: ' + colors.subtext + ';'] : []);
            wbox.add_child(detail);
            warn.add_child(wbox);
            this.menu.addMenuItem(warn);
            this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        }

        const update = this._updateInfo();
        if (update) {
            const banner = new PopupMenu.PopupBaseMenuItem({ reactive: false });
            const bbox = new St.BoxLayout({ vertical: true, style_class: 'um-update-box' });
            bbox.add_child(new St.Label({
                text: `Update ${update.available} available (installed ${update.installed}) — ` +
                    'run `usage-monitor-cli widget sync gnome`, then log out and back in (Wayland).',
                style_class: 'um-update-text',
            }));
            banner.add_child(bbox);
            this.menu.addMenuItem(banner);
        }

        const providers = this._orderedProviders();
        providers.forEach((entry, i) => {
            if (i > 0)
                this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            this.menu.addMenuItem(this._providerItem(entry, showDecimals, showEmail, theme));
        });

        if (!(this._summary?.providers || []).length && !this._fetchError) {
            const empty = new PopupMenu.PopupBaseMenuItem({ reactive: false });
            const message = new St.Label({
                text: 'No provider data yet. Enable a provider or configure credentials, then refresh.',
                style_class: 'um-empty',
            });
            message.clutter_text.line_wrap = true;
            this._setStyle(message, colors ? ['color: ' + colors.subtext + ';'] : []);
            empty.add_child(message);
            this.menu.addMenuItem(empty);
        }
    }

    _updateInfo() {
        // Filled by refresh() from the local stamp comparison in the future;
        // the CLI `widget check-update gnome` result is surfaced in Preferences.
        return null;
    }

    _providerItem(entry, showDecimals, showEmail, theme) {
        const colors = theme.colors;
        const item = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        const card = new St.BoxLayout({
            vertical: true,
            style_class: 'um-card',
            x_expand: true,
        });
        this._setStyle(card, [
            colors && 'border: 1px solid ' + colors.border + ';',
            'border-radius: ' + theme.cornerRadius + 'px;',
            'padding: 6px;',
        ]);

        const head = new St.BoxLayout({ style_class: 'um-card-head' });
        const name = new St.Label({
            text: entry.display_name || entry.provider_id || 'Provider',
            style_class: 'um-card-title',
            x_expand: true,
        });
        this._setStyle(name, colors ? ['color: ' + colors.text + ';'] : []);
        // A failed entry with no window data has no meaningful headline:
        // show an em-dash in error red instead of a misleading "0%".
        const hasWindows = (entry.windows || []).some(w =>
            w.percentage !== undefined && w.percentage !== null);
        const errNoData = !!entry.error && !hasWindows;
        const pct = new St.Label({
            text: errNoData ? '—' : pctLabel(entry.max_percentage || 0, showDecimals),
            style_class: errNoData
                ? 'um-card-pct um-card-error'
                : `um-card-pct um-${levelFor(entry.max_percentage || 0)}`,
        });
        head.add_child(name);
        head.add_child(pct);
        if (colors) this._setStyle(pct, [
            'color: ' + (errNoData ? colors.critical : colors[levelFor(entry.max_percentage || 0)]) + ';',
        ]);
        card.add_child(head);

        const acct = accountText(entry);
        if (acct && showEmail) {
            const account = new St.Label({ text: acct, style_class: 'um-card-account' });
            this._setStyle(account, colors ? ['color: ' + colors.subtext + ';'] : []);
            card.add_child(account);
        } else if (entry.plan && !showEmail) {
            const plan = new St.Label({ text: entry.plan, style_class: 'um-card-account' });
            this._setStyle(plan, colors ? ['color: ' + colors.subtext + ';'] : []);
            card.add_child(plan);
        }
        if (entry.stale === true || entry.error) {
            const status = new St.Label({
                text: entry.error ? String(entry.error.message || entry.error) : 'Using last successful value',
                style_class: entry.error ? 'um-card-error' : 'um-card-stale',
            });
            status.clutter_text.line_wrap = true;
            this._setStyle(status, colors ? [
                'color: ' + (entry.error ? colors.critical : colors.subtext) + ';',
            ] : []);
            card.add_child(status);
        }

        for (const win of windowList(entry)) {
            const row = new St.BoxLayout({ style_class: 'um-win-row' });
            const winLabel = new St.Label({
                text: win.label, style_class: 'um-win-label', x_expand: true,
            });
            this._setStyle(winLabel, colors ? ['color: ' + colors.text + ';'] : []);
            row.add_child(winLabel);
            const winPct = new St.Label({
                text: pctLabel(win.percent, showDecimals),
                style_class: `um-win-pct um-${levelFor(win.percent)}`,
            });
            this._setStyle(winPct, colors ? [
                'color: ' + colors[levelFor(win.percent)] + ';',
            ] : []);
            row.add_child(winPct);
            card.add_child(row);
            const track = new St.BoxLayout({ style_class: 'um-track', x_expand: true });
            const fill = new St.Bin({
                style_class: `um-fill um-${levelFor(win.percent)}`,
            });
            this._setStyle(track, [
                colors && 'background-color: ' + colors.track + ';',
                'border-radius: ' + theme.cornerRadius + 'px;',
                'height: ' + theme.barHeight + 'px;',
            ]);
            this._setStyle(fill, [
                colors && 'background-color: ' + colors[levelFor(win.percent)] + ';',
                'border-radius: ' + theme.cornerRadius + 'px;',
                'height: ' + theme.barHeight + 'px;',
            ]);
            track.add_child(fill);
            const fraction = Math.max(0, Math.min(100, win.percent)) / 100;
            track.connect('notify::width', () => {
                fill.width = track.width * fraction;
            });
            card.add_child(track);
            if (win.reset) {
                const reset = win.reset.startsWith('Reset') ? win.reset : `Resets: ${win.reset}`;
                const resetLabel = new St.Label({ text: reset, style_class: 'um-win-reset' });
                this._setStyle(resetLabel, colors ? ['color: ' + colors.subtext + ';'] : []);
                card.add_child(resetLabel);
            }
        }
        const cost = costLabel(entry.cost);
        if (cost) {
            const costLabel = new St.Label({ text: cost, style_class: 'um-card-cost' });
            this._setStyle(costLabel, colors ? ['color: ' + colors.subtext + ';'] : []);
            card.add_child(costLabel);
        }
        item.add_child(card);
        return item;
    }

    destroy() {
        if (this._timerId) {
            GLib.Source.remove(this._timerId);
            this._timerId = 0;
        }
        if (this._settingsChangedId) {
            this._settings.disconnect(this._settingsChangedId);
            this._settingsChangedId = 0;
        }
        super.destroy();
    }
});

export default class UsageMonitorExtension extends Extension {
    enable() {
        this._indicator = new UsageMonitorIndicator(this);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
    }

    disable() {
        this._indicator?.destroy();
        this._indicator = null;
    }
}
