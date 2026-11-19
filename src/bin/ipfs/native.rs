//! Minimal native `ipfs add` CLI for the Rust-IPFS workspace.
//!
//! This binary intentionally starts with a focused add implementation. It is not yet a
//! full Kubo replacement; unsupported Kubo import flags are rejected by Clap.

use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use futures::StreamExt;
use ipld_core::cid::{Cid, Version};
use multihash_codetable::Code;
use rust_ipfs::{
    Block, Ipfs,
    builder::DefaultIpfsBuilder,
    unixfs::{
        UnixfsStatus,
        ll::{Metadata, dir::builder::{BufferingTreeBuilder, TreeOptions}},
    },
};

#[derive(Debug, Parser)]
#[command(name = "ipfs", version, about = "Rust-IPFS command-line interface")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a file or directory to the local IPFS repository.
    Add(AddArgs),
}

#[derive(Debug, Args)]
struct AddArgs {
    /// File or directory to add. Multiple inputs are processed independently.
    #[arg(required = true, value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Add directory paths recursively.
    #[arg(short = 'r', long = "recursive")]
    recursive: bool,

    /// Write minimal output: one CID per imported file/directory node.
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,

    /// Write only the final CID.
    #[arg(short = 'Q', long = "quieter")]
    quieter: bool,

    /// Only chunk and hash. Do not store blocks, pin, or provide content.
    #[arg(short = 'n', long = "only-hash")]
    only_hash: bool,

    /// Wrap file inputs in a UnixFS directory object.
    #[arg(short = 'w', long = "wrap-with-directory")]
    wrap_with_directory: bool,

    /// Hypothetical path for a file inside a virtual directory tree.
    ///
    /// This is Rust-IPFS-specific. It implies --only-hash and computes the final
    /// /ipfs/<directory-cid>/<virtual-path> without persisting any blocks.
    #[arg(long = "virtual-path", value_name = "RELATIVE/PATH")]
    virtual_path: Option<String>,

    /// CID version. Defaults to 0, following Kubo's default import profile.
    #[arg(long = "cid-version", value_parser = clap::value_parser!(u8).range(0..=1))]
    cid_version: Option<u8>,

    /// Multihash function supported by this CLI.
    #[arg(long, value_parser = ["sha2-256", "blake3"], default_value = "sha2-256")]
    hash: String,

    /// Use raw leaves. CIDv1 enables raw leaves by default.
    #[arg(long)]
    raw_leaves: bool,

    /// Pin imported content. Use --pin=false to disable pinning.
    #[arg(long = "pin", default_value_t = true, action = clap::ArgAction::Set)]
    pin: bool,

    /// Do not write command output.
    #[arg(long)]
    silent: bool,

    /// Repository path. Defaults to IPFS_PATH or ~/.rust-ipfs.
    #[arg(long = "repo", value_name = "PATH")]
    repo: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct OutputEntry {
    cid: String,
    name: String,
}

#[derive(Debug, Clone, Copy)]
struct ImportConfig {
    cid_version: Version,
    hasher: Code,
    raw_leaves: bool,
    only_hash: bool,
    pin: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Add(args) => run_add(args).await,
    }
}

async fn run_add(mut args: AddArgs) -> anyhow::Result<()> {
    if args.paths.is_empty() {
        bail!("at least one input path is required");
    }

    let virtual_path = args
        .virtual_path
        .as_deref()
        .map(validate_virtual_path)
        .transpose()?;

    if virtual_path.is_some() && args.paths.len() != 1 {
        bail!("--virtual-path accepts exactly one input file");
    }

    if virtual_path.is_some() {
        // Virtual-path mode is deliberately read-only by default. The option
        // promises a hypothetical address, not an implicit import.
        args.only_hash = true;
        args.wrap_with_directory = true;
    }

    let hasher = match args.hash.as_str() {
        "sha2-256" => Code::Sha2_256,
        "blake3" => Code::Blake3_256,
        other => bail!("unsupported hash function: {other}"),
    };

    let cid_version = resolve_cid_version(args.cid_version, hasher, args.raw_leaves);

    let config = ImportConfig {
        cid_version,
        hasher,
        raw_leaves: args.raw_leaves || cid_version == Version::V1,
        only_hash: args.only_hash,
        pin: args.pin,
    };

    let repo_path = args
        .repo
        .clone()
        .or_else(|| std::env::var_os("IPFS_PATH").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".rust-ipfs"))
        })
        .unwrap_or_else(|| PathBuf::from(".rust-ipfs"));

    let ipfs = DefaultIpfsBuilder::new()
        .with_default()
        .enable_tcp()
        .set_path(&repo_path)
        .start()
        .await
        .with_context(|| format!("failed to open Rust-IPFS repository at {}", repo_path.display()))?;

    let result = add_inputs(&ipfs, &args, &config, virtual_path.as_deref()).await;

    // Stop the in-process node cleanly, including when an import fails.
    ipfs.exit_daemon().await;
    result
}

