const field = (key, label, secret = false, placeholder = '') => ({
    key, label, secret, placeholder,
});

const apiKey = [field('api_key', 'API key', true, 'sk-…')];
const token = [field('token', 'Token', true)];
const cookie = [field('cookie', 'Session cookie', true)];

const oauthHints = {
    codex: 'Use codex login in a terminal with a separate CODEX_HOME, then enter that account’s auth.json path.',
    claude: 'Sign in with Claude Code. For another account, use a separate credentials.json file and enter its path.',
    gemini: 'Sign in with gemini or gcloud auth application-default login, then enter the credentials path or a temporary access token.',
    antigravity: 'Enter the account’s oauth_creds.json path. Token refresh can also require the OAuth client ID and client secret.',
};

const authByProvider = {
    ...Object.fromEntries([
        'openai', 'anthropic', 'openrouter', 'groq', 'deepseek', 'kimik2',
        'minimax', 'moonshot', 'venice', 'zai', 'elevenlabs',
    ].map(id => [id, { kind: 'api_key', fields: apiKey }])),
    deepgram: {
        kind: 'api_key',
        fields: [...apiKey, field('project_id', 'Project ID')],
    },
    llmproxy: {
        kind: 'api_key',
        fields: [...apiKey, field('base_url', 'Base URL', false, 'https://…')],
    },
    ...Object.fromEntries(
        ['grok', 'kimi', 'copilot', 'windsurf'].map(id => [id, { kind: 'token', fields: token }]),
    ),
    devin: {
        kind: 'token',
        fields: [...token, field('org', 'Organization')],
    },
    ...Object.fromEntries(
        ['abacus', 'mistral', 'ollama', 'cursor', 'perplexity']
            .map(id => [id, { kind: 'cookie', fields: cookie }]),
    ),
    codex: {
        kind: 'oauth',
        fields: [field('credentials_path', 'Credentials path', false, '~/.codex-work/auth.json')],
        setupHint: oauthHints.codex,
        requiredFields: ['credentials_path'],
    },
    claude: {
        kind: 'oauth',
        fields: [field('credentials_path', 'Credentials path', false, '~/.claude/.credentials.json')],
        setupHint: oauthHints.claude,
        requiredFields: ['credentials_path'],
    },
    gemini: {
        kind: 'oauth',
        fields: [
            field('credentials_path', 'Credentials path'),
            field('access_token', 'Access token', true),
        ],
        setupHint: oauthHints.gemini,
        requiredAny: ['credentials_path', 'access_token'],
    },
    antigravity: {
        kind: 'oauth',
        fields: [
            field('credentials_path', 'Credentials path'),
            field('access_token', 'Access token', true),
            field('client_id', 'OAuth client ID'),
            field('client_secret', 'OAuth client secret', true),
        ],
        setupHint: oauthHints.antigravity,
        requiredAny: ['credentials_path', 'access_token'],
    },
    'opencode-go': {
        kind: 'token',
        fields: [field('token', 'API key', true)],
    },
};

const defaultAuth = { kind: 'api_key', fields: apiKey };

export function providerAuth(providerId) {
    return authByProvider[providerId] || defaultAuth;
}

export function parseProviderList(output) {
    const providers = [];
    for (const line of output.split('\n')) {
        const match = line.trim().match(
            /^(\S+)\s+(enabled(?:\s+\(auto\))?|disabled(?:\s+\(auto\))?)\s+(.+)$/);
        if (!match) continue;
        const [, id, state, description] = match;
        providers.push({
            id,
            displayName: description.split(' — ', 1)[0].trim() || id,
            enabled: state.startsWith('enabled'),
            state,
        });
    }
    return providers;
}

export function parseProviderAccounts(output) {
    const accounts = [];
    let current = null;
    for (const line of output.split('\n')) {
        const trimmed = line.trim();
        const heading = trimmed.match(/^\[([^\]]+)\](?:\s+(.*))?$/);
        if (heading) {
            const [, id, labelText] = heading;
            const rawLabel = (labelText || id).trim();
            const autoDetected = /auto-detected/i.test(rawLabel);
            const label = autoDetected ? 'Auto-detected credentials' : rawLabel;
            current = {
                id,
                label,
                active: true,
                removable: !autoDetected,
                autoDetected,
            };
            accounts.push(current);
        } else if (current && trimmed === 'disabled') {
            current.active = false;
        }
    }
    return accounts;
}
