# Usage Monitor

AI API usage monitor for your terminal.

Collects, stores, and displays consumption metrics from AI services like
Anthropic Claude, OpenAI, DeepSeek, Groq, and many more — all without
depending on external servers.

A Linux port of [CodexBar](https://github.com/steipete/CodexBar) by
[Peter Steinberger](https://github.com/steipete), reimplemented in Rust.

## Providers

| ID          | Service                       | Auth                                               |
|-------------|-------------------------------|----------------------------------------------------|
| `claude`    | Claude Pro/Max (subscription) | Claude Code OAuth (`~/.claude/.credentials.json`)  |
| `anthropic` | Anthropic API                 | `ANTHROPIC_API_KEY` or `--api-key`                 |
| `openai`    | OpenAI API                    | `OPENAI_API_KEY` or `--api-key`                    |

## Usage

```bash
# List available providers
usage-monitor-cli list

# Claude subscription usage (reads Claude Code CLI credentials)
usage-monitor-cli fetch claude

# Anthropic / OpenAI API usage
usage-monitor-cli fetch anthropic --api-key sk-ant-...
usage-monitor-cli fetch openai

# Machine-readable output
usage-monitor-cli fetch claude --json
```

## Structure

```
usage-monitor-core/     Core library (models, providers, fetching)
usage-monitor-cli/      Command-line interface
docs/clean-room/        Clean room provider specifications
```

## Build

```bash
cargo build
cargo test
```

## Tests

```bash
# All tests
cargo test

# Specific module
cargo test -p usage-monitor-core -- model::usage
cargo test -p usage-monitor-core -- provider::anthropic
```

## Credits

Concept, provider research, and original macOS implementation:
[steipete/CodexBar](https://github.com/steipete/CodexBar) (MIT). This project
ports the idea to Linux as a Rust library + CLI.

## License

MIT
