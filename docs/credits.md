# Credits & license

## Credits

The concept, the provider research, and the original macOS implementation are by
[**steipete/CodexBar**](https://github.com/steipete/CodexBar) (MIT).

Usage Monitor ports that idea to Linux as a Rust library + CLI. Every provider's
authentication method, endpoints, and response handling were ported from
CodexBar's implementation. Specifically:

- The native fetchers reproduce CodexBar's API/cookie/OAuth flows and the way it
  maps each upstream response onto usage windows, credits, and costs.
- Browser-cookie providers (e.g. `cursor`, `perplexity`, `abacus`, `mistral`,
  `ollama`) take the session cookie/token from config instead of auto-importing
  it from a browser — the browser/keychain extraction in CodexBar is
  macOS-specific.
- The protobuf providers (`grok` over gRPC-Web, `windsurf` over Connect) reuse
  CodexBar's reverse-engineered field numbers / scanning heuristics.

Per-provider extraction details are documented under
[`docs/providers/`](providers/README.md).

## License

MIT. See the repository's `LICENSE` file.

This project is an independent Linux port and is not affiliated with or endorsed
by CodexBar or any of the monitored AI providers. All product names and
trademarks belong to their respective owners.
