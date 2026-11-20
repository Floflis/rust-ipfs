# HTTP Gateway Roadmap

> Status: design / documentation only  
> Repository: `Floflis/rust-ipfs`  
> Target branch: `libp2p-next`  
> Scope: an inbound HTTP Gateway for serving IPFS content to browsers and ordinary HTTP clients.

## 1. Goal

Add a Kubo-compatible HTTP Gateway to Rust-IPFS so a user can open a website identified by an IPFS CID in a normal browser. The gateway must resolve paths inside the IPFS DAG, retrieve missing blocks through the node's existing retrieval mechanisms, and stream the requested resource over HTTP.

The gateway is an HTTP interface over the IPFS node. It must **not** require extracting every website into a temporary or permanent filesystem directory before serving it.

The intended local experience is:

- Path gateway: `http://127.0.0.1:8080/ipfs/<CID>/`
- Subdomain gateway: `http://<CIDv1>.ipfs.localhost:8080/`

Port `8080` is the Kubo-compatible default for the HTTP Gateway. The path and subdomain URL styles are distinct addressing modes, not separate services that require separate default ports.

For browser-hosted websites and DApps, subdomain addressing is the preferred mode because each root CID receives a separate browser origin. Path addressing remains useful for file retrieval and compatibility, but all content served from one path-gateway origin shares the same origin security boundary.

This document is a design and implementation plan, not a claim that any of these inbound HTTP Gateway capabilities already exist.

## 2. Current codebase baseline

The inspected `libp2p-next` codebase already contains useful building blocks:

| Existing area | Reuse in the gateway | Qualification |
|---|---|---|
| Blockstore | Read locally available blocks | Confirm async/error semantics at implementation time. |
| DAG / IPLD resolution | Resolve a CID and path to a node | Preserve codec-aware path semantics. |
| UnixFS Cat | Stream file bytes | Prefer streaming over whole-file buffering. |
| UnixFS Ls | Enumerate directory entries | Useful for directory resolution/listing. |
| UnixFS Get | Export files/directories | Useful for explicit prefetch/export workflows, not required per HTTP request. |
| Bitswap / network retrieval | Retrieve missing blocks | Must use the node's configured retrieval lifecycle. |
| Existing `gateway` module | Optional outbound trustless-gateway retrieval | This is an HTTP client/fallback, **not** an inbound HTTP server. Keep the responsibilities separate. |

The codebase also exposes UnixFS operations through the node/library API. The implementation must inspect the exact public APIs, feature gates, stream types, and tests on the target branch before wiring them together. Do not duplicate UnixFS or DAG traversal merely to make the HTTP layer convenient.

### 2.1 Naming and feature separation

The existing `gateway` feature/module refers to outbound retrieval through external HTTP gateways. Preserve it.

Introduce a separately named inbound server feature, provisionally:

- Cargo feature: `http-gateway`
- Rust module: `http_gateway`

The exact HTTP stack (for example Hyper directly or Axum) remains an implementation decision. Select a stack compatible with the repository's Tokio/runtime versions, MSRV, feature policy, and streaming body requirements. Avoid adding a large framework solely for simple routing.

The gateway feature should be independently selectable and should not silently enable outbound gateway retrieval unless there is a deliberate, documented reason.

## 3. Architecture

Recommended request flow:

```text
HTTP client / browser
        |
        v
HTTP listener (default port 8080)
        |
        v
Host + URL router
        |
        v
IPFS path parser (CID / IPNS name + path)
        |
        v
DAG / UnixFS resolver
        |
        +---- directory resolution (index.html / listing)
        |
        v
UnixFS file stream
        |
        v
HTTP response headers + streaming body
        |
        v
HTTP client / browser
```

The HTTP layer should adapt existing node APIs. It should not own a second blockstore, a second Bitswap implementation, or a second UnixFS implementation.

A proposed initial module layout is:

