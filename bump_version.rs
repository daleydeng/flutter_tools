#!/usr/bin/env rust-script
//! Bump Flutter Version
//!
//! Updates the version in pubspec.yaml.
//!
//! Usage:
//!   rust-script bump_version.rs [major|minor|patch|build]
//!
//! ```cargo
//! [dependencies]
//! clap = { version = "4.4", features = ["derive"] }
//! regex = "1.10"
//! anyhow = "1.0"
//! semver = "1.0"
//! ```

use clap::{Parser, ValueEnum};
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use regex::Regex;
use semver::{Version, Prerelease, BuildMetadata};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The part of the version to increment
    #[arg(value_enum)]
    part: VersionPart,

    /// Path to pubspec.yaml
    #[arg(long, default_value = "pubspec.yaml")]
    pubspec: String,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum VersionPart {
    Major,
    Minor,
    Patch,
    Build,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let pubspec_path = Path::new(&args.pubspec);

    let content = fs::read_to_string(pubspec_path)
        .with_context(|| format!("Failed to read {}", pubspec_path.display()))?;

    // Regex to locate the version line, preserving indentation and formatting
    let version_line_regex = Regex::new(r"(?m)^version:\s*(.+)$").unwrap();

    let mut new_version_string = String::new();

    let new_content = version_line_regex.replace(&content, |caps: &regex::Captures| {
        let old_version_str = caps[1].trim();
        // Use semver crate to parse the version string
        let mut v = Version::parse(old_version_str)
            .unwrap_or_else(|e| panic!("Invalid semver format in pubspec.yaml '{}': {}", old_version_str, e));
        
        // Helper to parse numeric build number (Flutter standard)
        // Returns 0 if no build number or not numeric
        let current_build_num: u64 = if v.build.is_empty() {
            0
        } else {
            v.build.as_str().parse().unwrap_or(0)
        };

        match args.part {
            VersionPart::Major => {
                v.major += 1;
                v.minor = 0;
                v.patch = 0;
                v.pre = Prerelease::EMPTY;
                // Reset build number to 1 on major bump
                v.build = BuildMetadata::new("1").unwrap();
            }
            VersionPart::Minor => {
                v.minor += 1;
                v.patch = 0;
                v.pre = Prerelease::EMPTY;
                // Reset build number to 1 on minor bump
                v.build = BuildMetadata::new("1").unwrap();
            }
            VersionPart::Patch => {
                v.patch += 1;
                v.pre = Prerelease::EMPTY;
                // Reset build number to 1 on patch bump
                v.build = BuildMetadata::new("1").unwrap();
            }
            VersionPart::Build => {
                let new_build = current_build_num + 1;
                v.build = BuildMetadata::new(&new_build.to_string()).unwrap();
            }
        }
        
        new_version_string = v.to_string();
        format!("version: {}", new_version_string)
    });

    if new_version_string.is_empty() {
        println!("No version line found in {}", pubspec_path.display());
        return Ok(());
    }

    fs::write(pubspec_path, new_content.to_string())?;
    println!("Bumped version to: {}", new_version_string);

    Ok(())
}
