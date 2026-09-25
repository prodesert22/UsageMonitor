const builtinThemes = {
    'macos-dark': {
        background: '#1c1c1e', text: '#f5f5f7', subtext: '#98989d',
        accent: '#0a84ff', warning: '#ff9f0a', critical: '#ff453a',
        track: '#3a3a3c', border: '#48484a',
    },
    'macos-light': {
        background: '#f5f5f7', text: '#1d1d1f', subtext: '#6e6e73',
        accent: '#007aff', warning: '#ff9500', critical: '#ff3b30',
        track: '#d1d1d6', border: '#c6c6c8',
    },
    nord: {
        background: '#2e3440', text: '#eceff4', subtext: '#81a1c1',
        accent: '#88c0d0', warning: '#ebcb8b', critical: '#bf616a',
        track: '#3b4252', border: '#4c566a',
    },
    dracula: {
        background: '#282a36', text: '#f8f8f2', subtext: '#6272a4',
        accent: '#bd93f9', warning: '#ffb86c', critical: '#ff5555',
        track: '#44475a', border: '#44475a',
    },
    'tokyo-night': {
        background: '#1a1b26', text: '#c0caf5', subtext: '#565f89',
        accent: '#7aa2f7', warning: '#e0af68', critical: '#f7768e',
        track: '#24283b', border: '#292e42',
    },
};

const colorKeys = ['background', 'text', 'subtext', 'accent', 'warning',
    'critical', 'track', 'border'];

function validColor(value) {
    return typeof value === 'string' &&
        /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i.test(value.trim());
}

function clamp(value, min, max, fallback) {
    const number = Number(value);
    return Number.isFinite(number) ? Math.max(min, Math.min(max, number)) : fallback;
}

export function resolveTheme(mode, builtin, custom, opacity, barHeight, cornerRadius) {
    let colors = null;
    if (mode === 'light') {
        colors = builtinThemes['macos-light'];
    } else if (mode === 'dark') {
        colors = builtinThemes['macos-dark'];
    } else if (mode === 'builtin') {
        colors = builtinThemes[builtin] || builtinThemes['macos-dark'];
    } else if (mode === 'custom') {
        colors = { ...builtinThemes['macos-dark'] };
        const values = custom && typeof custom === 'object' ? custom : {};
        for (const key of colorKeys) {
            if (validColor(values[key])) colors[key] = values[key].trim();
        }
    }
    return {
        colors,
        opacity: clamp(opacity, 0.3, 1.0, 1.0),
        barHeight: Math.round(clamp(barHeight, 2, 16, 6)),
        cornerRadius: Math.round(clamp(cornerRadius, 0, 16, 4)),
    };
}
