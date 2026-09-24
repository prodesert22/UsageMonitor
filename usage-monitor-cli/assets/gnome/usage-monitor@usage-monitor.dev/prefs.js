/* Usage Monitor preferences (GNOME 45+, Gtk4).
 *
 * Pages mirror the KDE widget settings: General, Providers, Order, Theme,
 * Updates, About. Provider/account state lives in the CLI config; this UI
 * shells out to `usage-monitor-cli enable|disable` and surfaces the same
 * per-auth-type account hints the KDE widget shows.
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';
import { ExtensionPreferences } from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

const ACCOUNT_HINTS = [
    ['API key', 'openai, anthropic, openrouter, groq, deepseek, kimik2, minimax,\nmoonshot, venice, zai, elevenlabs, deepgram, llmproxy',
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

function row(labelText, widget) {
    const box = new Gtk.Box({ orientation: Gtk.Orientation.HORIZONTAL, spacing: 12 });
    box.set_margin_start(12); box.set_margin_end(12);
    box.set_margin_top(6); box.set_margin_bottom(6);
    const label = new Gtk.Label({ label: labelText, hexpand: true, xalign: 0 });
    box.append(label);
    box.append(widget);
    return box;
}

function section(title) {
    const label = new Gtk.Label({ label: `<b>${title}</b>`, use_markup: true, xalign: 0 });
    label.set_margin_start(12); label.set_margin_top(12);
    return label;
}

function hint(text) {
    const label = new Gtk.Label({ label: text, xalign: 0, wrap: true });
    label.add_css_class('dim-label');
    label.set_margin_start(12); label.set_margin_end(12);
    return label;
}

export default class UsageMonitorPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        const stack = new Gtk.Stack();
        const switcher = new Gtk.StackSwitcher({ stack });
        switcher.set_margin_top(6);
        window.set_default_size(640, 560);

        const main = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });
        main.append(switcher);
        main.append(new Gtk.Separator());
        main.append(stack);
        window.add(main);

        stack.add_titled(this._generalPage(settings), 'general', 'General');
        stack.add_titled(this._providersPage(settings), 'providers', 'Providers');
        stack.add_titled(this._orderPage(settings), 'order', 'Order');
        stack.add_titled(this._themePage(settings), 'theme', 'Theme');
        stack.add_titled(this._updatesPage(), 'updates', 'Updates');
        stack.add_titled(this._aboutPage(), 'about', 'About');
    }

    _generalPage(s) {
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });

        const interval = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({ lower: 10, upper: 3600, step_increment: 5, value: s.get_int('refresh-interval') }),
        });
        interval.connect('value-changed', () => s.set_int('refresh-interval', interval.get_value_as_int()));
        page.append(row('Refresh interval (seconds)', interval));
        page.append(hint('Provider endpoints behind subscription plans rate-limit hard; the panel serves its cache between fetches.'));

        for (const [label, key] of [['Show bar text', 'show-bar-text'],
            ['Show account email', 'show-account-email'], ['Show decimal places', 'show-decimals']]) {
            const sw = new Gtk.Switch({ active: s.get_boolean(key), valign: Gtk.Align.CENTER });
            sw.connect('state-set', (_, state) => { s.set_boolean(key, state); return false; });
            page.append(row(label, sw));
        }

        const pin = new Gtk.Entry({
            text: s.get_string('pinned-provider'),
            placeholder_text: 'provider or provider/account (empty = overall)',
        });
        pin.connect('changed', () => s.set_string('pinned-provider', pin.get_text()));
        page.append(row('Pin to top bar', pin));
        page.append(hint('Disabled while "Show bar text" is off, since there is no bar text to drive.'));

        page.append(section('Windows in the bar text'));
        const wins = new Set(s.get_strv('bar-windows'));
        const winBox = new Gtk.Box({ orientation: Gtk.Orientation.HORIZONTAL, spacing: 12 });
        winBox.set_margin_start(12);
        for (const [id, label] of [['primary', 'Session'], ['secondary', 'Weekly'], ['tertiary', 'Monthly']]) {
            const check = new Gtk.CheckButton({ label, active: wins.has(id) });
            check.connect('toggled', () => {
                const cur = new Set(s.get_strv('bar-windows'));
                if (check.get_active()) cur.add(id); else cur.delete(id);
                s.set_strv('bar-windows', [...cur]);
            });
            winBox.append(check);
        }
        page.append(winBox);

        const clear = new Gtk.Button({ label: 'Clear cache' });
        clear.connect('clicked', () => {
            try {
                Gio.File.new_for_path(GLib.build_filenamev(
                    [GLib.get_user_cache_dir(), 'usage-monitor-gnome', 'last.json'])).delete(null);
            } catch { /* already gone */ }
        });
        page.append(row('Cached usage data', clear));
        return page;
    }

    _providersPage(s) {
        void s;
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });
        // `list` is local (no network): "id  state  Name — description".
        const listRes = spawnSync([cliBin(), 'list']);
        const states = new Map();
        for (const line of listRes.out.split('\n')) {
            const m = line.match(/^(\S+)\s+(enabled(?:\s*\(auto\))?|disabled(?:\s*\(auto\))?|auto[^\s]*)/);
            if (m) states.set(m[1], m[2]);
        }
        const cache = readCache();
        const providers = cache?.providers || [];
        if (!providers.length) {
            page.append(hint('No provider data cached yet. Open the top-bar menu once (it fetches on open), then reopen Preferences.'));
        }
        for (const p of providers) {
            const id = p.provider_id || '';
            const state = states.get(id) || '';
            const sw = new Gtk.Switch({ active: !/^disabled/.test(state), valign: Gtk.Align.CENTER });
            // Enabled state is authoritative in the CLI config; toggling shells
            // out to the CLI directly (same as `usage-monitor-cli enable|disable`).
            sw.connect('state-set', (_, active) => {
                spawnSync([cliBin(), active ? 'enable' : 'disable', id]);
                return false;
            });
            page.append(row(`${p.display_name || id}${p.error ? '  ·  error' : ''}  (${state || 'unknown'})`, sw));
        }
        page.append(section('Manage accounts'));
        page.append(hint('Add/remove named accounts in a terminal; the popup lists one card per account automatically:'));
        for (const [kind, who, cmd] of ACCOUNT_HINTS) {
            page.append(hint(`<b>${kind}</b> — ${who}\n${cmd}`));
        }
        return page;
    }

    _orderPage(s) {
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });
        page.append(hint('Order of the provider cards. Empty = CLI order.'));
        const list = new Gtk.ListBox();
        list.set_margin_start(12); list.set_margin_end(12);
        const cache = readCache();
        const seen = new Set();
        const keys = [...s.get_strv('provider-order')];
        for (const p of cache?.providers || []) {
            const k = p.account_id ? `${p.provider_id}/${p.account_id}` : p.provider_id;
            if (!seen.has(k)) { seen.add(k); if (!keys.includes(k)) keys.push(k); }
        }
        const save = () => s.set_strv('provider-order',
            [...list].map(r => r._key));
        const move = (r, dir) => {
            const i = r.get_index();
            list.remove(r);
            list.insert(r, Math.max(0, Math.min(list.observe_children().get_n_items(), i + dir)));
            save();
        };
        for (const k of keys) {
            const r = new Gtk.ListBoxRow();
            r._key = k;
            const box = new Gtk.Box({ orientation: Gtk.Orientation.HORIZONTAL, spacing: 6 });
            box.append(new Gtk.Label({ label: k, hexpand: true, xalign: 0 }));
            const up = new Gtk.Button({ label: '▲' });
            const down = new Gtk.Button({ label: '▼' });
            up.connect('clicked', () => move(r, -1));
            down.connect('clicked', () => move(r, 1));
            box.append(up); box.append(down);
            r.set_child(box);
            list.append(r);
        }
        page.append(list);
        const reset = new Gtk.Button({ label: 'Reset order' });
        reset.connect('clicked', () => {
            s.set_strv('provider-order', []);
            while (list.get_first_child()) list.remove(list.get_first_child());
        });
        page.append(row('Provider order', reset));
        return page;
    }

    _themePage(s) {
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });

        const mode = new Gtk.DropDown({
            model: Gtk.StringList.new(['system', 'light', 'dark', 'builtin', 'custom']),
        });
        const modes = ['system', 'light', 'dark', 'builtin', 'custom'];
        mode.set_selected(Math.max(0, modes.indexOf(s.get_string('theme-mode'))));
        mode.connect('notify::selected', () => s.set_string('theme-mode', modes[mode.get_selected()]));
        page.append(row('Mode', mode));
        page.append(hint('System follows the GNOME color-scheme preference (light/dark palettes built in).'));

        const builtin = new Gtk.DropDown({
            model: Gtk.StringList.new(['macos-dark', 'macos-light', 'nord', 'dracula', 'tokyo-night']),
        });
        const builtins = ['macos-dark', 'macos-light', 'nord', 'dracula', 'tokyo-night'];
        builtin.set_selected(Math.max(0, builtins.indexOf(s.get_string('theme-builtin'))));
        builtin.connect('notify::selected', () => s.set_string('theme-builtin', builtins[builtin.get_selected()]));
        page.append(row('Built-in theme', builtin));

        const opacity = new Gtk.Scale({
            adjustment: new Gtk.Adjustment({ lower: 0.3, upper: 1.0, step_increment: 0.05, value: s.get_double('theme-opacity') }),
            hexpand: true, digits: 2,
        });
        opacity.connect('value-changed', () => s.set_double('theme-opacity', opacity.get_value()));
        page.append(row('Popup opacity', opacity));

        page.append(section('Custom palette'));
        let custom = {};
        try { custom = JSON.parse(s.get_string('theme-custom') || '{}'); } catch { custom = {}; }
        for (const key of COLOR_KEYS) {
            const entry = new Gtk.Entry({ text: custom[key] || '', placeholder_text: '#rrggbb' });
            entry.connect('changed', () => {
                let cur = {};
                try { cur = JSON.parse(s.get_string('theme-custom') || '{}'); } catch { cur = {}; }
                const v = entry.get_text().trim();
                if (v) cur[key] = v; else delete cur[key];
                s.set_string('theme-custom', JSON.stringify(cur));
            });
            page.append(row(key, entry));
        }

        const barH = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({ lower: 2, upper: 16, step_increment: 1, value: s.get_int('bar-height') }),
        });
        barH.connect('value-changed', () => s.set_int('bar-height', barH.get_value_as_int()));
        page.append(row('Bar height (px)', barH));
        const radius = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({ lower: 0, upper: 16, step_increment: 1, value: s.get_int('corner-radius') }),
        });
        radius.connect('value-changed', () => s.set_int('corner-radius', radius.get_value_as_int()));
        page.append(row('Corner radius (px)', radius));
        return page;
    }

    _updatesPage() {
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });
        const res = spawnSync([cliBin(), 'widget', 'check-update', '--pretty', 'gnome']);
        let text = 'Could not reach usage-monitor-cli.';
        try {
            const payload = JSON.parse(res.out);
            const u = (payload.updates || [])[0] || {};
            text = u.installed
                ? `Installed: ${u.installed}\nAvailable (CLI): ${u.available}\n${u.outdated ? 'Outdated — press Update now.' : 'Up to date.'}`
                : 'GNOME widget not installed via the CLI (store install?).';
        } catch { /* keep fallback */ }
        const status = new Gtk.Label({ label: text, xalign: 0 });
        status.set_margin_start(12); status.set_margin_top(12);
        page.append(status);
        const update = new Gtk.Button({ label: 'Update now (widget sync gnome)' });
        update.connect('clicked', () => {
            spawnSync([cliBin(), 'widget', 'sync', 'gnome']);
        });
        page.append(row('Reinstall from the current binary', update));
        page.append(hint('On Wayland, log out and back in after updating so the Shell reloads the extension.'));
        return page;
    }

    _aboutPage() {
        const page = new Gtk.Box({ orientation: Gtk.Orientation.VERTICAL, spacing: 6 });
        let meta = {};
        try {
            meta = JSON.parse(new TextDecoder().decode(
                Gio.File.new_for_path(GLib.build_filenamev(
                    [this.path, 'metadata.json'])).load_contents(null)[1]));
        } catch { /* defaults below */ }
        const title = new Gtk.Label({
            label: `<b>${meta.name || 'Usage Monitor'}</b> ${meta.version || ''}`,
            use_markup: true, xalign: 0,
        });
        title.set_margin_start(12); title.set_margin_top(12);
        page.append(title);
        page.append(hint(meta.description || ''));
        page.append(hint(`${meta.url || 'https://github.com/prodesert22/UsageMonitor'}\nMIT License`));
        return page;
    }
}
