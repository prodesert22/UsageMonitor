const WINDOW_ORDER = ['primary', 'secondary', 'tertiary'];
const WINDOW_LABELS = { primary: 'Session (5h)', secondary: 'Weekly', tertiary: 'Monthly' };

export function windowList(entry) {
    const out = [];
    for (const key of WINDOW_ORDER) {
        const wins = (entry.windows || []).filter(win => {
            const id = String(win.id || '');
            return id === key ||
                (key === 'primary' && /session/i.test(id)) ||
                (key === 'secondary' && /week/i.test(id)) ||
                (key === 'tertiary' && /month/i.test(id));
        });
        for (const win of wins) {
            if (win.percentage === undefined || win.percentage === null) continue;
            const percent = Number(win.percentage);
            if (!Number.isFinite(percent)) continue;
            out.push({ key, label: WINDOW_LABELS[key], percent, reset: win.resets_at || '' });
        }
    }
    return out;
}

function pinKey(entry) {
    if (entry.account_id) return `${entry.provider_id}/${entry.account_id}`;
    return entry.provider_id || '';
}

function findPinnedEntry(entries, pinnedProvider) {
    if (!pinnedProvider) return null;
    return entries.find(entry => pinKey(entry) === pinnedProvider) ||
        (!pinnedProvider.includes('/')
            ? entries.find(entry => entry.provider_id === pinnedProvider)
            : null) || null;
}

export function panelPercentages(summary, pinnedProvider, selectedWindows) {
    const entries = (summary.providers || []).filter(entry => !entry.error);
    const pinned = findPinnedEntry(entries, pinnedProvider);
    const sources = pinned ? [pinned] : entries;
    const selected = new Set(selectedWindows || []);
    const percentages = [];

    for (const key of WINDOW_ORDER) {
        if (!selected.has(key)) continue;
        const values = [];
        for (const entry of sources) {
            for (const window of windowList(entry)) {
                if (window.key === key) values.push(window.percent);
            }
        }
        if (values.length) percentages.push(Math.max(...values));
    }
    return percentages;
}
