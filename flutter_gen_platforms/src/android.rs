use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::AndroidConfig;

fn copy_manifest_templates(project_dir: &Path, android_dir: &Path, templates_dir: &Path) -> Result<()> {
    let src_dir = project_dir.join(templates_dir);
    if !src_dir.exists() {
        anyhow::bail!(
            "Android manifest templates directory not found: {}",
            src_dir.display()
        );
    }

    let main_src = src_dir.join("AndroidManifest.main.xml");
    if !main_src.exists() {
        anyhow::bail!(
            "Missing required manifest template: {}",
            main_src.display()
        );
    }

    let mappings = [
        (
            src_dir.join("AndroidManifest.main.xml"),
            android_dir.join("app/src/main/AndroidManifest.xml"),
        ),
        (
            src_dir.join("AndroidManifest.debug.xml"),
            android_dir.join("app/src/debug/AndroidManifest.xml"),
        ),
        (
            src_dir.join("AndroidManifest.profile.xml"),
            android_dir.join("app/src/profile/AndroidManifest.xml"),
        ),
    ];

    for (src, dst) in mappings {
        // debug/profile templates are optional; main is validated above.
        if !src.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dir: {}", parent.display()))?;
        }
        fs::copy(&src, &dst)
            .with_context(|| format!("Failed to copy {} -> {}", src.display(), dst.display()))?;
    }

    Ok(())
}

pub fn apply_repositories(path: &Path, repos: &[String]) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let mut out = Vec::new();
    let mut in_repos = false;
    let mut inserted = false;

    for line in &lines {
        out.push(line.clone());
        if line.trim() == "repositories {" && !inserted {
            in_repos = true;
            for repo in repos {
                let insert = format!("        maven {{ url = uri(\"{}\") }}", repo);
                out.push(insert);
            }
            inserted = true;
        } else if in_repos && line.trim() == "}" {
            in_repos = false;
        }
    }

    fs::write(path, out.join("\n") + "\n")
        .with_context(|| format!("Failed to write file: {}", path.display()))?;
    Ok(())
}

pub fn apply_plugin_repositories(path: &Path, repos: &[String]) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let mut out = Vec::new();
    let mut in_plugin_repos = false;
    let mut inserted = false;

    for line in content.lines() {
        out.push(line.to_string());
        if line.trim() == "repositories {" && !inserted {
            in_plugin_repos = true;
            for repo in repos {
                let insert = format!("        maven {{ url = uri(\"{}\") }}", repo);
                out.push(insert);
            }
            inserted = true;
        } else if in_plugin_repos && line.trim() == "}" {
            in_plugin_repos = false;
        }
    }

    fs::write(path, out.join("\n") + "\n")
        .with_context(|| format!("Failed to write file: {}", path.display()))?;
    Ok(())
}