async fn add_inputs(
    ipfs: &Ipfs,
    args: &AddArgs,
    config: &ImportConfig,
    virtual_path: Option<&str>,
) -> anyhow::Result<()> {
    let mut all_entries = Vec::new();
    let mut final_cid = None;

    for input in &args.paths {
        let metadata = fs::symlink_metadata(input)
            .with_context(|| format!("cannot inspect input {}", input.display()))?;

        if metadata.file_type().is_symlink() {
            bail!("symbolic-link inputs are not supported yet: {}", input.display());
        }

        if virtual_path.is_some() && !metadata.is_file() {
            bail!("--virtual-path requires a regular file input");
        }

        let (entries, root_cid) = if metadata.is_dir() {
            if !args.recursive {
                bail!("{} is a directory; pass -r/--recursive to add it", input.display());
            }

            let (files, empty_dirs) = collect_directory(input)?;
            add_tree(
                ipfs,
                files,
                empty_dirs,
                input.file_name().and_then(|name| name.to_str()).unwrap_or("root").to_owned(),
                config,
                None,
            )
            .await?
        } else if metadata.is_file() {
            if let Some(virtual_path) = virtual_path {
                let files = vec![(input.to_path_buf(), virtual_path.to_owned())];
                add_tree(
                    ipfs,
                    files,
                    Vec::new(),
                    virtual_path.to_owned(),
                    config,
                    Some(virtual_path),
                )
                .await?
            } else if args.wrap_with_directory {
                let name = input
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("input file name is not valid UTF-8")?
                    .to_owned();
                let files = vec![(input.to_path_buf(), name.clone())];
                add_tree(ipfs, files, Vec::new(), name, config, None).await?
            } else {
                let (cid, _) = add_file(ipfs, input, config, config.pin).await?;
                (
                    vec![OutputEntry {
                        cid: cid.to_string(),
                        name: input.display().to_string(),
                    }],
                    cid,
                )
            }
        } else {
            bail!("input is neither a regular file nor a directory: {}", input.display());
        };

        all_entries.extend(entries);
        final_cid = Some(root_cid);
    }

    if args.silent {
        return Ok(());
    }

    if args.quieter {
        if let Some(cid) = final_cid {
            println!("{cid}");
        }
    } else if args.quiet {
        for entry in all_entries {
            println!("{}", entry.cid);
        }
    } else {
        for entry in all_entries {
            println!("added {} {}", entry.cid, entry.name);
        }
    }

    Ok(())
}

async fn add_file(
    ipfs: &Ipfs,
    file: &Path,
    config: &ImportConfig,
    pin: bool,
) -> anyhow::Result<(Cid, u64)> {
    let mut add = ipfs
        .add_unixfs(file.to_path_buf())
        .cid_version(config.cid_version)
        .hasher(config.hasher)
        .raw_leaves(config.raw_leaves)
        .pin(pin && !config.only_hash);

    if config.only_hash {
        add = add.only_hash();
    }

    let mut completed = None;
    while let Some(status) = add.next().await {
        match status {
            UnixfsStatus::ProgressStatus { .. } => {}
            UnixfsStatus::FailedStatus { error, .. } => return Err(error.into()),
            UnixfsStatus::CompletedStatus { path, dag_size, .. } => {
                let cid = path
                    .root()
                    .cid()
                    .copied()
                    .context("UnixFS add completed without a CID root")?;
                completed = Some((cid, dag_size));
            }
        }
    }

    completed.context("UnixFS add stream ended without a completion event")
}

async fn add_tree(
    ipfs: &Ipfs,
    files: Vec<(PathBuf, String)>,
    empty_dirs: Vec<String>,
    root_name: String,
    config: &ImportConfig,
    virtual_path: Option<&str>,
) -> anyhow::Result<(Vec<OutputEntry>, Cid)> {
    let mut options = TreeOptions::default();
    options.cid_version(config.cid_version);
    options.hasher(config.hasher);
    options.wrap_with_directory();

    let mut tree = BufferingTreeBuilder::new(options);
    let mut output = Vec::new();

    for (file, relative_path) in files {
        // The final directory root owns recursive pinning; do not leave direct pins on every leaf.
        let (cid, dag_size) = add_file(ipfs, &file, config, false).await?;
        tree.put_link(&relative_path, cid, dag_size)
            .with_context(|| format!("cannot add {} to the virtual directory tree", relative_path))?;
        output.push(OutputEntry {
            cid: cid.to_string(),
            name: relative_path,
        });
    }

    for directory in empty_dirs {
        tree.set_metadata(&directory, Metadata::default())
            .with_context(|| format!("cannot add empty directory {directory}"))?;
    }

    let mut directory_blocks = Vec::new();
    let mut root_cid = None;
    let mut iterator = tree.build();

    while let Some(node) = iterator.next_borrowed() {
        let node = node?;
        let cid = node.cid.to_owned();

        if !config.only_hash {
            directory_blocks.push(Block::new(cid, node.block.to_vec())?);
        }

        if node.path.is_empty() {
            root_cid = Some(cid);
        } else {
            output.push(OutputEntry {
                cid: cid.to_string(),
                name: node.path.to_owned(),
            });
        }
    }

    let root_cid = root_cid.context("directory builder did not produce a root CID")?;

    if !config.only_hash {
        ipfs.repo().put_blocks(directory_blocks).await?;
        if config.pin {
            ipfs.repo().pin(root_cid).recursive().await?;
        }
    }

    let display_name = match virtual_path {
        Some(path) => format!("/ipfs/{root_cid}/{path}"),
        None => root_name,
    };

    output.push(OutputEntry {
        cid: root_cid.to_string(),
        name: display_name,
    });

    Ok((output, root_cid))
}

