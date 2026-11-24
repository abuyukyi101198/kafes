use crate::config::{
    GlobalConfig, RepoConfig, delete_repo_config, load_global_config, load_repo_config,
};
use crate::git::kafes_refs;
use crate::github::gh_api;
use crate::state::list_records;
use crate::ui::{self, Manual};
use crate::utils::run_cmd;
use anyhow::{Result, bail, ensure};

pub fn cmd_remove(name: &str) -> Result<()> {
    let global = load_global_config()?;
    let repo = load_repo_config(name)?;
    // Any task on it, active or left behind by a failed delete
    if let Some(task) = list_records()?.into_iter().find(|t| t.repo == repo.name) {
        bail!(
            "task {} uses {name}; delete it first: kafes task delete {}",
            task.name,
            task.name
        );
    }
    ui::warn(&format!(
        "this removes kafes from {name} only; other repositories are kept"
    ));
    ensure!(ui::confirm_danger("Continue?", None)?, "stopped");
    remove_repo(&global, &repo)
}

pub fn remove_repo(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let name = repo.name.as_str();
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    let dir = repo.checkout.to_string_lossy();

    ui::section(&format!("Remove {name}: settings on {fork}"));
    let before = repo
        .default_branch_before
        .as_deref()
        .unwrap_or("its original branch");
    Manual::new(format!("Undo the kafes settings on {fork}"))
        .step("Close leftover kafes and dummy PRs without merging")
        .step("Delete the kafes-base ruleset:")
        .link(format!("https://github.com/{fork}/settings/rules"))
        .step(format!("Restore the default branch to {before}:"))
        .link(format!("https://github.com/{fork}/settings"))
        .step("Delete the KAFES_FORK and KAFES_EMAIL variables:")
        .link(format!("https://github.com/{fork}/settings/variables/actions"))
        .step("Restore the Actions setting, if you changed it only for kafes:")
        .link(format!("https://github.com/{fork}/settings/actions"))
        .confirm()?;
    if let Some(b) = &repo.default_branch_before {
        let current = gh_api(&format!("/repos/{fork}"))?["default_branch"]
            .as_str()
            .unwrap_or("")
            .to_string();
        ensure!(
            current == *b,
            "the default branch of {fork} is {current}, not {b} yet"
        );
        ui::done(&format!("the default branch is {b} again"));
    }

    ui::section(&format!("Remove {name}: kafes branches on {fork}"));
    let refs = kafes_refs(&fork)?;
    if refs.is_empty() {
        ui::done("none left");
    } else {
        ui::list("branches", &refs);
        if ui::confirm_danger("Delete these branches?", Some("save reviewed work first"))? {
            Manual::new("Delete the branches")
                .step("Run this yourself, in another terminal, with your own credentials:")
                .cmd(format!("git -C '{dir}' push origin --delete {}", refs.join(" ")))
                .confirm()?;
            if kafes_refs(&fork)?.is_empty() {
                ui::done("branches deleted");
            } else {
                ui::warn(&format!("some kafes branches are still on {fork}"));
            }
        }
    }

    let has_local = run_cmd(
        "git",
        &[
            "-C",
            &dir,
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/kafes-checks",
        ],
    )
    .is_ok();
    if has_local
        && ui::confirm_danger(
            &format!("Delete the local kafes-checks branch in {dir}?"),
            None,
        )?
    {
        run_cmd("git", &["-C", &dir, "branch", "-D", "kafes-checks"])?;
        ui::done("local kafes-checks deleted");
    }

    ui::section(&format!("Remove {name}: the kafes fork"));
    let owner = &global.kafes_owner;
    let mut manual = Manual::new(format!("Remove {kafes_fork}"));
    if let Some(id) = global.app.installation_id {
        manual = manual
            .step("Remove it from the App's repository access:")
            .link(format!(
                "https://github.com/organizations/{owner}/settings/installations/{id}"
            ))
            .text(format!("Repository access → remove {kafes_fork} → Save"));
    }
    manual
        .step("Delete it:")
        .link(format!("https://github.com/{kafes_fork}/settings"))
        .text("→ Delete this repository")
        .text("never delete your own fork or upstream")
        .confirm()?;

    delete_repo_config(name)?;
    ui::done(&format!("{name} removed"));
    Ok(())
}
