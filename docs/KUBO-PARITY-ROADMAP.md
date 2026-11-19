# Kubo Parity Roadmap

> Status snapshot: 2026-11-19  
> Repository: `Floflis/rust-ipfs`  
> Target branch: `libp2p-next`

## 1. Purpose and scope

This document records a practical roadmap for turning `rust-ipfs` from a Rust IPFS library with an initial native `ipfs add` command into a usable IPFS node and a Kubo-compatible operational interface.

The goal is **capability parity where it is useful**, not blind reproduction of every Kubo command or implementation detail. Kubo is the behavioral and interoperability reference. Rust-native architecture, storage internals, and implementation choices may differ as long as externally observable protocol and command behavior is documented and tested.

This is an audit snapshot, not a claim that every item below has been exhaustively tested. A library API existing does not mean the corresponding Kubo command, HTTP endpoint, lifecycle behavior, or compatibility guarantee already exists.

## 2. Current project baseline

The repository README describes the native executable as an initial CLI surface focused on adding local files and directories. Its documented options include CID version, SHA2-256/BLAKE3, quiet/quieter output, recursion, hash-only mode, wrapping, and pin control.

The README explicitly lists these `add` gaps:
- stdin input;
- symlink dereferencing;
- ignore rules;
- progress bars;
- advanced UnixFS import settings.

The native binary is therefore **not yet a Kubo CLI replacement**.

### 2.1 Existing library capabilities observed in the codebase

The following public APIs/modules were present in the `libp2p-next` snapshot inspected for this roadmap:

| Area | Existing library surface | Important qualification |
|---|---|---|
| UnixFS | `add_unixfs`, `cat_unixfs`, `get_unixfs`, `ls_unixfs` | CLI coverage is much smaller than the library surface. |
| DAG | `put_dag`, `get_dag`, DAG module | Verify exact codec/format behavior against Kubo. |
| CAR | CAR export/import APIs | Does not by itself provide all Kubo CAR CLI/API behavior. |
| MFS | `mkdir`, `write`, `read`, `read_range`, `truncate`, `rm`, `cp`, `mv`, `ls`, `stat`, streaming/file variants | Exposing this as `ipfs files` remains CLI/RPC work. |
| IPNS | `resolve`, `publish`, timeout variants | Republish lifecycle and full Kubo semantics require separate work. |
| Blocks | put/get/remove and block abstraction | Map command flags and output semantics to Kubo. |
| Pins | insert/remove/query/list/is-pinned and GC | Kubo pin subcommands and output semantics are not implied by these APIs. |
| Refs | local refs and IPLD reference traversal | More complete Kubo `refs` behavior needs verification. |
| Peer/network | identity, connect/disconnect, addresses, connected peers, swarm events | Not all administrative Kubo commands are exposed. |
| DHT/provider | find peer, provider lookup, provide, DHT get/put, closest peers, DHT mode | Verify routing protocol and CLI/API compatibility. |
| Bitswap | wantlist API | Broader Bitswap diagnostics/statistics need verification. |
| PubSub | subscribe, publish, unsubscribe, peers, subscribed topics, listener | Kubo-compatible CLI/RPC surface is not yet established. |
| Bootstrap | get/add/remove/clear/default bootstrap peers | Configuration persistence and CLI behavior still need work. |
| Remote pinning | add/status/list/replace/remove client API | This is a remote-pinning client surface, not a complete pinning service. |
| Gateway retrieval | optional trustless HTTP gateway retrieval | This is an outbound retrieval fallback, **not** an HTTP Gateway server. |
| Delegated routing | optional delegated-routing V1 provider discovery | This is a client capability, **not** a delegated-routing server. |
| Builder/network | TCP, QUIC, relay, AutoNAT, rendezvous, mDNS, WebSocket, WebRTC, PubSub and other behavior configuration | Availability depends on enabled Cargo features and runtime configuration. |

Treat this table as a code-inspection inventory. Before implementing a command, inspect the exact current API, its tests, feature gates, and semantics. Do not infer completeness from a module name.

## 3. Priority roadmap

### P0 — A persistent, controllable node

This is the largest architectural gap between the current executable and an everyday IPFS node.

Implement:
- `ipfs init`: initialize a repository and node identity;
- `ipfs daemon`: start a persistent node;
- `ipfs shutdown`: request a graceful shutdown;
- a Kubo-compatible HTTP RPC API, initially targeting the useful `/api/v0` command surface;
- CLI client mode, including an API endpoint option, so commands can operate against a running daemon rather than always constructing a short-lived node;
- online/offline operation and clear repository locking/lifecycle semantics;
- configuration inspection and mutation;
- safe defaults for API binding, authentication, and CORS.

**Architecture recommendation:** build one internal command/service layer and expose it through both the native CLI and HTTP RPC. Avoid implementing business logic separately in each transport. Keep the library API usable directly by Rust applications.

The HTTP RPC API is a compatibility surface: document endpoint behavior, argument names, streaming responses, error shapes, and command parity. Do not label it Kubo-compatible until tested against real Kubo clients.

