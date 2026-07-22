# OpenSDK tooling protocol

**OpenSDK protocol:** `0.2.0-beta.1`
**MCP adapter:** `2025-11-25`
**Transport:** newline-delimited JSON-RPC 2.0 over stdio

The tooling plane has one host and multiple clients. Editors use the native
`pliego/handshake`; the reference MCP adapter performs the required MCP
initialization lifecycle and exposes that same handshake as
`pliego_sdk_handshake`. MCP receives no implicit filesystem root, network,
sampling, elicitation, or project capability.

```powershell
pliego sdk tooling-host --protocol pliego --feature diagnostic-links
pliego sdk tooling-host --protocol mcp --feature diagnostic-links
npm run check:opensdk:tooling
```

The native handshake request includes
`params.protocolVersion = "0.2.0-beta.1"`; omission or mismatch fails before
diagnostics are available. MCP `initialize` requires a protocol version,
capabilities object, and bounded client name/version. The reference tool accepts
only the empty arguments object declared by its input schema.

The host accepts at most 10,000 requests per process and 1 MiB per JSON line.
Unknown fields, invalid JSON-RPC versions, unsupported MCP versions, calls
before MCP initialization, unknown methods, and unknown tools return structured
errors. Notifications produce no response.

The executable conformance client proves native editor negotiation and the MCP
sequence `initialize -> notifications/initialized -> tools/list -> tools/call`.
MCP is a client surface, not a privileged path around OpenSDK policy.

References:

- [JSON-RPC 2.0](https://www.jsonrpc.org/specification)
- [MCP 2025-11-25 schema](https://modelcontextprotocol.io/specification/2025-11-25/schema)
- [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
