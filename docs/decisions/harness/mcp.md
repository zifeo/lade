# MCP

An MCP server reaches Lade only through the bridge `lade mcp` starts, and the client and the server own the protocol: one stray byte on the bridge's stdout breaks the session. This note binds what the bridge passes through and how often it resolves a server's access.

## MCP

"When a harness needs MCP rather than shell rewrite", `lade mcp` starts a stdio server or bridges a Streamable HTTP one, with "the same rule matching, disclaimers, and diary as `lade -- <cmd>`" (`README.md`, MCP). The bindings become the stdio child's environment, or headers on the upstream requests, and nothing else, [jfxr4rmae](../promises.md#jfxr4rmae).

MCP stdout is protocol data: "copied without redaction or decoration. Lade diagnostics use stderr" (`docs/architecture.md` section 6). Masking, which every other wrap applies, [qq076qjq2](../secrets/output.md#qq076qjq2), would rewrite JSON-RPC messages and break the session, so the bridge does not mask. Over HTTP the bridge parses each message to learn the protocol version, then writes the server's own bytes, its line breaks dropped so the message fits the one line stdio framing needs: a re-serialised message would rewrite a number JSON allows but Rust cannot hold. A header carries a secret across the network, so the bridge sends one only over HTTPS or to the local machine, refuses a URL that holds credentials, and follows no redirect that would hand the headers to another host (`crates/lade-sdk/src/mcp/tests.rs`, `allows_https_and_loopback_http_urls`). Some header names belong to the transport, which sets them per request: a binding by that name would desynchronise the session (`crates/lade-sdk/src/mcp/mod.rs`, `header_map`; `rejects_transport_owned_headers`).

**§ <a id="aeffh6p49">aeffh6p49</a> `lade mcp` passes the protocol between the client and the server unchanged, and adds only the server's access.**

- (a) Each server message reaches the client with its content unchanged and unmasked. Lade writes its own diagnostics to stderr only.

- (b) Over HTTP, bindings travel as request headers only to an HTTPS URL or to plain HTTP on a loopback host; a URL with credentials or a fragment fails before any request, and a redirect fails the bridge.

- (c) A binding named like a header the transport owns, such as `content-type` or `mcp-session-id`, fails before any request.

Resolving access again means calling the vaults again, a Touch ID or a key touch included. A stdio server that dies before the client's `initialize` reached it has lost nothing, so Lade starts it again with the environment it already resolved, for up to 60 s, then gives up with a box (CHANGELOG 0.18.0, MCP stdio restart; 0.20.0, MCP startup; `tests/mcp.rs`, `test_mcp_restarts_stdio_child_before_initialize_without_rehydrating`). After `initialize` the session is the client's: a new server would not know it, so a server exit ends the bridge (`docs/architecture.md` section 6), and each request still open gets an error rather than silence (`src/mcp/tests.rs`, `after_initialize_answers_in_flight_ids_and_does_not_restart`). An HTTP request that fails in transport is answered in-process, and the bridge goes on without replaying it (`bridge_keeps_running_after_a_transport_error_without_replaying`).

**§ <a id="hkqw5dexk">hkqw5dexk</a> `lade mcp` resolves a server's access once, and reuses it across a restart or a transport error.**

- (a) A stdio server that exits before the client's `initialize` reaches it restarts with the same environment for a bounded startup window, then `lade mcp` fails.

- (b) After `initialize`, a server exit ends `lade mcp` with the server's status, and each request still open gets a JSON-RPC error.

- (c) An HTTP request that fails in transport gets a JSON-RPC error, and the bridge goes on without replaying it.

An MCP server of Lade's own, exposing its verbs as tools, is a deliberate non-goal: Lade is an interceptor, not a data source, and an agent already drives it through the CLI (`docs/architecture.md` section 7, MCP: out of scope).