### P1 — Everyday content workflows

Expose the existing UnixFS APIs through CLI and RPC:
- `ipfs cat`
- `ipfs get`
- `ipfs ls`
- `ipfs resolve`

Requirements:
- support IPFS paths and IPNS paths where applicable;
- preserve expected exit codes and stdout/stderr behavior;
- support files and directories, including large/streamed content;
- test local retrieval and network retrieval;
- verify path resolution, error handling, and output compatibility with Kubo.

### P2 — Content and repository lifecycle

Expose and validate:
- `ipfs pin add`, `pin rm`, `pin ls`, `pin verify`, `pin update`;
- `ipfs block get`, `block put`, `block rm`, `block stat`;
- `ipfs repo stat`, `repo gc`, `repo version`, `repo verify`;
- `ipfs refs` and `ipfs refs local`.

The library already has pin operations, block operations, local refs, and garbage collection. The remaining work includes command behavior, argument compatibility, output formatting, persistence guarantees, and tests.

Do not promise exact Kubo repository statistics or verification semantics until the storage model supports them.

### P3 — Node and network administration

Expose the existing network APIs through a coherent command surface:
- `ipfs id`, `ipfs version`;
- `ipfs bootstrap list/add/rm`;
- `ipfs swarm peers/connect/disconnect/addrs`;
- `ipfs routing findpeer/findprovs`;
- `ipfs bitswap wantlist` and supported diagnostics;
- `ipfs dht` operations supported by the library;
- `ipfs pubsub sub/pub/ls/peers`.

Also define which Kubo commands are intentionally unsupported, and report unsupported operations clearly instead of returning misleading success.

### P4 — Advanced content and filesystem workflows

Expose:
- `ipfs dag put/get/resolve` and other supported DAG operations;
- `ipfs dag export` / `dag import` or compatible CAR commands;
- `ipfs files mkdir/write/read/ls/stat/rm/cp/mv` for MFS;
- streaming and large-file handling.

The codebase already has CAR import/export and a substantial MFS API. Confirm exact Kubo argument and serialization behavior before declaring parity.

### P5 — Names, keys, and long-lived IPNS

Expose and complete:
- `ipfs name publish` and `name resolve`;
- `ipfs key gen/list/import/export/rename/rm`, subject to key-management APIs and security review;
- IPNS record validity, sequence numbers, validity periods, and key selection;
- an IPNS republisher lifecycle service.

**Important:** a one-shot IPNS publish API is not equivalent to Kubo's ongoing republishing behavior. The Kubo reference behavior periodically republishes records so they remain discoverable. The implementation must define scheduling, shutdown/restart behavior, persisted state, retry/backoff, and observability. Verify current Kubo defaults against the version used for compatibility tests rather than hardcoding undocumented assumptions.

The current IPNS module also contains a TODO concerning IPNS PubSub. Track that separately from basic publish/resolve.

### P6 — Services offered to other nodes and applications

Keep inbound servers distinct from outbound clients.

#### HTTP Gateway server

The current optional gateway feature is a **retrieval fallback client** that can use external trustless HTTP gateways. It is not a server that exposes this node's content over HTTP.

A separate HTTP Gateway server is needed for Kubo-like serving behavior:
- `/ipfs/{cid}/...` and `/ipns/{name}/...`;
- path-style and subdomain-style addressing as supported;
- UnixFS directory/index behavior;
- correct content types, caching, conditional requests, and range requests;
- trustless gateway behavior and relevant specifications;
- security controls and host/origin handling.

#### Delegated Routing V1 server

The current optional delegated-routing feature configures external routers for provider discovery. It is a client, not a server.

A separate server implementation is needed if this node is to offer delegated-routing endpoints to other nodes. Specify and test provider lookup and supported IPNS routing behavior against the relevant protocol specification.

### P7 — Complete `add` compatibility

The current CLI intentionally implements only a subset of Kubo's `add` surface. Track at least:
- stdin;
- symlink behavior and dereferencing policy;
- ignore/exclude rules;
- progress reporting;
- advanced UnixFS import settings;
- chunker configuration;
- raw leaves and CID version interactions;
- metadata such as mode and mtime where supported;
- wrapping and path semantics;
- quiet/quieter output and exit behavior.

The existing `--only-hash` mode and `--virtual-path` extension are useful Rust-IPFS capabilities. Preserve their documented semantics:
- hash-only computes the content address without persisting blocks, pinning, or announcing providers;
- virtual-path computes the UnixFS address for a file as if placed at a chosen path in a directory tree.

Do not conflate these extensions with Kubo flags unless there is an explicit compatibility mapping.

## 4. Repository compatibility and migration

Content-address compatibility and repository-layout compatibility are separate concerns.

A matching CID does not imply that two implementations can open each other's local repository directories. The Rust-IPFS storage implementation must not claim drop-in compatibility with a Kubo repository without a dedicated format audit and tests.

