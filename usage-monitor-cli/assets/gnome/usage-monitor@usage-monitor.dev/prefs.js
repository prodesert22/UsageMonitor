/* Usage Monitor preferences (GNOME 45+, libadwaita).
 *
 * Pages mirror the KDE widget settings: General, Providers, Order, Theme,
 * Updates, About. Provider/account state lives in the CLI config; this UI
 * shells out to `usage-monitor-cli enable|disable` and surfaces the same
 * per-auth-type account hints the KDE widget shows.
 *
 * The preferences window only accepts AdwPreferencesPage children, so every
 * page below is one (plain Gtk containers are rejected at add() time).
 */

import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';
import { ExtensionPreferences } from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

const ACCOUNT_HINTS = [
    ['API key', 'openai, anthropic, openrouter, groq, deepseek, kimik2, minimax, moonshot, venice, zai, elevenlabs, deepgram, llmproxy',
        'usage-monitor-cli openai set api_key sk-…'],
    ['Token', 'grok, kimi, copilot, devin, windsurf, opencode-go',
        'usage-monitor-cli kimi set token <kimi-auth-jwt>'],
    ['Cookie', 'abacus, mistral, ollama, cursor, perplexity',
        'usage-monitor-cli <provider> set … (see provider docs)'],
    ['OAuth / CLI', 'codex, claude, gemini, antigravity',
        'codex login   (in a terminal; isolated CODEX_HOME per extra account)'],
];

const COLOR_KEYS = ['background', 'text', 'subtext', 'accent', 'warning',
    'critical', 'track', 'border'];

function cliBin() {
    return GLib.getenv('USAGE_MONITOR_BIN') || 'usage-monitor-cli';
}

