//! Per-repository profile: workshop toolchain, project install and edit rules.
//! Lives on the host at ~/kafes/sandbox/repos/<repo>/profile.toml; never read from a task

use crate::config::{REPOS_PATH, valid_name};
use crate::constants::{CHECK_YML, DEV_YAML, PRESETS, PROMPT};
use crate::ui::print_info;
use crate::utils::{home_path, tmp_path, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

/// Unknown keys are refused, so a typo can't silently drop a setting
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub preset: Option<String>,
    pub preset_version: Option<u32>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub workshop: Workshop,
    #[serde(default)]
    pub project: Project,
    pub rules: Rules,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workshop {
    #[serde(default = "default_node")]
    pub node: String,
    #[serde(default)]
    pub apt: Vec<String>,
    #[serde(default)]
    pub setup: Vec<String>,
}

impl Default for Workshop {
    fn default() -> Self {
        Workshop {
            node: default_node(),
            apt: Vec::new(),
            setup: Vec::new(),
        }
    }
}

fn default_node() -> String {
    "22/stable".into()
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub install: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub test_hint: String,
}

/// JavaScript regular expression sources over repository paths
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    pub allowed: Vec<String>,
    #[serde(default)]
    pub rejected: Vec<String>,
    #[serde(default)]
    pub review_closely: Vec<String>,
}

fn apt_name(p: &str) -> bool {
    let mut chars = p.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '.' | '-'))
}