Recommended migration/interchange strategy:
1. prioritize interoperable content formats such as CAR;
2. implement explicit import/export workflows;
3. document what metadata and pin state are preserved or lost;
4. consider a dedicated migration tool only after the source and target formats are specified;
5. never point one implementation at another implementation's repository path as an informal migration method.

Test cross-implementation interoperability with real Kubo fixtures and binaries.

## 5. Compatibility and quality gates

For every command/API added:
- define the supported Kubo version and exact compatibility scope;
- compare flags, defaults, output, exit codes, errors, and streaming behavior;
- test CID equality for equivalent UnixFS import profiles;
- test content retrieval across implementations;
- test daemon restart and repository persistence;
- test offline and networked behavior;
- test feature-disabled builds;
- test Linux, Windows, macOS, Android/Termux, and Cygwin according to the project's supported target matrix;
- document intentional differences.

A command is not complete merely because it compiles or has a similarly named Rust method.

## 6. Suggested implementation sequence

1. Define the node lifecycle and repository initialization model.
2. Build the internal command/service layer.
3. Implement daemon lifecycle and HTTP RPC foundation.
4. Add the everyday UnixFS read commands.
5. Add pin/block/repo/refs commands.
6. Add node/network administration commands.
7. Expose MFS, DAG, and CAR workflows.
8. Complete IPNS/key management and republishing.
9. Implement inbound HTTP Gateway and delegated-routing servers.
10. Close the `add` compatibility gaps and expand cross-implementation tests.

This sequence deliberately prioritizes a controllable persistent node before advanced serving features.

## 7. Non-goals and guardrails

- Do not reimplement library capabilities without first checking whether an API already exists.
- Do not claim full Kubo parity from command-name overlap.
- Do not treat outbound gateway retrieval as an inbound Gateway server.
- Do not treat delegated-routing client support as a delegated-routing server.
- Do not treat one-shot IPNS publishing as a republishing service.
- Do not claim repository-layout compatibility based only on matching CIDs.
- Do not add placeholder commands that report success without performing the operation.
- Keep unfinished work in this roadmap or a linked TODO, with explicit blockers and acceptance criteria.

## 8. Primary code locations inspected

- `README.md` — current CLI scope and documented `add` limitations.
- `src/lib.rs` — public node APIs for UnixFS, DAG, pins, blocks, peer/network operations, DHT, Bitswap, PubSub, bootstrap, and IPNS.
- `src/builder.rs` — node construction and feature-gated network behavior.
- `src/mfs.rs` — Mutable File System operations.
- `src/ipns/mod.rs` — IPNS resolution and publication; contains an IPNS PubSub TODO.
- `src/dag/car.rs` — CAR import/export.
- `src/pinning.rs` — remote pinning client operations.
- `src/refs.rs` — IPLD reference traversal.
- `src/bin/ipfs/native.rs` — current native CLI implementation.

## 9. Reference material

Use the official Kubo documentation and protocol specifications as living references:
- Kubo repository: https://github.com/ipfs/kubo
- Kubo RPC API: https://docs.ipfs.tech/reference/kubo/rpc/
- Kubo CLI reference: https://docs.ipfs.tech/reference/kubo/cli/
- IPFS HTTP Gateway: https://specs.ipfs.tech/http-gateways/
- Delegated Routing V1: https://specs.ipfs.tech/routing/http-routing-v1/
- Content Addressable aRchives (CAR): https://ipld.io/specs/transport/car/
- IPNS specification: https://specs.ipfs.tech/ipns/

Re-check defaults and behavior against the exact Kubo release selected for compatibility testing. Documentation and protocol specifications evolve.


## 10. Iroh-compatible whole-file CID

The native CLI includes a deliberate non-UnixFS content-addressing operation:

```sh
ipfs cid --iroh-hash ./large-file.bin
```

The operation:

1. streams the complete file through BLAKE3-256;
2. wraps the 32-byte digest as a multihash using code `0x1e`;
3. emits a CIDv1 with the `raw` codec (`0x55`).

The BLAKE3 digest is the same content identity used by Iroh blobs. The CID is an interoperable Multiformats encoding of that digest, not Iroh's native hash-string representation. Iroh's verified transfer format (Bao) is a transport/verification layer and is not embedded in this CID.

This must remain separate from `ipfs add --only-hash`: UnixFS import chunking and DAG layout produce a different root identity. This command does not start an IPFS node, create UnixFS nodes, store blocks, pin content, or announce providers. It accepts a regular file and hashes it in bounded memory.

The output identifies the whole file as one `raw` block. A CID can be computed for files of arbitrary size, but this alone does not make an oversized raw block Bitswap-compatible. Bitswap requires implementations to support blocks up to 2 MiB and discourages larger blocks. For large-content distribution, use Iroh's blob transfer or an IPFS UnixFS DAG with appropriately sized blocks.

The BLAKE3 multicodec code `0x1e` is currently listed as draft in the Multicodec table. Consumers that do not implement this code may reject or fail to interpret the CID.

This is a Rust-IPFS interoperability extension, not a claim of Kubo CLI parity.