function spawnSync(argv) {
    try {
        const proc = Gio.Subprocess.new(argv,
            Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        const [ok, stdout, stderr] = proc.communicate_utf8(null, null);
        return { ok: ok && proc.get_exit_status() === 0, out: stdout || '', err: stderr || '' };
    } catch (e) {
        return { ok: false, out: '', err: String(e && e.message || e) };
    }
}

function readCache() {
    try {
        const path = GLib.build_filenamev(
            [GLib.get_user_cache_dir(), 'usage-monitor-gnome', 'last.json']);
        const [, bytes] = Gio.File.new_for_path(path).load_contents(null);
        return JSON.parse(new TextDecoder().decode(bytes));
    } catch {
        return null;
    }
}

function newPage(title, iconName) {
    return new Adw.PreferencesPage({ title, icon_name: iconName });
}

function newGroup(page, title, description) {
    const group = new Adw.PreferencesGroup({ title, description });
    page.add(group);
    return group;
}

export default class UsageMonitorPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        window.add(this._generalPage(settings));
        window.add(this._providersPage());
        window.add(this._orderPage(settings));
        window.add(this._themePage(settings));
        window.add(this._updatesPage());
        window.add(this._aboutPage());
    }

    _generalPage(s) {
        const page = newPage('General', 'preferences-system-symbolic');

        const refresh = newGroup(page, 'Refresh',
            'Provider endpoints behind subscription plans rate-limit hard; the panel serves its cache between fetches.');
        const interval = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({
                lower: 10, upper: 3600, step_increment: 5,
                value: s.get_int('refresh-interval'),
            }),
            valign: Gtk.Align.CENTER,
        });
        interval.connect('value-changed',
            () => s.set_int('refresh-interval', interval.get_value_as_int()));
        const intervalRow = new Adw.ActionRow({ title: 'Refresh interval (seconds)' });
        intervalRow.add_suffix(interval);
        refresh.add(intervalRow);

        const panel = newGroup(page, 'Top bar');
        for (const [key, title, subtitle] of [
            ['show-bar-text', 'Show bar text', 'When off, the pin setting has no effect.'],
            ['show-account-email', 'Show account email', 'When off, only the plan is shown.'],
            ['show-decimals', 'Show decimal places', 'When off, everything rounds to whole numbers.'],
        ]) {
            const sw = new Adw.SwitchRow({ title, subtitle, active: s.get_boolean(key) });
            sw.connect('notify::active', () => s.set_boolean(key, sw.get_active()));
            panel.add(sw);
        }
        const pin = new Adw.EntryRow({
            title: 'Pin to top bar',
            text: s.get_string('pinned-provider'),
        });
        pin.connect('notify::text', () => s.set_string('pinned-provider', pin.get_text()));
        panel.add(pin);

        const wins = newGroup(page, 'Windows in the bar text',
            'Which usage windows compose the bar text and the pinned headline. ' +
            'Use "provider" or "provider/account" (empty = overall).');
        const selected = new Set(s.get_strv('bar-windows'));
        for (const [id, title] of [['primary', 'Session'], ['secondary', 'Weekly'], ['tertiary', 'Monthly']]) {
            const check = new Adw.SwitchRow({ title, active: selected.has(id) });
            check.connect('notify::active', () => {
                const cur = new Set(s.get_strv('bar-windows'));
                if (check.get_active()) cur.add(id); else cur.delete(id);
                s.set_strv('bar-windows', [...cur]);
            });
            wins.add(check);
        }

        const data = newGroup(page, 'Data');
        const clear = new Gtk.Button({ label: 'Clear', valign: Gtk.Align.CENTER });
        clear.connect('clicked', () => {
            try {
                Gio.File.new_for_path(GLib.build_filenamev(
                    [GLib.get_user_cache_dir(), 'usage-monitor-gnome', 'last.json'])).delete(null);
            } catch { /* already gone */ }
        });
        const clearRow = new Adw.ActionRow({ title: 'Cached usage data' });
        clearRow.add_suffix(clear);
        data.add(clearRow);
        return page;
    }

    _providersPage() {
        const page = newPage('Providers', 'network-workgroup-symbolic');
        // `list` is local (no network): "id  state  Name — description".
        const listRes = spawnSync([cliBin(), 'list']);
        const states = new Map();
        for (const line of listRes.out.split('\n')) {
            const m = line.match(/^(\S+)\s+(enabled(?:\s*\(auto\))?|disabled(?:\s*\(auto\))?|auto[^\s]*)/);
            if (m) states.set(m[1], m[2]);
        }
        const toggles = newGroup(page, 'Providers');
        const cache = readCache();
        const providers = cache?.providers || [];
        if (!providers.length) {
            toggles.description =
                'No provider data cached yet. Open the top-bar menu once (it fetches on open), then reopen Preferences.';
        }
        for (const p of providers) {
            const id = p.provider_id || '';
            const state = states.get(id) || 'unknown';
            const sw = new Adw.SwitchRow({
                title: `${p.display_name || id}${p.error ? '  ·  error' : ''}`,
                subtitle: state,
                active: !/^disabled/.test(state),
            });
            // Enabled state is authoritative in the CLI config; toggling shells
            // out to the CLI directly (same as `usage-monitor-cli enable|disable`).
            sw.connect('notify::active', () => {
                spawnSync([cliBin(), sw.get_active() ? 'enable' : 'disable', id]);
            });
            toggles.add(sw);
        }
        const accounts = newGroup(page, 'Manage accounts',
            'Add/remove named accounts in a terminal; the popup lists one card per account automatically:');
        for (const [kind, who, cmd] of ACCOUNT_HINTS) {
            accounts.add(new Adw.ActionRow({ title: `${kind} — ${who}`, subtitle: cmd }));
        }
        return page;
    }

    _orderPage(s) {
        const page = newPage('Order', 'view-list-ordered-symbolic');
        const group = newGroup(page, 'Provider cards', 'Empty = CLI order.');
        const cache = readCache();
        const seen = new Set();
        const keys = [...s.get_strv('provider-order')];
        for (const p of cache?.providers || []) {
            const k = p.account_id ? `${p.provider_id}/${p.account_id}` : p.provider_id;
            if (!seen.has(k)) { seen.add(k); if (!keys.includes(k)) keys.push(k); }
        }
        const save = () => {
            const ordered = [];
            let i = 0, row;
            while ((row = group.get_row_at_index(i++))) ordered.push(row._key);
            s.set_strv('provider-order', ordered);
        };
        const move = (r, dir) => {
            const i = r.get_index();
            let total = 0;
            while (group.get_row_at_index(total)) total++;
            group.remove(r);
            group.insert(r, Math.max(0, Math.min(total - 1, i + dir)));
            save();
        };
        for (const k of keys) {
            const r = new Adw.ActionRow({ title: k });
            r._key = k;
            const up = new Gtk.Button({ icon_name: 'go-up-symbolic', valign: Gtk.Align.CENTER });
            const down = new Gtk.Button({ icon_name: 'go-down-symbolic', valign: Gtk.Align.CENTER });
            up.connect('clicked', () => move(r, -1));
            down.connect('clicked', () => move(r, 1));
            r.add_suffix(up);
            r.add_suffix(down);
            group.add(r);
        }
        const reset = new Gtk.Button({ label: 'Reset', valign: Gtk.Align.CENTER });
        reset.connect('clicked', () => {
            s.set_strv('provider-order', []);
            let r;
            while ((r = group.get_first_child())) group.remove(r);
        });
        const resetRow = new Adw.ActionRow({ title: 'Reset order' });
        resetRow.add_suffix(reset);
        group.add(resetRow);
        return page;
    }

    _themePage(s) {
        const page = newPage('Theme', 'preferences-desktop-theme-symbolic');
        const mode = newGroup(page, 'Palette',
            'System follows the GNOME color-scheme preference (light/dark palettes built in).');
        const modes = ['system', 'light', 'dark', 'builtin', 'custom'];
        const modeRow = new Adw.ComboRow({
            title: 'Mode',
            model: Gtk.StringList.new(['System', 'Light', 'Dark', 'Built-in', 'Custom']),
            selected: Math.max(0, modes.indexOf(s.get_string('theme-mode'))),
        });
        modeRow.connect('notify::selected', () => s.set_string('theme-mode', modes[modeRow.get_selected()]));
        mode.add(modeRow);
        const builtins = ['macos-dark', 'macos-light', 'nord', 'dracula', 'tokyo-night'];
        const builtinRow = new Adw.ComboRow({
            title: 'Built-in theme',
            model: Gtk.StringList.new(['macOS Dark', 'macOS Light', 'Nord', 'Dracula', 'Tokyo Night']),
            selected: Math.max(0, builtins.indexOf(s.get_string('theme-builtin'))),
        });
        builtinRow.connect('notify::selected',
            () => s.set_string('theme-builtin', builtins[builtinRow.get_selected()]));
        mode.add(builtinRow);

        const opacity = new Gtk.Scale({
            adjustment: new Gtk.Adjustment({
                lower: 0.3, upper: 1.0, step_increment: 0.05,
                value: s.get_double('theme-opacity'),
            }),
            digits: 2, width_request: 160, valign: Gtk.Align.CENTER,
        });
        opacity.connect('value-changed', () => s.set_double('theme-opacity', opacity.get_value()));
        const opacityRow = new Adw.ActionRow({ title: 'Popup opacity' });
        opacityRow.add_suffix(opacity);
        mode.add(opacityRow);

        const custom = newGroup(page, 'Custom palette');
        let palette = {};
        try { palette = JSON.parse(s.get_string('theme-custom') || '{}'); } catch { palette = {}; }
        for (const key of COLOR_KEYS) {
            const entry = new Adw.EntryRow({ title: key, text: palette[key] || '' });
            entry.connect('notify::text', () => {
                let cur = {};
                try { cur = JSON.parse(s.get_string('theme-custom') || '{}'); } catch { cur = {}; }
                const v = entry.get_text().trim();
                if (v) cur[key] = v; else delete cur[key];
                s.set_string('theme-custom', JSON.stringify(cur));
            });
            custom.add(entry);
        }

        const metrics = newGroup(page, 'Metrics');
        for (const [key, title, lower, upper] of [
            ['bar-height', 'Bar height (px)', 2, 16],
            ['corner-radius', 'Corner radius (px)', 0, 16],
        ]) {
            const spin = new Gtk.SpinButton({
                adjustment: new Gtk.Adjustment({
                    lower, upper, step_increment: 1, value: s.get_int(key),
                }),
                valign: Gtk.Align.CENTER,
            });
            spin.connect('value-changed', () => s.set_int(key, spin.get_value_as_int()));
            const r = new Adw.ActionRow({ title });
            r.add_suffix(spin);
            metrics.add(r);
        }
        return page;
    }

    _updatesPage() {
        const page = newPage('Updates', 'software-update-available-symbolic');
        const group = newGroup(page, 'Widget version');
        const res = spawnSync([cliBin(), 'widget', 'check-update', '--pretty', 'gnome']);
        let status = 'Could not reach usage-monitor-cli.';
        try {
            const payload = JSON.parse(res.out);
            const u = (payload.updates || [])[0] || {};
            status = u.installed
                ? `Installed ${u.installed} · available ${u.available}` +
                    (u.outdated ? ' — outdated, press Update now.' : ' — up to date.')
                : 'GNOME widget not installed via the CLI (store install?).';
        } catch { /* keep fallback */ }
        group.add(new Adw.ActionRow({ title: 'Status', subtitle: status }));
        const update = new Gtk.Button({ label: 'Update now', valign: Gtk.Align.CENTER });
        update.connect('clicked', () => {
            spawnSync([cliBin(), 'widget', 'sync', 'gnome']);
        });
        const updateRow = new Adw.ActionRow({
            title: 'Reinstall from the current binary',
            subtitle: 'On Wayland, log out and back in after updating so the Shell reloads the extension.',
        });
        updateRow.add_suffix(update);
        group.add(updateRow);
        return page;
    }

    _aboutPage() {
        const page = newPage('About', 'help-about-symbolic');
        let meta = {};
        try {
            meta = JSON.parse(new TextDecoder().decode(
                Gio.File.new_for_path(GLib.build_filenamev(
                    [this.path, 'metadata.json'])).load_contents(null)[1]));
        } catch { /* defaults below */ }
        const group = newGroup(page, `${meta.name || 'Usage Monitor'} ${meta.version || ''}`);
        group.add(new Adw.ActionRow({
            title: 'Description',
            subtitle: meta.description || '',
        }));
        group.add(new Adw.ActionRow({
            title: 'Website',
            subtitle: meta.url || 'https://github.com/prodesert22/UsageMonitor',
        }));
        group.add(new Adw.ActionRow({ title: 'License', subtitle: 'MIT' }));
        return page;
    }
}
