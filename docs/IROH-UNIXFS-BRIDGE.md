# Iroh ↔ UnixFS: Integration Research and Architecture

> **Status:** Research and architecture proposal. No Iroh bridge implementation is included in this change.
>
> **Research snapshot:** 2026-11-19  
> **Repository baseline:** \`libp2p-next\` at \`a08b2b1a869b059ed1ee618d8e9773d8f15897da\`

## 1. Executive summary

Rust-IPFS can use Iroh as a high-performance transport for existing IPFS content without replacing UnixFS, changing existing CIDs, or pretending that Iroh is an IPFS Mainnet node.

The recommended division of responsibility is:

- **Rust-IPFS owns IPFS meaning:** CIDs, codecs, UnixFS, DAG traversal, repository persistence, CAR import/export, pins, and IPFS-facing CLI/API behavior.
- **Iroh owns transport:** authenticated endpoint connections, QUIC, BLAKE3-verified blob transfer, resumability, and Bao streaming.
- **A bridge owns the translation:** mapping each original IPFS block identity to its Iroh BLAKE3 blob identity, preserving the graph, and verifying both identities at the receiver.

The most important distinction is between two identifiers:

1. The **original CID** identifies an IPFS block and includes its version, codec, and multihash.
2. The **Iroh blob hash** identifies the serialized bytes of that block using BLAKE3.

A UnixFS root CID remains the root CID throughout the transfer. Iroh hashes are transport/storage identities for the individual blocks, not replacements for the UnixFS root.

This architecture is compatible with the direction described by Iroh's own DAG-sync experiment: preserve arbitrary IPLD hashes, map them to BLAKE3 blobs, traverse deterministically, and validate the original hash after receiving the bytes. The experiment is useful precedent, not production-ready code to copy unchanged.

## 2. Why a bridge is necessary

### 2.1 Iroh is intentionally not a drop-in IPFS transport

Iroh began as a Rust IPFS implementation. Its maintainers later chose a narrower architecture after finding that the compatibility-oriented implementation did not meet their performance, dependency-size, and reliability goals for their primary use cases.

The current Iroh direction is built around QUIC, public-key-based endpoint identity, and BLAKE3-verified streaming. It deliberately moved away from IPFS multiaddresses, multihashes, and the goal of remaining compatible with existing IPFS implementations. The earlier IPFS-oriented work continued separately as Beetle.

This is an intentional architectural boundary, not an accidental missing feature. Rust-IPFS should not try to make Iroh behave like Kubo internally. It should build a clear interoperability layer around the parts each project does well.

Sources:
- [The road to Iroh 1.0](https://www.iroh.computer/blog/the-road-to-iroh-1-0)
- [Iroh as an IPFS-compatible library: maintainer discussion](https://github.com/n0-computer/iroh/discussions/955)

### 2.2 IPFS documentation describes the same interoperability gap

The IPFS implementations documentation currently places Iroh under **Limited Mainnet Interop**. It describes Iroh as a QUIC- and BLAKE3-based peer-to-peer library and points out that it does not participate in the normal IPFS Mainnet transport/protocol stack, including Bitswap, Amino DHT, and UnixFS.

That classification is a technical description in the IPFS documentation, not a formal statement that the projects should not interoperate. It does establish that merely sharing CIDs or using BLAKE3 does not make Iroh and IPFS nodes members of the same swarm.

Source:
- [IPFS implementations](https://docs.ipfs.tech/concepts/ipfs-implementations/)

### 2.3 Iroh maintainers have acknowledged the bridge problem

In a 2023 discussion about using Iroh as an IPFS-compatible library, an Iroh maintainer acknowledged that the division between Iroh and Kubo was a significant barrier. The stated direction was to address interoperability with documentation, utility crates, and CLI tools, while keeping Iroh's core focused on doing one thing well.

That is a useful design constraint for Rust-IPFS: interoperability should be explicit and layered rather than forcing IPFS-specific behavior into Iroh's core abstractions.

Source:
- [Iroh discussion #955](https://github.com/n0-computer/iroh/discussions/955)

## 3. The data models are related, but not interchangeable

Both UnixFS and Iroh Collections can represent collections of content using content-addressed structures. Their semantics differ.

| Property | UnixFS | Iroh Collections |
|---|---|---|
| Core representation | Hierarchical IPLD DAG | Flat path-to-BLAKE3-hash collection |
| Block/metadata encoding | DAG-PB plus UnixFS protobuf metadata; raw leaves are common | Postcard-based collection metadata |
| Directory structure | Actual directory nodes and links | Relative paths in a flat collection |
| Subdirectory identity | A directory/subtree can have its own CID | No equivalent native CID for each folder |
| Retrieval | Traverse links and retrieve a subtree | Filter paths and fetch matching file blobs |
| Ecosystem | IPFS and UnixFS implementations | Iroh ecosystem |

An Iroh Collection is not a replacement encoding for an existing UnixFS DAG. Converting UnixFS to a flat collection changes the representation and loses the original subtree-addressing semantics. It may be useful as an additional export format, but must not silently replace the original DAG or root CID.

The IPFS comparison documentation also notes that Iroh Collection roots can be represented in CID form using Iroh-related multicodecs. That is a representation convention for an Iroh Collection, not a UnixFS root CID.

Sources:
- [IPFS: Content addressing data sets](https://docs.ipfs.tech/how-to/content-addressing-data-sets/)
- [Iroh protocols](https://www.iroh.computer/proto)

## 4. A concrete precedent: iroh-dag-sync

The N0 Computer repository [iroh-experiments](https://github.com/n0-computer/iroh-experiments) contains an experiment named [iroh-dag-sync](https://github.com/n0-computer/iroh-experiments/tree/main/iroh-dag-sync). Its stated goal is to bridge Iroh Blobs and IPFS DAGs, including DAGs whose CIDs do not use BLAKE3.

The experiment demonstrates an architecture close to what Rust-IPFS needs:

1. Create an IPFS DAG and export it as a CAR.
2. Import the CAR into an Iroh blob store.
3. Keep a persistent mapping from the original IPLD multihash to the corresponding BLAKE3 hash.
4. Cache the outgoing links for each block, keyed by its IPLD codec and BLAKE3 identity.
5. Synchronize a deterministic DAG traversal over a custom Iroh protocol.
6. Transfer block data through Bao-encoded streams.
7. On receipt, verify the BLAKE3 transfer identity and recompute the original CID multihash from the received bytes.
8. Export the reconstructed DAG as a CAR.

The experiment's README describes depth-first, pre-order, left-to-right traversal and options for sending all blocks, only DAG structure, or selected blocks inline. The protocol uses a custom ALPN and Postcard-encoded control messages. Its transfer design reduces round trips compared with fetching each level of a deep DAG separately.

### 4.1 What should be reused conceptually

- The two-identity model: original IPLD hash ↔ Iroh BLAKE3 hash.
- Persistent mapping tables rather than pretending the Iroh blob store contains the original DAG structure.
- Deterministic traversal so sender and receiver agree on which block is next.
- Bao streaming for data transfer and incremental BLAKE3 validation.
- Revalidation of the original CID multihash before accepting the block.
- CAR at the import/export boundary.

### 4.2 Why the experiment must not be copied as-is

The N0 repository explicitly describes itself as experimental, low-level, and unpolished. Its current \`iroh-dag-sync\` manifest also pins pre-GA Iroh dependencies, including \`iroh = 1.0.0-rc.1\` and \`iroh-blobs = 0.102\`. Iroh 1.0 has since been released, so dependency and API compatibility must be audited before any port.

There is a more important correctness issue for UnixFS: the current experiment's CAR import path and link-cache insertion call \`DagCborCodec::links\` for blocks without dispatching on the block's actual codec. That is not a correct general-purpose UnixFS traversal. A UnixFS CAR normally contains DAG-PB nodes and raw leaves, not exclusively DAG-CBOR nodes.

Rust-IPFS already has codec-aware block decoding. The bridge should build on that implementation rather than copy the prototype's DAG-CBOR-only extraction path.

Relevant source:
- [Experiment README](https://github.com/n0-computer/iroh-experiments/blob/main/iroh-dag-sync/README.md)
- [Experiment import and command entry point](https://github.com/n0-computer/iroh-experiments/blob/main/iroh-dag-sync/src/main.rs)
- [Experiment link tables](https://github.com/n0-computer/iroh-experiments/blob/main/iroh-dag-sync/src/tables.rs)
- [Experiment sync protocol](https://github.com/n0-computer/iroh-experiments/blob/main/iroh-dag-sync/src/protocol.rs)
- [Experiment sync implementation](https://github.com/n0-computer/iroh-experiments/blob/main/iroh-dag-sync/src/sync.rs)
- [Iroh: DAG sync design article](https://www.iroh.computer/blog/iroh-dag-sync)

## 5. Rust-IPFS capabilities that should anchor the bridge

The inspected Rust-IPFS baseline already contains building blocks that should be reused:

- \`src/block.rs\` defines the block abstraction and codec-aware decoding for Raw, DAG-CBOR, DAG-JSON, and DAG-PB blocks.
- \`Block::to_ipld()\` decodes according to the CID codec.
- \`Block::references()\` extracts IPLD references from the decoded block.
- The UnixFS implementation has its own traversal and file reconstruction logic under \`unixfs/\`.
- CAR functionality is exposed under \`src/dag/car.rs\`.

The bridge should use the generic block/DAG abstraction to move complete DAGs, and the UnixFS-aware traversal when file- or directory-specific semantics are needed. These are related layers, not interchangeable ones.

Before implementing support for a codec, check whether Rust-IPFS can decode it and enumerate links correctly. Unknown codecs must produce an explicit unsupported-codec error. They must never be guessed to be DAG-CBOR.

Code inspected at baseline commit:
- [src/block.rs](https://github.com/Floflis/rust-ipfs/blob/a08b2b1a869b059ed1ee618d8e9773d8f15897da/src/block.rs)
- [src/dag.rs](https://github.com/Floflis/rust-ipfs/blob/a08b2b1a869b059ed1ee618d8e9773d8f15897da/src/dag.rs)
- [src/dag/car.rs](https://github.com/Floflis/rust-ipfs/tree/a08b2b1a869b059ed1ee618d8e9773d8f15897da/src/dag)
- [UnixFS implementation](https://github.com/Floflis/rust-ipfs/tree/a08b2b1a869b059ed1ee618d8e9773d8f15897da/unixfs)

## 6. Proposed architecture

### 6.1 Preserve the original DAG

The sender starts from an existing IPFS root CID and walks the reachable graph.

For each block:

1. Load the exact serialized block bytes from the Rust-IPFS blockstore or CAR importer.
2. Retain its original full CID, including version, codec, and multihash.
3. Compute the BLAKE3 hash of those exact serialized bytes.
4. Store or reuse the BLAKE3 blob in the Iroh store.
5. Extract outgoing links by decoding the block according to its actual codec.
6. Continue in a deterministic traversal order.

The original CID and block bytes remain authoritative for IPFS. The BLAKE3 identity is an additional transport/store index.

**Do not re-encode a block before calculating its original CID.** The CID commits to the exact serialized bytes, not merely to an equivalent in-memory IPLD value.

### 6.2 Persistent mapping and link index

The bridge should maintain persistent metadata alongside the Iroh blob store. A practical logical schema is:

| Mapping | Purpose |
|---|---|
| Full original CID → BLAKE3 hash | Find the Iroh blob for an exact IPFS block |
| (codec, BLAKE3 hash) → ordered child CIDs | Cache codec-aware link extraction |
| BLAKE3 hash → blob metadata | Reuse Iroh store metadata, size, and availability |

Using the full original CID as the primary mapping key avoids conflating blocks that have the same multihash bytes but different CID codecs or versions. A lower-level multihash index can still be added for deduplication, but it must not erase the full CID identity.

Only insert a CID-to-BLAKE3 mapping after validating that the exact block bytes actually produce the original CID multihash. Never trust a mapping received from a remote peer without validating the block data.

Link-cache entries must preserve the codec's meaningful link order. Do not use randomized map iteration to determine wire traversal order. If traversal order is normalized, define and test that rule explicitly.

### 6.3 Deterministic, DAG-aware synchronization

The receiver requests a root CID and a traversal policy. Both sides execute the same deterministic traversal. The sender streams blocks in traversal order over one custom Iroh protocol exchange, minimizing request/response round trips for deep DAGs.

The first version should keep the traversal policy small and explicit:

- Full traversal from one root.
- Optional set of CIDs already present at the receiver.
- Stable traversal ordering.
- Optional filters for codecs or application-specific subsets.
- Bounded traversal and transfer limits.

The initial goal is complete DAG synchronization. Optimizations such as selective leaves, partial subtrees, and multi-provider scheduling can follow after correctness is established.

### 6.4 Transfer and validation

Each transferred block has two independent integrity checks:

1. **Iroh/BLAKE3 verification:** Bao verifies the bytes against the BLAKE3 blob identity while streaming.
2. **Original CID verification:** after receiving the complete block, Rust-IPFS recomputes the multihash specified by the original CID and compares it with the CID's multihash.

The receiver must also ensure that the block corresponds to the requested deterministic traversal. A valid BLAKE3 blob alone does not prove that it belongs to the requested DAG.

Only after validation should the receiver persist the block, original-CID mapping, and extracted links. Unsupported multihash functions, malformed blocks, codec mismatches, truncated streams, unexpected traversal items, and size-limit violations must fail closed.

Integrity verification is not application-level authorization. Endpoint identity, peer trust, access policy, and any application signatures remain separate concerns.

### 6.5 Wire format: binary control, Bao data

Use a versioned binary control message, such as Postcard, for the root CID, traversal options, negotiation, and frame metadata. Use Bao streams for block data.

JSON can be useful for diagnostics, logs, manifests, or a human-readable inspection command. It should not be the primary block-transfer wire format. Encoding every block as JSON would add framing and parsing overhead without improving the content-addressed integrity model.

The experiment's custom-ALPN approach is a sound precedent. Rust-IPFS should define and version its own ALPN, for example a project-specific DAG-sync protocol identifier, and document compatibility rules. Do not reuse another project's experimental ALPN.

### 6.6 CAR remains an interoperability boundary

CAR is still valuable:

- Import existing Kubo/IPFS DAGs into the bridge.
- Export synchronized DAGs for Kubo and other IPFS implementations.
- Create reproducible fixtures for testing.
- Provide an offline transfer or archive path.

The native Iroh sync path should be DAG-aware rather than treating a CAR as one opaque giant blob. A simple whole-CAR transfer using an Iroh file-transfer tool can remain an optional fallback for users who only need to move an archive and do not need incremental DAG synchronization.

## 7. CID and hash policy

### 7.1 Do not confuse a raw file CID with a UnixFS root CID

Rust-IPFS currently documents \`ipfs cid --iroh-hash FILE\` as BLAKE3-256 over the complete file byte stream, wrapped in a CIDv1 using the Raw codec.

That operation is deliberately not UnixFS: it does not chunk the file, build a DAG, or create a directory/file root node. It produces one raw whole-file identity.

For the bridge, BLAKE3 should instead be computed independently for each serialized DAG block. The original UnixFS root CID must remain the root used by IPFS.

The existing 10,000,000-byte rejection policy belongs to the standalone \`cid --iroh-hash\` command. It is not a BLAKE3 or Iroh limitation and must not be copied into the DAG mediator. The mediator transfers individual UnixFS blocks, not the entire file as one raw block.

### 7.2 IPIP-0499 must guide UnixFS CID profiles

IPIP-0499 defines named UnixFS import profiles to make CID generation deterministic across implementations. Its modern \`unixfs-v1-2025\` profile specifies:

- CIDv1;
- SHA2-256;
- fixed-size chunks up to 1 MiB;
- balanced DAG layout with width 1024;
- raw leaves;
- defined HAMT sharding and threshold behavior;
- deterministic field-order and metadata rules.

It does **not** define BLAKE3 as the hash function for that profile.

Therefore:

- Keep the canonical \`unixfs-v1-2025\` profile aligned with IPIP-0499.
- Do not describe a BLAKE3 UnixFS profile as IPIP-0499-compliant.
- If Rust-IPFS offers UnixFS DAGs hashed with BLAKE3, label the profile custom/experimental and document all import parameters.
- Do not invent or publish a new public CID convention without checking existing multicodec and IPIP work.

This does not block the bridge. Existing SHA2-256 UnixFS blocks can be transferred through Iroh because the bridge maps each original CID to the BLAKE3 hash of its serialized block bytes.

Sources:
- [IPIP-0499: UnixFS CID Profiles](https://specs.ipfs.tech/ipips/ipip-0499/)
- [UnixFS specification](https://specs.ipfs.tech/unixfs/)
- [Kubo import profiles](https://github.com/ipfs/kubo/blob/master/docs/config.md)

## 8. Discovery is a separate problem from transfer

A first implementation should connect peers using an explicit Iroh EndpointId and/or ticket. This keeps the first bridge focused on block transfer and DAG correctness.

Do not assume Iroh's content discovery is IPFS provider discovery. Iroh's 2026 Global Content Discovery work explores BitTorrent Mainline DHT and an address-index service to find candidate Iroh endpoints for BLAKE3 content. IPFS Mainnet uses different routing mechanisms, including Amino DHT. A provider-discovery adapter must be a separate module with an explicit mapping and trust model.

The separation is useful: a bridge can transfer content once it knows an endpoint, even before any cross-network provider discovery exists.

Source:
- [Iroh Global Content Discovery](https://www.iroh.computer/blog/iroh-global-content-discovery)

## 9. CLI and process integration

Iroh is primarily a Rust library plus separately maintained protocols. The upstream Iroh CLI was removed in the v0.28.1 release line; the Iroh roadmap directs users toward protocol-specific tools rather than a universal Iroh node CLI.

Consequently, a command such as \`ipfs iroh send\` cannot safely assume that an upstream generic \`iroh\` executable exists or understands Rust-IPFS DAG requests.

Recommended options:

1. **Built-in library integration:** an optional Rust-IPFS feature uses Iroh crates directly and exposes IPFS-native commands.
2. **Companion helper process:** a separately built \`rust-ipfs-iroh\` executable owns the Iroh endpoint and custom protocol. The main \`ipfs\` CLI invokes it through a documented, versioned process interface.
3. **Archive fallback:** an optional \`sendme\`-style path transfers a CAR as a file when DAG-aware sync is not required.

The companion helper is the cleanest fit if the CLI must keep Iroh in an external process while Rust-IPFS owns the mediator and user-facing command abstraction. Its process interface should be treated as a real protocol: version it, validate input/output, propagate exit codes, and test missing-helper and incompatible-version failures.

Avoid shell-string concatenation. Use structured process arguments and pass data through files, pipes, or a documented local IPC protocol. The default IPFS workflow must continue to work when the optional helper is not installed.

Sources:
- [Iroh 1.0 roadmap: CLI removal](https://www.iroh.computer/roadmap)
- [Iroh protocols](https://www.iroh.computer/proto)
- [sendme](https://www.iroh.computer/sendme)

## 10. Implementation plan

This document records research and architecture only. The following sequence is a proposed implementation plan, not a claim that these features already exist.

### Phase 0 — Establish compatibility fixtures

- Produce CAR fixtures from Kubo for small files, large chunked files, nested directories, empty directories, HAMT directories, and mixed codecs.
- Record the expected original root CID and each block CID.
- Include SHA2-256 and any explicitly supported non-default multihash cases.
- Keep fixtures reproducible and check them into tests or generate them through a pinned Kubo test environment.

### Phase 1 — Codec-aware bridge core

- Reuse Rust-IPFS block decoding and reference extraction.
- Define a deterministic traversal API with explicit unsupported-codec errors.
- Preserve ordered links where codec semantics require it.
- Add a persistent original-CID ↔ BLAKE3 mapping.
- Verify original multihashes before accepting mappings.

### Phase 2 — Iroh store adapter

- Integrate a compatible, stable Iroh/Iroh Blobs release.
- Store serialized IPFS blocks as Iroh blobs.
- Implement Bao-verified block transfer.
- Make metadata writes atomic with respect to block acceptance.
- Define cancellation, retries, resumability, and resource limits.

### Phase 3 — DAG sync protocol

- Define and version the custom ALPN.
- Implement deterministic traversal negotiation.
- Transfer a DAG in one stream-oriented exchange, with explicit framing and validation.
- Test interrupted transfers and resuming from blocks already present.

### Phase 4 — CLI and distribution

- Add IPFS-native commands for sending, receiving, and inspecting bridge state.
- Decide whether the default implementation is an optional in-process feature or the companion helper.
- Keep helper discovery and invocation explicit and testable.
- Document installation and platform availability separately from core Rust-IPFS support.

### Phase 5 — Discovery adapters

- Add explicit Iroh EndpointId/ticket workflows first.
- Evaluate Iroh Global Content Discovery as an optional provider source.
- Evaluate IPFS provider discovery separately.
- Do not claim cross-DHT interoperability until a tested adapter exists.

## 11. Required correctness tests

At minimum, the bridge must demonstrate:

- A Kubo-generated UnixFS CAR can be imported and synchronized without changing the original root CID.
- The receiver can export the result as CAR and Kubo can import/read it.
- Every block's BLAKE3 identity and original CID multihash are independently verified.
- DAG-PB UnixFS nodes are traversed as DAG-PB, not parsed as DAG-CBOR.
- Raw leaves are transferred as data blocks without attempting to parse them as linked nodes.
- DAG-CBOR and DAG-JSON references are handled according to their codecs.
- Unsupported codecs and unsupported multihash algorithms fail explicitly.
- Deep DAGs complete without a request/response round trip for every node.
- Duplicate blocks are deduplicated without corrupting distinct CID metadata.
- Unicode paths, nested directories, empty directories, and HAMT-sharded directories survive round trips.
- Interrupted transfers can resume without trusting unverified mappings.
- Resource limits prevent unbounded traversal, oversized control messages, and unbounded block allocation.

A successful BLAKE3 check alone is not a passing end-to-end test. The original root CID and DAG structure must also survive.

## 12. Current limitations and non-goals

This proposal does not:

- make Iroh a Bitswap or Amino DHT peer;
- claim that Iroh endpoints can directly retrieve arbitrary content from IPFS Mainnet;
- replace UnixFS with Iroh Collections;
- change the CID of an existing UnixFS DAG;
- define a new standardized BLAKE3 UnixFS profile;
- promise that the experimental N0 DAG-sync repository is production-ready;
- implement the bridge, protocol, helper binary, provider discovery, or CLI commands.

The first milestone is a correct, testable block/DAG bridge with explicit endpoint addressing. Broader discovery and convenience features come later.

## 13. References

### Iroh

- [The road to Iroh 1.0](https://www.iroh.computer/blog/the-road-to-iroh-1-0)
- [Iroh and DAGs](https://www.iroh.computer/blog/iroh-dag-sync)
- [Iroh protocols](https://www.iroh.computer/proto)
- [Iroh Global Content Discovery](https://www.iroh.computer/blog/iroh-global-content-discovery)
- [Iroh 1.0 roadmap](https://www.iroh.computer/roadmap)
- [Iroh as an IPFS-compatible library discussion](https://github.com/n0-computer/iroh/discussions/955)
- [N0 Iroh experiments](https://github.com/n0-computer/iroh-experiments/tree/main/iroh-dag-sync)

### IPFS and UnixFS

- [IPFS implementations](https://docs.ipfs.tech/concepts/ipfs-implementations/)
- [IPFS content addressing data sets](https://docs.ipfs.tech/how-to/content-addressing-data-sets/)
- [UnixFS specification](https://specs.ipfs.tech/unixfs/)
- [IPIP-0499: UnixFS CID Profiles](https://specs.ipfs.tech/ipips/ipip-0499/)
- [Kubo import profiles](https://github.com/ipfs/kubo/blob/master/docs/config.md)

### Rust-IPFS baseline

- [Rust-IPFS block abstraction](https://github.com/Floflis/rust-ipfs/blob/a08b2b1a869b059ed1ee618d8e9773d8f15897da/src/block.rs)
- [Rust-IPFS DAG implementation](https://github.com/Floflis/rust-ipfs/blob/a08b2b1a869b059ed1ee618d8e9773d8f15897da/src/dag.rs)
- [Rust-IPFS repository](https://github.com/Floflis/rust-ipfs/tree/libp2p-next)
