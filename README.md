<h1>
  <img src="https://ipfs.io/ipfs/QmRcFsCvTgGrB52UGpp9P2bSDmnYNTAATdRf4NBj8SKf77/rust-ipfs-logo-256w.png" width="128" /><br />
  Rust IPFS
</h1>

> The Interplanetary File System (IPFS), implemented in Rust
## Table of Contents

- [Description](#description)
    - [Project Status](#project-status---alpha)
- [Getting Started](#getting-started)
    - [Running the tests](#running-the-tests)
    - [Contributing](#contributing)
- [Roadmap](#roadmap)
    - [Completed](#completed-work)
    - [In progress](#work-in-progress)
    - [Still required](#work-still-required)
- [Maintainers](#maintainers)
- [Alternatives](#alternatives-and-other-cool-related-projects)
- [Contributors](#contributors)
- [License](#license)
- [Trademarks](#trademarks)

## Description

This repository is a fork of [rust-ipfs](https://github.com/rs-ipfs/rust-ipfs), which contains the crates for the IPFS core implementation which includes a blockstore, a libp2p integration which includes DHT content discovery and pubsub support Our goal is to leverage both the unique properties of Rust to create powerful, performant software that works even in resource-constrained environments, while also maximizing interoperability with the other "flavors" of IPFS, namely JavaScript and Go.

### Project Status - `Alpha`

This project is a WIP and everything is subject to change

For more information about IPFS see: https://docs.ipfs.io/introduction/overview/

## Getting started

We recommend browsing the [examples](https://github.com/dariusc93/rust-ipfs/tree/libp2p-next/examples) and [tests](https://github.com/dariusc93/rust-ipfs/tree/libp2p-next/tests) in order to see how to use Rust-IPFS in different scenarios.

**Note: Test are a WIP**


### Native CLI

The workspace now builds both the `rust-ipfs` library and a native `ipfs` executable. The first CLI surface is focused on adding local files and directories.

```bash
# Add one file to the local repository.
cargo run --bin ipfs -- add ./asset.bin

# Recursively add a directory and print one CID per imported file/directory node.
cargo run --bin ipfs -- add -qr ./website/

# Print only the final CID, without storing or pinning the imported blocks.
cargo run --bin ipfs -- add -Q -n ./asset.bin

# Select CIDv1 with SHA2-256, or BLAKE3 (which requires CIDv1).
cargo run --bin ipfs -- add --cid-version 1 ./asset.bin
cargo run --bin ipfs -- add --hash blake3 ./asset.bin
```

The output flags follow Kubo's `ipfs add` conventions:

- `-q` / `--quiet`: print the CID for each imported file or directory node.
- `-Q` / `--quieter`: print only the final CID.
- `-r` / `--recursive`: traverse directory inputs recursively. `-qr` combines quiet output with recursive import.
- `-n` / `--only-hash`: compute the UnixFS DAG without storing blocks, pinning, or providing them.
- `-w` / `--wrap-with-directory`: wrap a file in a directory object.
- `--pin=false`: do not pin imported content.

The CLI defaults to CIDv0 + SHA2-256, matching Kubo's default import profile. BLAKE3 automatically selects CIDv1. `IPFS_PATH` or `--repo` selects the repository directory; otherwise the CLI uses `~/.rust-ipfs`.

#### Virtual paths

`--virtual-path` is a Rust-IPFS CLI extension. It answers a different question from hashing the file by itself: *what would the address be if this file were located at a chosen relative path inside a UnixFS directory tree?*

```bash
cargo run --bin ipfs -- add \
  --virtual-path collection/metadata.json \
  ./metadata.json
```

This mode implies `--only-hash`, so it does not persist blocks. Its normal output includes the computed route:

```text
added <directory-cid> /ipfs/<directory-cid>/collection/metadata.json
```

The virtual path may contain multiple directory segments. Absolute paths, `.` and `..` segments are rejected. The file bytes are hashed normally; the directory nodes are then built only in memory. This is useful for reproducible NFT metadata layouts, manifests, and release trees before committing to a final directory structure.

#### Rust API: hash-only CID examples

`UnixfsAdd::only_hash()` is the library API behind the CLI's `-n` mode. It does not save blocks, pin content, or announce providers. The returned path is content-addressed, but it is not locally retrievable unless its blocks are stored separately.

```rust
use std::path::Path;

use futures::StreamExt;
use ipld_core::cid::Version;
use multihash_codetable::Code;
use rust_ipfs::{Ipfs, unixfs::UnixfsStatus};

async fn hash_only(
    ipfs: &Ipfs,
    file: &Path,
    version: Version,
    hasher: Code,
) -> anyhow::Result<String> {
    let mut add = ipfs
        .add_unixfs(file)
        .cid_version(version)
        .hasher(hasher)
        .only_hash();

    while let Some(status) = add.next().await {
        match status {
            UnixfsStatus::ProgressStatus { .. } => {}
            UnixfsStatus::FailedStatus { error, .. } => return Err(error.into()),
            UnixfsStatus::CompletedStatus { path, .. } => return Ok(path.to_string()),
        }
    }

    anyhow::bail!("UnixFS add ended without a completion event")
}

// CIDv0 requires SHA2-256.
let cidv0 = hash_only(
    &ipfs,
    Path::new("./asset.bin"),
    Version::V0,
    Code::Sha2_256,
).await?;

// CIDv1 with SHA2-256.
let cidv1 = hash_only(
    &ipfs,
    Path::new("./asset.bin"),
    Version::V1,
    Code::Sha2_256,
).await?;

// CIDv1 with BLAKE3.
let blake3 = hash_only(
    &ipfs,
    Path::new("./asset.bin"),
    Version::V1,
    Code::Blake3_256,
).await?;
```

The initial CLI intentionally implements a subset of Kubo's `add` surface: local regular files/directories, CID version, SHA2-256/BLAKE3, quiet/quieter output, recursion, hash-only mode, wrapping, and pin control. Stdin, symlink dereferencing, ignore rules, progress bars, and advanced UnixFS import settings are not implemented yet.

### Running the tests


**For information on running test, please see the [archived readme](./archived/README.md). This may be outdated but this section will be updated in the future** 


### Contributing

See [the contributing docs](./CONTRIBUTING.md) for more info.

If you have any questions on the use of the library or other inquiries, you are welcome to submit an issue.

## Roadmap

### Completed API Work

- [ ] Pubsub

**For previous completed work, please see the [archived readme](./archived/README.md).**

## Maintainers

Rust IPFS was originally authored by @dvc94ch and was maintained by @koivunej, and @aphelionz, but now is maintained by @dariusc93. 

**For maintainers please see the [archived readme](./archived/README.md).**

## Alternatives and other cool, related projects

It’s been noted that the Rust-IPFS name and popularity may serve its organization from a "first-mover" perspective. However, alternatives with different philosophies do exist, and we believe that supporting a diverse IPFS community is important and will ultimately help produce the best solution possible.

- Parity's [`rust-libp2p`](https://github.com/libp2p/rust-libp2p), which does a lot the of heavy lifting here
- [`Beetle (previously known as Iroh)`](https://github.com/n0-computer/beetle) - Another rust implementation of IPFS
- [`ipfs-embed`](https://github.com/ipfs-rust/ipfs-embed/) - Another rust implementation of IPFS
- [`rust-ipfs-api`](https://github.com/ferristseng/rust-ipfs-api) - A Rust client for an existing IPFS HTTP API. Supports both hyper and actix.
- [`rust-ipld`](https://github.com/ipld/libipld) - Basic rust ipld library supporting `dag-cbor`, `dag-json` and `dag-pb` formats.
- PolkaX's own [`rust-ipfs`](https://github.com/PolkaX/rust-ipfs)


If you know of another implementation or another cool project adjacent to these efforts, let us know!

## Contributors

**For previous/original contributors, please see the [archived readme](./archived/README.md).**

## License

Dual licensed under MIT or Apache License (Version 2.0). See [LICENSE-MIT](./LICENSE-MIT) and [LICENSE-APACHE](./LICENSE-APACHE) for more details.

## Trademarks

The [Rust logo and wordmark](https://www.rust-lang.org/policies/media-guide) are trademarks owned and protected by the [Rust Foundation](https://foundation.rust-lang.org). The Rust and Cargo logos (bitmap and vector) are owned by Rust Foundation and distributed under the terms of the [Creative Commons Attribution license (CC-BY)](https://creativecommons.org/licenses/by/4.0/).
