#!/usr/bin/env rust-script
//! Tag Git Version
//!
//! Creates a lightweight git tag for the *current* version in pubspec.yaml.
//!
//! ## What it does
//! - Reads the current `version:` from `pubspec.yaml` (YAML parser, with a regex fallback).
//! - Checks if a tag already exists for that version (`vX.Y.Z` or `X.Y.Z`).
//! - If not, creates a **lightweight** tag pointing at `HEAD` with the expected name.
//!
//! Usage:
//!   rust-script git_tag_version.rs [--pubspec PATH] [--tag-prefix v|none]
//!
//! ```cargo
//! [dependencies]
//! clap = { version = "4.4", features = ["derive"] }
//! regex = "1.10"
//! anyhow = "1.0"
//! semver = "1.0"
//! gix = "0.78"
//! serde = { version = "1.0", features = ["derive"] }
//! serde_yaml = "0.9"
//! ```

use clap::{Parser, ValueEnum};
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use regex::Regex;
use semver::{Version, BuildMetadata};
use gix::refs::transaction::PreviousValue;
use serde::Deserialize;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to pubspec.yaml
    #[arg(long, default_value = "pubspec.yaml")]
    pubspec: String,

    /// Tag prefix for the auto-created lightweight tag.
    ///
    /// Use `v` to create tags like `v1.2.3`, or `none` to create `1.2.3`.
    #[arg(long, value_enum, default_value = "v")]
    tag_prefix: TagPrefix,

    /// Force recreate tag even if it already exists
    #[arg(short = 'f', long)]
    force: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum TagPrefix {
    V,
    None,
}

fn tag_exists(repo: &gix::Repository, tag: &str) -> Result<bool> {
    let full = format!("refs/tags/{tag}");
    Ok(repo.try_find_reference(full.as_str())?.is_some())
}

#[derive(Debug, Deserialize)]
struct PubspecYaml {
    version: Option<String>,
}

fn read_pubspec_version(content: &str) -> Option<String> {
    // YAML parse (preferred): robust against indentation/ordering differences.
    if let Ok(doc) = serde_yaml::from_str::<PubspecYaml>(content) {
        if let Some(v) = doc.version {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(v);
            }
        }
    }

    // Fallback: handle partial/invalid YAML while still supporting the common case.
    let version_line_regex = Regex::new(r"(?m)^version:\s*(.+)$").ok()?;
    version_line_regex
        .captures(content)
        .map(|c| c[1].trim().to_string())
        .filter(|s| !s.is_empty())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let pubspec_path = Path::new(&args.pubspec);

    let start_dir = pubspec_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    
    let repo = match gix::discover(start_dir) {
        Ok(r) => r,
        Err(_) => {
            println!("[tag-version] Skipping: not in a git repository");
            return Ok(());
        }
    };

    let content = fs::read_to_string(pubspec_path)
        .with_context(|| format!("Failed to read {}", pubspec_path.display()))?;

    let version_str = match read_pubspec_version(&content) {
        Some(v) => v,
        None => {
            println!("[tag-version] Skipping: no version found in pubspec");
            return Ok(());
        }
    };
    
    let v = match Version::parse(&version_str) {
        Ok(v) => v,
        Err(e) => {
            println!(
                "[tag-version] Skipping: invalid semver in pubspec '{}': {}",
                version_str, e
            );
            return Ok(());
        }
    };

    // Tag name: use semver core (+ optional prerelease), ignore build metadata for the tag string base
    // But wait - usually we WANT the build number in the tag for Flutter apps if it's significant?
    // bump_version.rs ignored it:
    // let mut base = format!("{}.{}.{}", v.major, v.minor, v.patch);
    // if !v.pre.is_empty() { base = format!("{}-{}", base, v.pre); }
    //
    // pubspec version often has +1, +2 etc.
    // If we have 0.4.0+1, do we tag v0.4.0+1 or v0.4.0?
    // Git tags with + are tricky sometimes but valid.
    // bump_version.rs explicitly ignored build metadata:
    // "Tag name: use semver core (+ optional prerelease), ignore build metadata."
    
    let mut v_tag = v.clone();
    v_tag.build = BuildMetadata::EMPTY;
    let base = v_tag.to_string();

    let tag_plain = base.clone();
    let tag_v = format!("v{}", base);

    let preferred_tag = match args.tag_prefix {
        TagPrefix::V => tag_v.clone(),
        TagPrefix::None => tag_plain.clone(),
    };

    // If either convention exists, do nothing or maybe just report it.
    if tag_exists(&repo, &tag_plain)? {
        if args.force {
            println!("[tag-version] Tag '{}' already exists. Force deleting...", tag_plain);
            let tag_ref = format!("refs/tags/{}", tag_plain);
            if let Some(reference) = repo.try_find_reference(&tag_ref)? {
                reference.delete()?;
            }
        } else {
            println!("[tag-version] Tag '{}' already exists.", tag_plain);
            if preferred_tag == tag_v && !tag_exists(&repo, &tag_v)? {
                 println!("[tag-version] Note: '{}' exists but you asked for prefix 'v'. Skipping to avoid duplicates.", tag_plain);
            }
            return Ok(());
        }
    }
    
    if tag_exists(&repo, &tag_v)? {
        if args.force {
            println!("[tag-version] Tag '{}' already exists. Force deleting...", tag_v);
            let tag_ref = format!("refs/tags/{}", tag_v);
            if let Some(reference) = repo.try_find_reference(&tag_ref)? {
                reference.delete()?;
            }
        } else {
            println!("[tag-version] Tag '{}' already exists.", tag_v);
             if preferred_tag == tag_plain {
                 println!("[tag-version] Note: '{}' exists but you asked for 'none'. Skipping.", tag_v);
            }
            return Ok(());
        }
    }

    let head_id = match repo.head_id() {
        Ok(id) => id.detach(),
        Err(_) => {
            println!("[tag-version] Skipping: repository has no commits yet");
            return Ok(());
        }
    };

    let tag_creation_result = if args.force {
        repo.tag_reference(&preferred_tag, head_id, PreviousValue::Any)
    } else {
        repo.tag_reference(&preferred_tag, head_id, PreviousValue::MustNotExist)
    };

    tag_creation_result
        .with_context(|| format!("Failed to create lightweight tag '{preferred_tag}'"))?;
    
    println!(
        "[tag-version] Created lightweight tag '{}' for version {}",
        preferred_tag, version_str
    );
    
    Ok(())
}
