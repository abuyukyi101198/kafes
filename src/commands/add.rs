//! `kafes add [repo]`: add a repository under the kafes organization.

use crate::commands::setup::{setup_check, setup_verify};
use crate::config::{
    RepoConfig, load_global_config, load_repo_config, save_global_config, save_repo_config,
    valid_name,
};
use crate::github::gh_api;
use crate::ui::{print, print_info, prompt_instruction, prompt_text};
use crate::utils::run_cmd;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub fn cmd_add(name: Option<String>) -> Result<()> {
    load_global_config().context("run kafes setup first")?;
    repo_stages(name)?;
    print("Repository added.");
    Ok(())
}

pub fn repo_stages(name: Option<String>) -> Result<()> {
    let mut repo = add_repo(name)?;
    // Loaded after add_repo, which may have saved the installation ID
    let global = load_global_config()?;
    setup_check(&global, &mut repo)?;
    setup_verify(&global, &repo)
}

fn add_repo(name: Option<String>) -> Result<RepoConfig> {
    let mut global = load_global_config()?;
    let name = match name {
        Some(n) => n,
        None => prompt_text("Repository name", None)?,
    };
    if !valid_name(&name) {
        bail!("invalid repository name");
    }
    print(&format!("Adding repository {name}"));
    let prev = load_repo_config(&name).ok();

    let default_checkout = prev
        .as_ref()
        .map(|p| p.checkout.to_string_lossy().into_owned());
    let checkout = prompt_text(
        &format!(
            "Absolute path of your local checkout of {}/{name}",
            global.gh_user
        ),
        default_checkout.as_deref(),
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

    let kafes_fork = repo.kafes_fork(&global);
    if gh_api(&format!("/repos/{kafes_fork}")).is_err() {
        let url = format!("  https://github.com/{fork}/fork");
        let owner = format!(
            "  Owner: {}. Repository name: {} (keep it).",
            global.kafes_owner, repo.name
        );
        prompt_instruction(&[
            "Fork YOUR fork (not upstream) into the kafes organization:",
            &url,
            &owner,
        ])?;
    }
    let info = gh_api(&format!("/repos/{kafes_fork}")).context("create the kafes fork first")?;
    let parent = info["parent"]["full_name"].as_str().unwrap_or("nothing");
    if parent != fork {
        bail!("{kafes_fork} is a fork of {parent}, not of {fork}");
    }
    let url = format!("  https://github.com/{kafes_fork}/settings/actions");
    prompt_instruction(&[
        "Disable Actions on the kafes fork:",
        &url,
        "  Actions permissions: 'Disable actions'. Save.",
    ])?;

    let owner = &global.kafes_owner;
    match global.app.installation_id {
        None => {
            let line = format!("  Repository access: Only select repositories -> {kafes_fork}");
            prompt_instruction(&[
                &format!("Install the App: App settings -> Install App -> Install on {owner}."),
                &line,
                "The browser then shows .../settings/installations/<number>.",
            ])?;
            let id = prompt_text("Installation ID", None)?;
            global.app.installation_id =
                Some(id.parse().context("the installation ID must be a number")?);
            save_global_config(&global)?;
        }
        Some(id) => {
            let url =
                format!("  https://github.com/organizations/{owner}/settings/installations/{id}");
            let line = format!("  Repository access: add {kafes_fork}. Save.");
            prompt_instruction(&["Give the App access to the new fork:", &url, &line])?;
        }
    }

    save_repo_config(&repo)?;
    print_info(&format!("{} added", repo.name));
    Ok(repo)
}
