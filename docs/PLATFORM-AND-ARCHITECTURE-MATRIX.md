# Platform and architecture matrix

**Status:** build/export roadmap. The existing `.github/workflows/export-binaries.yml` remains the single source of truth for release artifact production. This document does not claim that every listed target is already buildable.

## Existing export coverage

The current export workflow builds standalone binaries for:

- `x86_64-unknown-linux-gnu`
- `x86_64-pc-windows-msvc`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `aarch64-linux-android`

It also runs a Cygwin smoke test against the Windows executable. Extend this workflow rather than creating a competing release publisher.

## Expansion matrix

| OS / environment | Target triple | Intended evidence | Status |
|---|---|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu` | Native build, package, smoke test | Existing export |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | Native ARM64 build and test | Candidate |
| Linux ARMv7 hard-float | `armv7-unknown-linux-gnueabihf` | Cross-build, QEMU/native smoke test | Candidate |
| Linux RISC-V 64 | `riscv64gc-unknown-linux-gnu` | Cross-build, QEMU/native smoke test | Candidate |
| Linux musl x64 / ARM64 | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | Static-link validation | Candidate |
| Windows x64 | `x86_64-pc-windows-msvc` | Native build and smoke test | Existing export |
| Windows ARM64 | `aarch64-pc-windows-msvc` | Native ARM64 runner and smoke test | Candidate |
| macOS Intel / Apple Silicon | `x86_64-apple-darwin`, `aarch64-apple-darwin` | Native build per architecture | Existing export |
| Android ARM64 | `aarch64-linux-android` | NDK build and real-device smoke test | Existing export |
| Android ARMv7 | `armv7-linux-androideabi` | NDK build and device test | Candidate |
| FreeBSD amd64 | `x86_64-unknown-freebsd` | BSD VM build and test | Research |
| FreeBSD ARM64 | `aarch64-unknown-freebsd` | Native/cross build and test | Research |
| NetBSD amd64 / RISC-V | `x86_64-unknown-netbsd`, `riscv64gc-unknown-netbsd` | NetBSD VM/native RISC-V | Research |
| OpenBSD amd64 / RISC-V | `x86_64-unknown-openbsd`, `riscv64gc-unknown-openbsd` | OpenBSD VM/native RISC-V | Research |
| DragonFly BSD x64 | `x86_64-unknown-dragonfly` | DragonFly VM | Research |
| Redox x64 | `x86_64-unknown-redox` | redoxer + QEMU | Research |
| Redox ARM64 / RISC-V 64 | `aarch64-unknown-redox`, `riscv64gc-unknown-redox` | Custom toolchain + QEMU/native hardware | Experimental research |

## Crate-specific compatibility gates

Rust-IPFS is a workspace, not a single dependency-free executable. A target is supported only when the selected package and feature set compile together.

Review these areas per target:

- TCP, QUIC, WebSocket and WebRTC transports have different native and platform requirements.
- TLS, Noise, RSA, Ed25519, secp256k1 and ECDSA features may pull different native/build dependencies.
- DNS, UPnP, filesystem storage and OS-specific interfaces need platform-specific checks.
- The default `full` feature set is a stricter gate than a reduced-feature library build. Never describe a reduced-feature success as a full-node build.
- The workspace's Rust version requirement must be honored by every runner.

## CI rules

1. Keep the current export workflow as the only publisher of release assets.
2. Use native runners for macOS and Windows release builds; cross-compilation alone is not runtime validation.
3. Use ARM64 Linux runners for ARM64 Linux artifacts where available.
4. For Linux ARMv7/RISC-V, use a pinned cross toolchain and record whether the job performs `cargo check`, full link, or runtime execution.
5. Use BSD and Redox VMs/self-hosted runners for OS-specific tests. GitHub-hosted Ubuntu runners do not constitute BSD/Redox runtime evidence.
6. Do not make Tier 3 targets release blockers. Report unavailable toolchains as experimental/unverified, not as passing.
7. Upload checksums and target metadata alongside each published binary.

## Evidence levels

- **Toolchain:** Rust target can be installed or built.
- **Cross-check:** selected Cargo package/features pass `cargo check`.
- **Linked build:** the executable links successfully for the target.
- **Runtime:** executable starts and passes smoke tests on native hardware or a documented VM.
- **Release:** clean-environment packaging and checksum verification pass.

## First expansion order

1. Linux ARM64 native build.
2. Linux ARMv7 and RISC-V cross-checks.
3. Android ARMv7 NDK build, preserving the existing ARM64 lane.
4. Windows ARM64 native build.
5. FreeBSD and NetBSD x64 VM experiments.
6. OpenBSD, DragonFly and Redox x64 VM experiments.
7. Redox RISC-V 64 and the other Tier 3 architecture targets, with a custom toolchain and explicit runtime evidence.
