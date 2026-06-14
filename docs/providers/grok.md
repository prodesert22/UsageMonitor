# Grok provider

Tracks Grok credit usage through the same gRPC-Web billing RPC CodexBar reads
(`grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig`).

## Auth

An xAI access token (Bearer) or a signed-in `grok.com` browser cookie:

```bash
export GROK_TOKEN="xai-..."          # Bearer token
# or a browser cookie
export GROK_COOKIE="..."
# or persist per account
usage-monitor-cli grok set token "xai-..."
usage-monitor-cli grok set cookie "..."
```

`access_token`/`api_key` alias `token` (env `GROK_TOKEN`/`GROK_ACCESS_TOKEN`);
`cookie` uses env `GROK_COOKIE`. At least one is required.

## Data source

- `POST https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig`
- gRPC-Web (`Content-Type: application/grpc-web+proto`); the request is an empty
  frame and the response is protobuf.

The response schema is not published, so — like CodexBar — the protobuf is
generically scanned for the usage percentage (a `float` field ending in field 1,
in `0..=100`) and the reset timestamp (a future unix-seconds varint, preferring
path `1.5.1`).

## Behavior

- Primary window = monthly credit usage (`usedPercent`), with the reset time when
  present.
- A fresh billing period that reports no usage yet renders as 0% used.
- A non-zero gRPC trailer status surfaces as a re-authentication error.

## Multiple accounts

```bash
usage-monitor-cli grok account add work --label "Work"
usage-monitor-cli grok account set work token "xai-..."
usage-monitor-cli fetch grok --account work
```

## Notes

The percentage/reset are located heuristically from the protobuf wire data, so an
upstream schema change can affect parsing.
