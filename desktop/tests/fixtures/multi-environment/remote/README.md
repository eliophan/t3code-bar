# Synthetic remote contract

`manifest.json` describes two paired remote sources. Both servers intentionally return the same thread ID, with different titles and source-specific usage and limits. The fixture runner binds the two loopback origins to the `httpBaseUrl` values before starting the Bar.

The server contract is the read-only T3 RPC flow. It serves the public descriptor, accepts the fixture pairing code, issues an environment grant, returns a websocket ticket, and sends shell/config/usage events. Shell entries use `threadId` so the Bar must request `orchestration.getThreadProjection`. Permission responses for diagnostics leave thread data available and clear usage and limits. Closing a socket represents offline state and must retain the last good snapshot.