```text
src/
├── gateway.rs                 # Existing outbound gateway retrieval
└── http_gateway/
    ├── mod.rs                 # Public API and feature boundary
    ├── server.rs              # Listener, lifecycle, graceful shutdown
    ├── routes.rs              # HTTP methods and URL dispatch
    ├── resolver.rs            # CID/name + path resolution
    ├── response.rs            # Status, headers, and streaming body
    ├── directory.rs           # Directory and index behavior
    ├── mime.rs                # Content-Type detection
    └── subdomain.rs           # Host-based content-root addressing
```

This is a proposed organization, not a requirement to create empty modules. Keep the implementation cohesive if the existing codebase suggests a better structure.

## 4. The five implementation blocks

### Block 1 — HTTP server and lifecycle

Build the inbound HTTP server.

Requirements:

- Bind to the Kubo-compatible default gateway address/port, with port `8080`.
- Default to loopback-only access; do not expose a recursive gateway to the LAN or Internet implicitly.
- Allow explicit address/port configuration.
- Integrate with the node's Tokio runtime without creating competing runtimes.
- Support graceful shutdown and cancellation of in-flight work.
- Return clear bind errors, especially when another Kubo or Rust-IPFS process already owns port `8080`.
- Keep HTTP Gateway lifecycle separate from the HTTP RPC API and libp2p swarm listeners.

Kubo's commonly used local arrangement is:

| Purpose | Default |
|---|---|
| HTTP Gateway | TCP `8080` |
| HTTP RPC API | TCP `5001` |
| libp2p swarm | TCP `4001` (transport/configuration dependent) |

The gateway and RPC API are different services. Do not treat port `5001` as the Gateway port or expose administrative RPC methods through the public content-serving router.

The current native CLI is not yet a persistent daemon. A long-running node/daemon lifecycle will be needed to keep the gateway available after startup. Gateway server construction should nevertheless be reusable from Rust applications and from the future daemon, rather than hard-coded only into a CLI command.

### Block 2 — Path routing and content resolution

Implement the Path Gateway baseline:

- `GET /ipfs/{cid}`
- `GET /ipfs/{cid}/{path...}`
- `HEAD /ipfs/{cid}`
- `HEAD /ipfs/{cid}/{path...}`

Plan for the corresponding `/ipns/{name}[/{path}]` routes, but only advertise IPNS support once name resolution (including the intended DNSLink scope) is implemented and tested.

Resolution requirements:

1. Parse and validate the namespace and root identifier.
2. Preserve the remaining URL path, including correct percent-decoding rules.
3. Resolve the CID/path through existing DAG and UnixFS APIs.
4. Distinguish files, directories, unsupported node types, missing paths, and invalid identifiers.
5. Retrieve missing blocks through the node's configured retrieval mechanisms.
6. Map failures to useful HTTP status codes without leaking internal details.
7. Prevent path traversal and ambiguous path normalization from escaping the content root.

The resolver must work with content already in the local blockstore and with content that must be retrieved from the network. Retrieval must obey configured timeouts, cancellation, and resource limits.

Do not implement a separate filesystem extraction step as part of normal GET handling.

### Block 3 — Browser/web-server behavior and HTTP responses

Make UnixFS content behave like resources served by a conventional web server.

Directory behavior:

- A request for a directory without a trailing slash should redirect to its slash-terminated URL when required for relative URL resolution.
- A directory containing `index.html` should serve it for the directory URL.
- A directory without an index should have a deliberate policy: generate a safe directory listing or return a documented not-found/forbidden response. Match Kubo behavior where applicable.
- Do not generate HTML listings by injecting unescaped filenames or untrusted metadata.

Response behavior:

- Set an accurate `Content-Type` for HTML, CSS, JavaScript, JSON, SVG, images, fonts, and other common web assets.
- Use a safe fallback such as `application/octet-stream` when the type is unknown.
- Set `Content-Length` when it is known without consuming the complete stream.
- Support `Content-Disposition` and the Gateway `download` / `filename` query behavior according to the selected specification scope.
- Implement correct cache validators and caching policy, including `ETag` and `Cache-Control` where appropriate.
- Handle conditional requests such as `If-None-Match` if validators are emitted.
- Return consistent HTTP errors for invalid CIDs, missing content, unsupported codecs, timeouts, and retrieval failures.
- Do not assume that CORS headers make path-gateway origins safe for hosting multiple unrelated DApps.

