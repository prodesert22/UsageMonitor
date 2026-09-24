/* Usage Monitor GNOME Shell extension (GNOME 45+, ESM).
 *
 * Top-bar indicator backed by the `usage-monitor-cli` binary: it spawns
 * `usage-monitor-cli widget gnome` (the same stable widget JSON contract the
 * KDE and Waybar widgets consume), keeps the last-good payload as a stale
 * fallback, and renders one card per provider/account in the popup.
 *
 * When the CLI is missing (e.g. extension installed from extensions.gnome.org
 * without the binary), the panel shows "--" and the popup explains how to
 * install it instead of failing.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

const WINDOW_LABELS = { primary: 'Session', secondary: 'Weekly', tertiary: 'Monthly' };
const BAR_WIDTH_PX = 220;

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

function windowList(entry) {
    const out = [];
    for (const key of ['primary', 'secondary', 'tertiary']) {
        const wins = (entry.windows || []).filter(w => {
            const id = String(w.id || '');
            return id === key ||
                (key === 'primary' && /session/i.test(id)) ||
                (key === 'secondary' && /week/i.test(id)) ||
                (key === 'tertiary' && /month/i.test(id));
        });
        for (const win of wins) {
            if (win.percentage === undefined || win.percentage === null) continue;
            out.push({
                key,
                label: WINDOW_LABELS[key],
                percent: Number(win.percentage),
                reset: win.resets_at || '',
            });
        }
    }
    return out;
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

class UsageMonitorIndicator extends PanelMenu.Button {
    constructor(ext) {
        super(0.0, 'Usage Monitor', false);
        this._ext = ext;
        this._settings = ext.getSettings();
        this._cli = GLib.getenv('USAGE_MONITOR_BIN') || 'usage-monitor-cli';
        this._summary = null;
        this._busy = false;
        this._missingCli = false;

        const box = new St.BoxLayout({ style_class: 'um-panel-box' });
        this._icon = new St.Icon({
            gicon: Gio.FileIcon.new(Gio.File.new_for_path(
                GLib.build_filenamev([ext.path, 'icons', 'usage-monitor.png']))),
            style_class: 'um-panel-icon',
        });
        this._label = new St.Label({ text: '--', style_class: 'um-panel-text' });
        box.add_child(this._icon);
        box.add_child(this._label);
        this.add_child(box);

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
            const out = await spawnCli([this._cli, 'widget', 'gnome']);
            const payload = JSON.parse(out);
            this._missingCli = false;
            this._summary = payload;
            writeCache(payload);
        } catch (e) {
            const message = String(e && e.message || e);
            this._missingCli = /not found|No such file|G_IO_ERROR_NOT_FOUND/i.test(message) &&
                !readCache();
            if (!this._summary) this._summary = readCache();
            this._lastError = message;
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
        this._label.visible = showText;
        if (!this._summary) {
            this._label.set_text('--');
            this._label.style_class = 'um-panel-text um-stale';
            return;
        }
        const showDecimals = this._settings.get_boolean('show-decimals');
        const pct = this._pinnedPercent();
        this._label.set_text(showText ? pctLabel(pct, showDecimals) : '');
        this._label.style_class = `um-panel-text um-${levelFor(pct)}` +
            (this._summary.class === 'stale' ? ' um-stale' : '');
    }

    _clearPopup() {
        for (const item of [...this.menu._getMenuItems()])
            item.destroy();
    }

    _rebuildPopup() {
        this._clearPopup();
        const showDecimals = this._settings.get_boolean('show-decimals');
        const showEmail = this._settings.get_boolean('show-account-email');

        const header = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        const hbox = new St.BoxLayout({ style_class: 'um-header' });
        const title = new St.BoxLayout({ vertical: true, style_class: 'um-title-box' });
        title.add_child(new St.Label({ text: 'Usage Monitor', style_class: 'um-title' }));
        const sub = this._summary
            ? `${this._summary.text}${this._summary.class === 'stale' ? ' · cached/stale' : ''}`
            : 'No provider data yet';
        title.add_child(new St.Label({ text: sub, style_class: 'um-subtitle' }));
        hbox.add_child(title);
        const spacer = new St.Bin({ x_expand: true });
        hbox.add_child(spacer);
        for (const [label, cb] of [['Refresh', () => this.refresh()],
            ['Settings', () => this._ext.openPreferences()]]) {
            const btn = new St.Button({ label, style_class: 'um-button' });
            btn.connect('clicked', cb);
            hbox.add_child(btn);
        }
        header.add_child(hbox);
        this.menu.addMenuItem(header);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        if (this._missingCli) {
            const warn = new PopupMenu.PopupBaseMenuItem({ reactive: false });
            const wbox = new St.BoxLayout({ vertical: true, style_class: 'um-warning-box' });
            wbox.add_child(new St.Label({
                text: 'usage-monitor-cli not found',
                style_class: 'um-warning-title',
            }));
            wbox.add_child(new St.Label({
                text: 'Install it first, then reopen this menu:\n' +
                    'cargo install --path usage-monitor-cli\n' +
                    'or fetch a release from the project page.',
                style_class: 'um-warning-body',
            }));
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

        for (const entry of this._orderedProviders())
            this.menu.addMenuItem(this._providerItem(entry, showDecimals, showEmail));

        if (!(this._summary?.providers || []).length && !this._missingCli) {
            const empty = new PopupMenu.PopupBaseMenuItem({ reactive: false });
            empty.add_child(new St.Label({
                text: 'No provider data yet. Enable a provider or configure credentials, then refresh.',
                style_class: 'um-empty',
            }));
            this.menu.addMenuItem(empty);
        }
    }

    _updateInfo() {
        // Filled by refresh() from the local stamp comparison in the future;
        // the CLI `widget check-update gnome` result is surfaced in Preferences.
        return null;
    }

    _providerItem(entry, showDecimals, showEmail) {
        const item = new PopupMenu.PopupBaseMenuItem({ reactive: false });
        const card = new St.BoxLayout({ vertical: true, style_class: 'um-card' });

        const head = new St.BoxLayout({ style_class: 'um-card-head' });
        const name = new St.Label({
            text: entry.display_name || entry.provider_id || 'Provider',
            style_class: 'um-card-title',
            x_expand: true,
        });
        const pct = new St.Label({
            text: pctLabel(entry.max_percentage || 0, showDecimals),
            style_class: `um-card-pct um-${levelFor(entry.max_percentage || 0)}`,
        });
        head.add_child(name);
        head.add_child(pct);
        card.add_child(head);

        const acct = accountText(entry);
        if (acct && showEmail) {
            card.add_child(new St.Label({ text: acct, style_class: 'um-card-account' }));
        } else if (entry.plan && !showEmail) {
            card.add_child(new St.Label({ text: entry.plan, style_class: 'um-card-account' }));
        }
        if (entry.stale === true || entry.error) {
            card.add_child(new St.Label({
                text: entry.error ? String(entry.error.message || entry.error) : 'Using last successful value',
                style_class: entry.error ? 'um-card-error' : 'um-card-stale',
            }));
        }

        for (const win of windowList(entry)) {
            const row = new St.BoxLayout({ style_class: 'um-win-row' });
            row.add_child(new St.Label({ text: win.label, style_class: 'um-win-label', x_expand: true }));
            row.add_child(new St.Label({
                text: pctLabel(win.percent, showDecimals),
                style_class: `um-win-pct um-${levelFor(win.percent)}`,
            }));
            card.add_child(row);
            const track = new St.BoxLayout({ style_class: 'um-track' });
            const fill = new St.Bin({
                style_class: `um-fill um-${levelFor(win.percent)}`,
                width: Math.max(0, Math.min(100, win.percent)) / 100 * BAR_WIDTH_PX,
            });
            track.add_child(fill);
            card.add_child(track);
            if (win.reset) {
                const reset = win.reset.startsWith('Reset') ? win.reset : `Resets: ${win.reset}`;
                card.add_child(new St.Label({ text: reset, style_class: 'um-win-reset' }));
            }
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
}

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