fn collect_directory(root: &Path) -> anyhow::Result<(Vec<(PathBuf, String)>, Vec<String>)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut empty_dirs = Vec::new();

    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .with_context(|| format!("cannot read directory {}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()?;

        entries.sort_by_key(|entry| entry.file_name());

        if entries.is_empty() && directory != root {
            empty_dirs.push(relative_unixfs_path(root, &directory)?);
        }

        for entry in entries {
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_symlink() {
                bail!("symbolic links are not supported yet: {}", path.display());
            } else if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                files.push((path.clone(), relative_unixfs_path(root, &path)?));
            }
        }
    }

    files.sort_by(|left, right| left.1.cmp(&right.1));
    empty_dirs.sort();
    Ok((files, empty_dirs))
}

fn relative_unixfs_path(root: &Path, path: &Path) -> anyhow::Result<String> {
    let relative = path.strip_prefix(root)?;
    let mut segments = Vec::new();

    for component in relative.components() {
        match component {
            Component::Normal(value) => segments.push(
                value
                    .to_str()
                    .context("UnixFS path contains a non-UTF-8 segment")?,
            ),
            _ => bail!("invalid relative UnixFS path: {}", path.display()),
        }
    }

    if segments.is_empty() {
        bail!("relative UnixFS path cannot be empty");
    }

    Ok(segments.join("/"))
}

fn resolve_cid_version(requested: Option<u8>, hasher: Code, raw_leaves: bool) -> Version {
    let requested = requested.unwrap_or(0);
    if requested == 0 && (hasher != Code::Sha2_256 || raw_leaves) {
        // CIDv0 only supports SHA2-256 and cannot use raw leaves.
        Version::V1
    } else if requested == 0 {
        Version::V0
    } else {
        Version::V1
    }
}

fn validate_virtual_path(path: &str) -> anyhow::Result<String> {
    let parsed = Path::new(path);
    if parsed.as_os_str().is_empty() || parsed.is_absolute() {
        bail!("--virtual-path must be a non-empty relative path");
    }

    let mut segments = Vec::new();
    for component in parsed.components() {
        match component {
            Component::Normal(value) => segments.push(
                value
                    .to_str()
                    .context("--virtual-path contains a non-UTF-8 segment")?,
            ),
            _ => bail!("--virtual-path cannot contain '.', '..', or a root prefix"),
        }
    }

    if segments.is_empty() {
        bail!("--virtual-path must name a file");
    }

    Ok(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_profile_promotes_blake3_and_raw_leaves_to_cidv1() {
        assert_eq!(resolve_cid_version(None, Code::Sha2_256, false), Version::V0);
        assert_eq!(resolve_cid_version(None, Code::Blake3_256, false), Version::V1);
        assert_eq!(resolve_cid_version(Some(0), Code::Sha2_256, true), Version::V1);
        assert_eq!(resolve_cid_version(Some(1), Code::Sha2_256, false), Version::V1);
    }

    #[test]
    fn parses_kubo_style_combined_quiet_recursive_flags() {
        let cli = Cli::try_parse_from(["ipfs", "add", "-qr", "./site"]).unwrap();
        let Command::Add(args) = cli.command;
        assert!(args.quiet);
        assert!(args.recursive);
        assert!(!args.quieter);
    }

    #[test]
    fn parses_quieter_flag_independently() {
        let cli = Cli::try_parse_from(["ipfs", "add", "-Q", "./asset.bin"]).unwrap();
        let Command::Add(args) = cli.command;
        assert!(args.quieter);
        assert!(!args.quiet);
    }

    #[test]
    fn virtual_paths_keep_nested_segments() {
        assert_eq!(
            validate_virtual_path("collection/metadata.json").unwrap(),
            "collection/metadata.json"
        );
    }

    #[test]
    fn virtual_paths_reject_parent_traversal() {
        assert!(validate_virtual_path("../metadata.json").is_err());
        assert!(validate_virtual_path("/collection/metadata.json").is_err());
    }
}