The first release should prioritize the common deserialized web response path needed to load static websites. Trustless response formats (raw blocks and CAR) are a related but distinct compatibility surface; document their support explicitly rather than implying it from ordinary HTML serving.

### Block 4 — HTTP Range Requests

Support byte-range retrieval for files where the underlying UnixFS reader can provide it efficiently.

Minimum target:

- Parse a single `Range: bytes=start-end` request.
- Return `206 Partial Content` with a correct `Content-Range` and `Content-Length`.
- Return `416 Range Not Satisfiable` for invalid or unsatisfiable ranges, with the appropriate `Content-Range: bytes */<length>` when the length is known.
- Return `Accept-Ranges: bytes` only when range behavior is actually supported for the response.
- Ensure the streamed body contains exactly the selected byte interval.
- Avoid reading the entire file into memory to serve a range.
- Define behavior for suffix ranges, open-ended ranges, multiple ranges, empty files, and unknown file lengths.

Start with one range if that is the chosen compatibility baseline. Multiple-range responses can be deferred, but must be rejected or handled deliberately rather than silently returning incorrect bytes.

Inspect the current UnixFS Cat implementation before designing an adapter. If its internal range support is not exposed through a stable public API, add the smallest appropriate public API and tests instead of reaching into private fields from the HTTP module.

Range support is especially useful for media, large files, and clients that resume downloads.

### Block 5 — Subdomain Gateway and browser origin isolation

Implement host-based addressing as an extension of the Path Gateway.

Example:

```text
http://<CIDv1>.ipfs.localhost:8080/
http://<CIDv1>.ipfs.localhost:8080/assets/app.js
```

The root CID is derived from the request's `Host` header; the URL path is relative to that root.

Requirements:

- Parse and validate the host, including an optional port.
- Recognize the configured gateway domain and the `ipfs` namespace.
- Decode the CIDv1 subdomain form according to the Subdomain Gateway specification.
- Resolve the URL path beneath the selected root CID.
- Support relative URLs and directory/index behavior.
- Reject malformed, ambiguous, or untrusted hostnames.
- Do not trust arbitrary `Host`, `Forwarded`, or `X-Forwarded-Host` values unless reverse-proxy trust is explicitly configured.
- Do not let one content root select another root through path manipulation.
- Keep the gateway bound to loopback by default; browser localhost handling must be tested on supported browsers/platforms.

Subdomain addressing is essential for browser-hosted DApps because it gives each root CID a distinct origin. Cookies, localStorage, service workers, and other origin-scoped browser capabilities must not be shared between unrelated content roots.

CIDv0 is not directly suitable as a DNS label in its usual Base58 form. Where path-to-subdomain redirects are implemented, convert to the canonical CIDv1 representation required by the specification and preserve the requested path/query correctly.

IPNS subdomain support is a later extension and must follow the relevant IPNS/Subdomain Gateway rules; do not treat arbitrary DNS names as trusted IPNS identifiers.

## 5. Retrieval strategy: stream by default, prefetch optionally

Normal gateway requests should retrieve and stream content on demand:

1. Browser requests `index.html`.
2. The node resolves the requested path and retrieves the blocks needed for that file.
3. The gateway streams the file to the browser.
4. The browser requests CSS, JavaScript, fonts, and images as separate resources.
5. Retrieved blocks remain in the node's blockstore according to the repository's storage and GC policy.

This is not the same as downloading the entire website to a directory.

Add a separate recursive prefetch workflow if offline preparation is desired. Prefetch should traverse the relevant DAG and retrieve its blocks before the user disconnects. Keep these concepts separate:

- **Gateway:** serve requests and retrieve only what is needed.
- **Prefetch:** proactively retrieve a selected DAG for later offline use.
- **UnixFS Get:** export content into the local filesystem.

Do not claim a website is fully offline-ready merely because its HTML root was requested once.

## 6. Configuration and CLI integration

Proposed configuration defaults:

