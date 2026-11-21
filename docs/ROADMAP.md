# Rust-IPFS Roadmap

> Status: planning document for the Floflis fork. The project README describes the current alpha state and available native CLI packaging; this file tracks forward work separately from those existing capabilities.

## Direction

Continue the Rust IPFS implementation with a focus on interoperability, correctness, resource-conscious operation, and a dependable native CLI. Preserve the crate-level architecture and make support claims only when validated on the relevant target.

## Workstreams

### 1. Core protocol and interoperability
- [ ] Maintain an interoperability matrix for content addressing, block exchange, provider/content discovery, and pubsub behavior.
- [ ] Add integration tests against independent IPFS implementations where practical.
- [ ] Document protocol gaps, deviations, and feature maturity.

### 2. Storage and data integrity
- [ ] Expand blockstore tests for persistence, corruption, concurrent access, and recovery.
- [ ] Verify CID and multihash handling with shared test vectors.
- [ ] Document storage backends, durability guarantees, and configuration trade-offs.

### 3. Networking and discovery
- [ ] Validate libp2p transport and discovery behavior under peer churn and network partitions.
- [ ] Add tests for DHT/provider discovery and pubsub lifecycle.
- [ ] Document bootstrap, connectivity, and resource-limit configuration.

### 4. Native CLI
- [ ] Keep CLI commands and flags documented with executable examples.
- [ ] Add end-to-end tests for add, CID operations, and other shipped commands.
- [ ] Ensure CLI behavior is consistent across supported operating systems and architectures.

### 5. Build, packaging, and portability
- [ ] Keep release artifacts reproducible and publish checksums for every release asset.
- [ ] Validate each target independently in CI and record toolchain/system prerequisites.
- [ ] Expand targets only with successful build and runtime evidence; distinguish cross-compilation from on-device validation.

### 6. Quality and security
- [ ] Maintain formatting, lint, unit-test, and integration-test gates.
- [ ] Audit parsing, network input, resource exhaustion, and dependency updates.
- [ ] Publish release notes with clear alpha/stability and compatibility statements.

## Completion policy

A roadmap item is complete only when the implementation is merged and the relevant tests or target-specific validation are linked. A successful build alone does not establish runtime interoperability.