fn env_name(k: &str) -> bool {
    let mut chars = k.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_uppercase() || c == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

impl Profile {
    fn validate(&self) -> Result<()> {
        let w = &self.workshop;
        // Written into dev.yaml as a plain scalar
        ensure!(
            !w.node.is_empty()
                && w.node
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-')),
            "workshop.node: invalid channel '{}'",
            w.node
        );
        for p in &w.apt {
            ensure!(apt_name(p), "workshop.apt: invalid package name '{p}'");
        }
        for cmd in w.setup.iter().chain(&self.project.install) {
            ensure!(!cmd.trim().is_empty(), "empty setup or install command");
            ensure!(!cmd.contains('\0'), "a command holds a NUL character");
        }
        for (k, v) in &self.project.env {
            ensure!(env_name(k), "project.env: invalid variable name '{k}'");
            ensure!(
                !v.contains(['\n', '\r', '\0']),
                "project.env.{k}: one line only"
            );
        }

        let r = &self.rules;
        ensure!(
            !r.allowed.is_empty(),
            "rules.allowed is empty: the agent could not change anything"
        );
        for rule in r.allowed.iter().chain(&r.rejected).chain(&r.review_closely) {
            ensure!(
                !rule.is_empty(),
                "rules: an empty pattern matches every path"
            );
            ensure!(
                !rule.contains(['\n', '\r']),
                "rules: one line per pattern: {rule}"
            );
            // GitHub Actions would evaluate it as an expression inside the workflow
            ensure!(
                !rule.contains("${{"),
                "rules: '$' followed by two braces is not allowed: {rule}"
            );
        }
        Ok(())
    }

    pub fn dev_yaml(&self) -> String {
        DEV_YAML.replace("{{NODE_CHANNEL}}", &self.workshop.node)
    }

    pub fn check_workflow(&self) -> Result<String> {
        let rules = serde_json::to_string(&self.rules)?;
        Ok(CHECK_YML.replace("{{KAFES_RULES}}", &rules))
    }

    /// The agent's prompt, from the same rules the check enforces
    pub fn prompt(&self) -> String {
        let list = |rules: &[String]| {
            rules
                .iter()
                .map(|r| format!("    - `{r}`"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut paths = format!(
            "- Only edit files whose path matches one of these regular expressions:\n{}",
            list(&self.rules.allowed)
        );
        if !self.rules.rejected.is_empty() {
            paths.push_str(&format!(
                "\n  and none of these:\n{}",
                list(&self.rules.rejected)
            ));
        }
        let hint = match self.project.test_hint.trim() {
            "" => String::new(),
            h => format!("- {h}\n"),
        };
        PROMPT
            .replace("{{KAFES_TEST_HINT}}", &hint)
            .replace("{{KAFES_PATHS}}", &paths)
    }

    pub fn print_summary(&self) {
        let none = |v: &[String]| {
            if v.is_empty() {
                "none".to_string()
            } else {
                v.join(" ")
            }
        };
        let list = |title: &str, v: &[String]| {
            print_info(&format!(
                "{title}:{}",
                if v.is_empty() { " none" } else { "" }
            ));
            for item in v {
                print_info(&format!("    {item}"));
            }
        };
        if let Some(p) = &self.preset {
            print_info(&format!(
                "preset: {p} v{}",
                self.preset_version.unwrap_or(0)
            ));
        }
        if !self.description.is_empty() {
            print_info(&self.description);
        }
        let (w, p) = (&self.workshop, &self.project);
        print_info(&format!("node SDK: {}", w.node));
        print_info(&format!("apt packages: {}", none(&w.apt)));
        list("setup commands", &w.setup);
        print_info(&format!(
            "install: {}",
            p.install.as_deref().unwrap_or("none")
        ));
        let env: Vec<String> = p.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        print_info(&format!("install env: {}", none(&env)));
        list("allowed paths", &self.rules.allowed);
        list("rejected paths", &self.rules.rejected);
        list("review closely", &self.rules.review_closely);
        print_info(
            "always rejected: dotfiles, binaries, symlinks, executables, submodules, unusual path characters",
        );
    }
}

pub fn parse_profile(text: &str) -> Result<Profile> {
    let profile: Profile = toml::from_str(text).context("invalid profile")?;
    profile.validate()?;
    Ok(profile)
}

pub fn preset_names() -> Vec<String> {
    PRESETS.iter().map(|(n, _)| n.to_string()).collect()
}

pub fn preset_text(name: &str) -> Result<&'static str> {
    PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
        .with_context(|| {
            format!(
                "no preset '{name}'; available: {}",
                preset_names().join(", ")
            )
        })
}

pub fn repo_dir(name: &str) -> Result<PathBuf> {
    ensure!(valid_name(name), "invalid repository name: {name}");
    Ok(home_path(REPOS_PATH)?.join(name))
}

fn profile_path(name: &str) -> Result<PathBuf> {
    Ok(repo_dir(name)?.join("profile.toml"))
}

pub fn draft_path(name: &str) -> Result<PathBuf> {
    Ok(repo_dir(name)?.join("profile.edit.toml"))
}

pub fn check_path(name: &str) -> Result<PathBuf> {
    Ok(repo_dir(name)?.join("kafes-pr-check.yml"))
}

pub fn load_profile_text(name: &str) -> Result<Option<String>> {
    match fs::read_to_string(profile_path(name)?) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn load_profile(name: &str) -> Result<Profile> {
    let text = load_profile_text(name)?.with_context(|| {
        format!("{name} has no profile; set one with: kafes repo edit {name} --profile <preset>")
    })?;
    parse_profile(&text).with_context(|| format!("profile of {name}"))
}

pub fn save_profile_text(name: &str, text: &str) -> Result<()> {
    parse_profile(text)?;
    fs::create_dir_all(repo_dir(name)?)?;
    write_atomic(&profile_path(name)?, text)
}

pub fn delete_repo_dir(name: &str) -> Result<()> {
    let dir = repo_dir(name)?;
    for path in [profile_path(name)?, draft_path(name)?, check_path(name)?] {
        for p in [tmp_path(&path), path] {
            match fs::remove_file(&p) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    match fs::remove_dir(&dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => bail!("{} is not empty ({e}); inspect it", dir.display()),
        Ok(()) => Ok(()),
    }
}
