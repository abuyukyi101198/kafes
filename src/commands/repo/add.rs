//! `kafes repo add [repo] [--profile <preset>]`: add a repository under the kafes organization.

use crate::commands::repo::profile::choose_profile;
use crate::commands::setup::{setup_check, setup_verify};
use crate::config::{
    RepoConfig, load_global_config, load_repo_config, save_global_config, save_repo_config,
    valid_name,
};
use crate::github::gh_api;
use crate::ui::{self, Manual};
use crate::utils::run_cmd;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub fn cmd_add(name: Option<String>, profile: Option<String>) -> Result<()> {
    load_global_config().context("run kafes init first")?;
    let name = repo_stages(name, profile)?;
    ui::section(&format!("{name} added"));
    ui::next(&[(
        format!("kafes task create {name}/<feature-branch>"),
        "start a task on it",
    )]);
    Ok(())
}

pub fn repo_stages(name: Option<String>, profile: Option<String>) -> Result<String> {
    let mut repo = add_repo(name, profile)?;
    // Loaded after add_repo, which may have saved the installation ID
    let global = load_global_config()?;
    setup_check(&global, &mut repo)?;
    setup_verify(&global, &repo)?;
    Ok(repo.name)
}

fn add_repo(name: Option<String>, profile: Option<String>) -> Result<RepoConfig> {
    let mut global = load_global_config()?;
    let name = match name {
        Some(n) => n,
        None => {
            ui::section("Repository");
            ui::text_help(
                "Repository name",
                None,
                Some("the name of your fork on GitHub, without the owner"),
            )?
        }
    };
    if !valid_name(&name) {
        bail!("invalid repository name");
    }
    ui::section(&format!("Add {}/{name}", global.gh_user));
    let prev = load_repo_config(&name).ok();

    let default_checkout = prev
        .as_ref()
        .map(|p| p.checkout.to_string_lossy().into_owned());
    let checkout = ui::text_help(
        "Local checkout",
        default_checkout.as_deref(),
        Some(&format!(
            "absolute path of your clone of {}/{name}",
            global.gh_user
        )),
    )?;
    let checkout = PathBuf::from(checkout);
    if !checkout.is_absolute() {
        bail!("checkout path must be absolute");
    }

    let repo = RepoConfig {
        name,
        checkout,
        default_branch_before: prev.and_then(|p| p.default_branch_before),
    };
    let fork = repo.fork(&global);
    let dir = repo.checkout.to_string_lossy();

    run_cmd("git", &["-C", &dir, "rev-parse", "--git-dir"])
        .with_context(|| format!("{dir} is not a Git checkout"))?;
    let origin = run_cmd("git", &["-C", &dir, "remote", "get-url", "origin"])?;
    let origin = origin.trim().trim_end_matches('/').trim_end_matches(".git");
    if !(origin.ends_with(&format!("/{fork}")) || origin.ends_with(&format!(":{fork}"))) {
        bail!("origin of {dir} is {origin}, not {fork}");
    }
    gh_api(&format!("/repos/{fork}")).with_context(|| format!("{fork} not found or not public"))?;
    ui::done(&format!("{dir} is a checkout of {fork}"));

    let kafes_fork = repo.kafes_fork(&global);
    if gh_api(&format!("/repos/{kafes_fork}")).is_err() {
        Manual::new("Fork your fork (not upstream) into the kafes organization")
            .step("Open:")
            .link(format!("https://github.com/{fork}/fork"))
            .step("Fill in, then Create fork:")
            .field("Owner", global.kafes_owner.as_str())
            .field("Repository name", format!("{} (keep it)", repo.name))
            .confirm()?;
    }
    let info = gh_api(&format!("/repos/{kafes_fork}")).context("create the kafes fork first")?;
    let parent = info["parent"]["full_name"].as_str().unwrap_or("nothing");
    if parent != fork {
        bail!("{kafes_fork} is a fork of {parent}, not of {fork}");
    }
    ui::done(&format!("{kafes_fork} is a fork of {fork}"));
    Manual::new("Disable Actions on the kafes fork")
        .step("Open:")
        .link(format!("https://github.com/{kafes_fork}/settings/actions"))
        .step("Actions permissions → Disable actions → Save")
        .confirm()?;

    let owner = &global.kafes_owner;
    match global.app.installation_id {
        None => {
            Manual::new(format!("Install the App on {owner}"))
                .step(format!("App settings → Install App → Install on {owner}"))
                .field(
                    "Repository access",
                    format!("Only select repositories → {kafes_fork}"),
                )
                .step("Install; the browser then shows .../settings/installations/<number>")
                .confirm()?;
            let id = ui::text_help(
                "Installation ID",
                None,
                Some("the number at the end of the installation's URL"),
            )?;
            global.app.installation_id =
                Some(id.parse().context("the installation ID must be a number")?);
            save_global_config(&global)?;
        }
        Some(id) => {
            Manual::new("Give the App access to the new fork")
                .step("Open:")
                .link(format!(
                    "https://github.com/organizations/{owner}/settings/installations/{id}"
                ))
                .step(format!("Repository access → add {kafes_fork} → Save"))
                .confirm()?;
        }
    }

    // Before the repo record, which is saved last
    choose_profile(&repo.name, profile)?;

    save_repo_config(&repo)?;
    ui::done(&format!("{} saved", repo.name));
    Ok(repo)
}