| Setting | Initial default / policy |
|---|---|
| Gateway enabled | Explicitly configured by the embedding application/daemon; choose and document the CLI default |
| Listen address | Loopback only |
| Listen port | `8080` |
| Path gateway | Enabled |
| Subdomain gateway | Enabled for the local `localhost` gateway when host handling is available |
| Recursive retrieval | Uses the node's configured retrieval policy |
| External exposure | Opt-in, with security documentation |

The exact configuration schema should be designed alongside the daemon/repository configuration work. Avoid inventing a second configuration source that conflicts with the future Kubo-like node configuration.

Future CLI shape:

```sh
# Start the persistent node and its configured HTTP Gateway
ipfs daemon

# Path-style file/site retrieval
# http://127.0.0.1:8080/ipfs/<CID>/

# Browser website/DApp access with origin isolation
# http://<CIDv1>.ipfs.localhost:8080/
```

The commands above describe the intended user experience; they are not implemented by this documentation change.

## 7. Security and operational guardrails

- Bind to loopback by default.
- Do not expose the administrative HTTP RPC API through the Gateway listener.
- Treat host routing and reverse-proxy headers as untrusted input.
- Apply request, retrieval, and concurrency limits to prevent resource exhaustion.
- Bound memory use; stream large responses.
- Set timeouts and support cancellation when clients disconnect.
- Prevent path traversal and unsafe path normalization.
- Escape all generated directory-listing content.
- Define safe MIME and content-disposition behavior.
- Preserve browser origin isolation through subdomain addressing.
- Document that path gateways do not isolate origins and must not be recommended for hosting mutually untrusted DApps.
- Do not add permissive CORS as a substitute for correct origin design.
- Make non-loopback exposure an explicit operator decision.

## 8. Testing and acceptance criteria

### Unit tests

- CID and path parsing, including malformed and percent-encoded inputs.
- Host parsing, CIDv1 subdomain decoding, and invalid namespace rejection.
- Directory/index and trailing-slash behavior.
- MIME selection and safe fallback.
- Range parsing and boundary cases.
- HTTP status/header mapping.
- Host-header and path traversal security cases.

### Integration tests

- Serve a small UnixFS HTML site from a locally populated blockstore.
- Serve the same site when blocks must be retrieved through the node's network retrieval path.
- Fetch HTML, CSS, JavaScript, images, and nested assets using a normal HTTP client.
- Verify that relative URLs resolve correctly.
- Verify GET and HEAD behavior.
- Verify streaming and bounded memory behavior for a large file.
- Verify single-range responses and unsatisfiable ranges.
- Verify that two different CID subdomains produce different browser origins.
- Verify graceful shutdown, port conflicts, and client-disconnect cancellation.
- Verify that the gateway does not accidentally expose the HTTP RPC API.

### Compatibility tests

Use the official IPFS Gateway specifications as the normative reference and compare observable behavior with the selected Kubo release.

Adopt the IPFS Gateway Conformance Test Suite as a compatibility gate where practical. Track unsupported specification sections explicitly; do not label the implementation fully Kubo-compatible while required behavior remains unimplemented.

Test at least:

- Path Gateway specification.
- Subdomain Gateway specification.
- Relevant HTTP semantics and conditional/range behavior.
- Browser behavior for `localhost` subdomains on supported platforms.

### Definition of done for the first usable browser gateway

- A user can start a persistent Rust-IPFS node with the Gateway listening on loopback port `8080`.
- A CID-rooted UnixFS website loads through the path gateway.
- The same website loads through a CIDv1 subdomain gateway.
- Relative CSS, JavaScript, image, and font resources resolve correctly.
- Missing blocks can be retrieved through the node's configured retrieval mechanisms.
- Large file responses are streamed without whole-file buffering.
- GET, HEAD, directory/index behavior, MIME types, and error handling have tests.
- The gateway does not claim origin isolation for path URLs; subdomain isolation is tested.

## 9. Suggested implementation sequence

