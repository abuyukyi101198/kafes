use crate::utils::{home_path, write_atomic};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, read_to_string};
use std::io::ErrorKind;
use std::path::PathBuf;

const CONFIG_PATH: &str = "kafes/sandbox/config.toml";

#[derive(Serialize, Deserialize)]
pub struct GlobalConfig {
    pub gh_user: String,
    pub kafes_owner: String,
    pub app: AppConfig,
    pub network: Option<NetworkConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: String,
    pub installation_id: Option<u64>,
    pub bot_name: String,
    pub bot_email: String,
}

#[derive(Serialize, Deserialize)]
pub struct NetworkConfig {
    pub bridge: String,
    pub acls_before: String,
}

fn config_path() -> Result<PathBuf> {
    home_path(CONFIG_PATH)
}

pub fn load_global_config() -> Result<GlobalConfig> {
    let path = config_path()?;
    let text = read_to_string(&path)
        .with_context(|| format!("{} missing; run kafes setup first", path.display()))?;
    toml::from_str(&text).with_context(|| format!("{} invalid toml", path.display()))
}

pub fn save_global_config(cfg: &GlobalConfig) -> Result<()> {
    write_atomic(&config_path()?, &toml::to_string(cfg)?)
}

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

const REPOS_PATH: &str = "kafes/sandbox/repos";

#[derive(Serialize, Deserialize)]
pub struct RepoConfig {
    pub name: String,
    pub checkout: PathBuf,
    pub default_branch_before: Option<String>,
}

impl RepoConfig {
    pub fn fork(&self, g: &GlobalConfig) -> String {
        format!("{}/{}", g.gh_user, self.name)
    }

    pub fn kafes_fork(&self, g: &GlobalConfig) -> String {
        format!("{}/{}", g.kafes_owner, self.name)
    }
}

fn repo_path(name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        bail!("invalid repository name: {name}");
    }
    Ok(home_path(REPOS_PATH)?.join(format!("{name}.toml")))
}

pub fn load_repo_config(name: &str) -> Result<RepoConfig> {
    let path = repo_path(name)?;
    let text = read_to_string(&path)
        .with_context(|| format!("no repository {name}; run kafes add {name}"))?;
    toml::from_str(&text).with_context(|| format!("{} invalid toml", path.display()))
}

pub fn save_repo_config(cfg: &RepoConfig) -> Result<()> {
    let path = repo_path(&cfg.name)?;
    create_dir_all(home_path(REPOS_PATH)?)?;
    write_atomic(&path, &toml::to_string(cfg)?)
}

pub fn list_repos() -> Result<Vec<String>> {
    let dir = home_path(REPOS_PATH)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut names = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "toml") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if valid_name(stem) {
                    names.push(stem.to_string());
                }
            }
        }
    }

    names.sort();
    Ok(names)
}

pub fn pick_repo(name: Option<String>) -> Result<RepoConfig> {
    let name = match name {
        Some(n) => n,
        None => match list_repos()?.as_slice() {
            [only] => only.clone(),
            [] => bail!("no repositories; run kafes add"),
            many => bail!("several repositories; pass --repo <{}>", many.join("|")),
        },
    };
    load_repo_config(&name)
}

pub fn delete_repo_config(name: &str) -> Result<()> {
    let path = repo_path(name)?;
    std::fs::remove_file(&path).with_context(|| format!("can't remove {}", path.display()))
}