pub fn apply_app_gradle(
    path: &Path,
    namespace: &str,
    application_id: &str,
    output_file_name: Option<&str>,
    abi_filters: Option<&[String]>,
    kotlin_incremental: Option<bool>,
) -> Result<()> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let mut out = Vec::new();
    let mut in_build_types = false;
    let mut in_default_config = false;
    let mut in_kotlin_options = false;
    let mut added_output_config = false;
    let mut added_abi_filters = false;
    let mut added_kotlin_incremental = false;

    for line in content.lines() {
        if line.trim_start().starts_with("namespace = ") {
            out.push(format!("    namespace = \"{}\"", namespace));
        } else if line.trim_start().starts_with("applicationId = ") {
            out.push(format!("        applicationId = \"{}\"", application_id));
        } else {
            out.push(line.to_string());
        }

        if line.trim().starts_with("kotlinOptions {") {
            in_kotlin_options = true;
        }

        if in_kotlin_options && line.trim() == "}" && !added_kotlin_incremental {
            in_kotlin_options = false;
            if let Some(false) = kotlin_incremental {
                out.push(String::new());
                out.push(
                    "    // Disable Kotlin incremental compilation to avoid cross-drive path issues"
                        .to_string(),
                );
                out.push("    tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile> {".to_string());
                out.push("        incremental = false".to_string());
                out.push("    }".to_string());
                added_kotlin_incremental = true;
            }
        }

        if line.trim().starts_with("defaultConfig {") {
            in_default_config = true;
        }

        if in_default_config && line.trim() == "}" && !added_abi_filters {
            if let Some(abis) = abi_filters {
                if !abis.is_empty() {
                    out.insert(out.len() - 1, format!("        ndk {{"));
                    for abi in abis {
                        out.insert(out.len() - 1, format!("            abiFilters.add(\"{}\")", abi));
                    }
                    out.insert(out.len() - 1, format!("        }}"));
                }
            }
            in_default_config = false;
            added_abi_filters = true;
        }

        if line.trim().starts_with("buildTypes {") {
            in_build_types = true;
        }

        if in_build_types && line.trim() == "}" && !added_output_config {
            in_build_types = false;
            if let Some(filename_pattern) = output_file_name {
                out.push(String::new());
                out.push("    applicationVariants.all {".to_string());
                out.push("        outputs.all {".to_string());
                out.push("            val output = this as com.android.build.gradle.internal.api.BaseVariantOutputImpl".to_string());
                out.push(format!("            output.outputFileName = \"{}\"", filename_pattern));
                out.push("        }".to_string());
                out.push("    }".to_string());
                added_output_config = true;
            }
        }
    }
    fs::write(path, out.join("\n") + "\n")
        .with_context(|| format!("Failed to write file: {}", path.display()))?;
    Ok(())
}

pub fn apply_gradle_wrapper_properties(path: &Path, distribution_url: &str) -> Result<()> {
    let mut props = read_properties(path)?;
    props.insert("distributionUrl".to_string(), distribution_url.to_string());
    write_properties(path, &props)?;
    Ok(())
}

fn read_properties(path: &Path) -> Result<HashMap<String, String>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let props = java_properties::read(std::io::BufReader::new(file))
        .with_context(|| format!("Failed to parse properties: {}", path.display()))?;
    Ok(props)
}

fn write_properties(path: &Path, props: &HashMap<String, String>) -> Result<()> {
    let file = fs::File::create(path)
        .with_context(|| format!("Failed to write file: {}", path.display()))?;
    java_properties::write(std::io::BufWriter::new(file), props)
        .with_context(|| format!("Failed to write properties: {}", path.display()))?;
    Ok(())
}

pub fn process_android_platform(
    project_dir: &Path,
    config: &AndroidConfig,
    platforms_dir: Option<&str>,
) -> Result<()> {
    let android_dir = project_dir.join("android");

    let platforms_root = platforms_dir
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or("platforms");
    let templates_dir = std::path::PathBuf::from(platforms_root).join("android");
    copy_manifest_templates(project_dir, &android_dir, &templates_dir)?;

    apply_repositories(
        &android_dir.join("build.gradle.kts"),
        &config.build.allprojects.repositories,
    )?;
    apply_plugin_repositories(
        &android_dir.join("settings.gradle.kts"),
        &config.settings.plugin_management.repositories,
    )?;
    apply_app_gradle(
        &android_dir.join("app/build.gradle.kts"),
        &config.app.build.namespace,
        &config.app.build.application_id,
        config.app.build.output_file_name.as_deref(),
        config.app.build.abi_filters.as_deref(),
        config.app.build.kotlin_incremental,
    )?;
    // Manifests are fully driven by template files under platforms/android.
    if let Some(distribution_url) = &config.gradle_wrapper.distribution_url {
        apply_gradle_wrapper_properties(
            &android_dir.join("gradle/wrapper/gradle-wrapper.properties"),
            distribution_url,
        )?;
    }

    println!("Android directory generated at: {}", android_dir.display());
    Ok(())
}