1. Confirm the exact UnixFS/DAG read and streaming APIs on the target branch.
2. Add the `http-gateway` feature boundary and a reusable server API.
3. Implement the loopback listener and lifecycle/shutdown behavior.
4. Implement Path Gateway CID/path parsing and GET/HEAD.
5. Connect the resolver to existing DAG and UnixFS APIs.
6. Add directory/index handling, MIME detection, and correct HTTP errors.
7. Add streaming response bodies and bounded retrieval behavior.
8. Add single-range support using a public UnixFS range API where necessary.
9. Add Subdomain Gateway host routing and origin-isolation tests.
10. Integrate the server into the persistent daemon/configuration lifecycle.
11. Run the conformance suite and document remaining compatibility gaps.

This sequence keeps the first vertical slice small: serve one locally available UnixFS file, then a directory website, then network-retrieved content, and finally browser-safe subdomain behavior.

## 10. References

Use these specifications and implementation references as living documents. Re-check them when implementation begins:

- Kubo Gateway guide: https://github.com/ipfs/kubo/blob/master/docs/gateway.md
- Kubo Gateway implementation: https://github.com/ipfs/kubo/tree/master/core/corehttp
- HTTP Gateway overview: https://specs.ipfs.tech/http-gateways/
- Path Gateway specification: https://specs.ipfs.tech/http-gateways/path-gateway/
- Subdomain Gateway specification: https://specs.ipfs.tech/http-gateways/subdomain-gateway/
- Trustless Gateway specification: https://specs.ipfs.tech/http-gateways/trustless-gateway/
- HTTP Gateway reference: https://docs.ipfs.tech/reference/http/gateway/
- Address IPFS on the web: https://docs.ipfs.tech/how-to/address-ipfs-on-web/
- Gateway Conformance Test Suite: https://github.com/ipfs/gateway-conformance

Kubo's documented local defaults distinguish `127.0.0.1:8080` for path-style retrieval and `localhost:8080` for subdomain-style website access. Use the same port while preserving the different host-based behavior. Subdomain mode is the recommended browser/DApp path because it isolates origins; path mode is appropriate for file retrieval and compatibility.

## 11. Non-goals for this documentation phase

- No HTTP server or routing code is added.
- No new Cargo dependencies or feature flags are added.
- No claim is made that the current CLI has a working `daemon` command.
- No claim is made that the current outbound `gateway` feature serves inbound HTTP requests.
- No claim is made that all content is prefetched or available offline after one page load.
- No claim of full Kubo Gateway conformance is made before running the conformance tests.


## 12. Implementation progress

The opt-in native http-gateway feature now provides an Axum listener on loopback port 8080, with Path Gateway and CIDv1 Subdomain Gateway request routing.

Implemented in the current branch:

- GET and HEAD for /ipfs/{cid}[/{path}].
- CIDv1 subdomain addressing at <cidv1>.ipfs.localhost, with optional Host port parsing and rejection of unconfigured hosts.
- Percent-decoding of path segments, rejecting malformed escapes, encoded separators, backslashes, NUL bytes, and dot segments.
- UnixFS directory detection through the existing shallow listing API.
- index.html fallback for directories and permanent slash redirects for directory URLs.
- A deliberate no-index policy: directories without index.html return 404; no generated directory listing is exposed.
- MIME types for common web, image, font, media, and document extensions, with application/octet-stream as the fallback.
- Single byte ranges, including closed, open-ended, and suffix forms. Unsatisfiable and multiple-range requests return 416.
- A public UnixfsCat range adapter that applies the interval during UnixFS traversal so unrelated file blocks can be skipped.
- Local UnixFS-backed tests for index serving, MIME, byte-range delivery, and CID subdomain serving.

The feature-specific Rust workflow now runs cargo check with http-gateway enabled and the matching gateway tests. It also uploads the resolved Cargo.lock as an artifact so the lockfile can be committed from the exact dependency resolution used by CI.

This is not yet a claim of full Kubo compatibility. Conditional requests, ETag/If-Range, trustless/CAR responses, IPNS subdomains, configurable gateway domains, daemon/CLI integration, explicit graceful shutdown, and the gateway conformance suite remain outstanding. Stream failures that happen after response headers are sent still terminate the body rather than becoming a new HTTP status.

Do not mark this implementation release-ready until the feature-specific CI checks pass and the generated Cargo.lock has been committed.
