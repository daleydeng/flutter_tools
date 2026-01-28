use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;
use which::which;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub project_name: String,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub output_file_name_pattern: Option<String>,
    #[serde(default)]
    pub pubspec: Option<PubspecConfig>,
    #[serde(default)]
    pub platforms_dir: Option<String>,
    #[serde(default)]
    pub create: FlutterCreateConfig,
    pub android: AndroidConfig,
    pub ios: Option<IosConfig>,
    pub windows: Option<WindowsConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PubspecConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct FlutterCreateConfig {
    #[serde(default)]
    pub platforms: Option<Vec<String>>,
    #[serde(default)]
    pub android_language: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AndroidConfig {
    #[serde(default)]
    pub gradle_wrapper: AndroidGradleWrapperConfig,
    #[serde(default)]
    pub app: AndroidAppConfig,
    pub build: AndroidBuildConfig,
    #[serde(default)]
    pub settings: AndroidSettingsConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct AndroidBuildConfig {
    pub allprojects: RepositoryList,
}

#[derive(Debug, Deserialize, Default)]
pub struct AndroidSettingsConfig {
    pub plugin_management: RepositoryList,
}

#[derive(Debug, Deserialize, Default)]
pub struct AndroidGradleWrapperConfig {
    pub distribution_url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AndroidAppConfig {
    #[serde(default)]
    pub build: AndroidAppBuildConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct AndroidAppBuildConfig {
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub application_id: String,
    #[serde(default)]
    pub output_file_name: Option<String>,
    #[serde(default)]
    pub abi_filters: Option<Vec<String>>,
    #[serde(default)]
    pub kotlin_incremental: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct IosConfig {}

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct WindowsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub window_width: Option<u32>,
    #[serde(default)]
    pub window_height: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RepositoryList {
    pub repositories: Vec<String>,
}

pub fn load_config(path: &Path) -> Result<Config> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("pkl") => load_pkl_config(path),
        Some("toml") => {
            let content = fs::read_to_string(path)
                .with_context(|| format!("Failed to read config: {}", path.display()))?;
            let cfg: Config = toml::from_str(&content).context("Failed to parse config")?;
            Ok(cfg)
        }
        _ => {
            bail!("Unsupported config format: {}", path.display());
        }
    }
}

fn load_pkl_config(path: &Path) -> Result<Config> {
    let pkl_cmd = resolve_cmd("pkl")?;
    let output = run_pkl_eval(&pkl_cmd, path, ["-f", "json"])
        .or_else(|_| run_pkl_eval(&pkl_cmd, path, ["--format", "json"]))
        .with_context(|| format!("Failed to run pkl eval for: {}", path.display()))?;

    let cfg: Config = serde_json::from_slice(&output)
        .with_context(|| format!("Failed to parse pkl output: {}", path.display()))?;
    Ok(cfg)
}

fn run_pkl_eval(pkl_cmd: &Path, path: &Path, format_args: [&str; 2]) -> Result<Vec<u8>> {
    let output = Command::new(pkl_cmd)
        .arg("eval")
        .args(format_args)
        .arg(path)
        .output()
        .with_context(|| format!("Failed to run pkl eval for: {}", path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("pkl eval failed: {stderr}");
    }

    Ok(output.stdout)
}

pub fn expand_config(cfg: &mut Config) -> Result<()> {
    cfg.project_name = expand_env_vars(&cfg.project_name)?;
    if let Some(value) = cfg.org.as_ref() {
        cfg.org = Some(expand_env_vars(value)?);
    }
    if let Some(value) = cfg.description.as_ref() {
        cfg.description = Some(expand_env_vars(value)?);
    }
    if let Some(value) = cfg.platforms_dir.as_ref() {
        cfg.platforms_dir = Some(expand_env_vars(value)?);
    }
    expand_flutter_create_config(&mut cfg.create)?;
    expand_android_config(&mut cfg.android)?;
    if cfg.android.app.build.application_id.trim().is_empty() {
        if let Some(org) = cfg.org.as_ref().map(|value| value.trim()).filter(|v| !v.is_empty()) {
            let org = org.trim_end_matches('.');
            cfg.android.app.build.application_id = format!("{}.{}", org, cfg.project_name);
        } else {
            bail!("android.app.build.application_id is required when org is not set");
        }
    }
    if cfg.android.app.build.namespace.trim().is_empty() {
        cfg.android.app.build.namespace = cfg.android.app.build.application_id.clone();
    }
    Ok(())
}

fn expand_android_config(cfg: &mut AndroidConfig) -> Result<()> {
    cfg.app.build.namespace = expand_env_vars(&cfg.app.build.namespace)?;
    cfg.app.build.application_id = expand_env_vars(&cfg.app.build.application_id)?;
    if let Some(value) = cfg.gradle_wrapper.distribution_url.as_ref() {
        cfg.gradle_wrapper.distribution_url = Some(expand_env_vars(value)?);
    }
    cfg.build.allprojects.repositories = cfg
        .build
        .allprojects
        .repositories
        .iter()
        .map(|value| expand_env_vars(value))
        .collect::<Result<Vec<_>>>()?;
    cfg.settings.plugin_management.repositories = cfg
        .settings
        .plugin_management
        .repositories
        .iter()
        .map(|value| expand_env_vars(value))
        .collect::<Result<Vec<_>>>()?;
    Ok(())
}

fn expand_flutter_create_config(cfg: &mut FlutterCreateConfig) -> Result<()> {
    if let Some(value) = cfg.android_language.as_ref() {
        cfg.android_language = Some(expand_env_vars(value)?);
    }
    if let Some(platforms) = cfg.platforms.as_ref() {
        cfg.platforms = Some(
            platforms
                .iter()
                .map(|value| expand_env_vars(value))
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Ok(())
}

fn expand_env_vars(input: &str) -> Result<String> {
    let mut out = String::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                let mut end = i + 2;
                while end < chars.len() && chars[end] != '}' {
                    end += 1;
                }
                if end >= chars.len() {
                    bail!("Unclosed env var in config value: {input}");
                }
                let key: String = chars[i + 2..end].iter().collect();
                let value = env::var(&key)
                    .with_context(|| format!("Missing env var: {key}"))?;
                out.push_str(&value);
                i = end + 1;
                continue;
            }

            let mut end = i + 1;
            while end < chars.len()
                && (chars[end].is_ascii_alphanumeric() || chars[end] == '_')
            {
                end += 1;
            }
            if end > i + 1 {
                let key: String = chars[i + 1..end].iter().collect();
                let value = env::var(&key)
                    .with_context(|| format!("Missing env var: {key}"))?;
                out.push_str(&value);
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    Ok(out)
}

fn resolve_cmd(command: &str) -> Result<std::path::PathBuf> {
    if command.contains(['/', '\\']) {
        let path = std::path::PathBuf::from(command);
        if path.exists() {
            return Ok(path);
        }
        bail!("command not found at: {}", path.display());
    }
    which(command).with_context(|| format!("command not found in PATH: {command}"))
}
