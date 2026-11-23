use crate::config::{
    GlobalConfig, RepoConfig, delete_repo_config, load_global_config, load_repo_config,
};
use crate::git::kafes_refs;
use crate::github::gh_api;
use crate::state::load_active;
use crate::ui::{print, print_info, print_warning, prompt_confirm, prompt_instruction};
use crate::utils::run_cmd;
use anyhow::{Result, ensure};

pub fn cmd_remove(name: &str) -> Result<()> {
    let global = load_global_config()?;
    let repo = load_repo_config(name)?;
    if let Some(active) = load_active()? {
        ensure!(
            active.repo != repo.name,
            "task {} uses {name}; end it first: kafes task end {}",
            active.name,
            active.name
        );
    }
    print_warning(&format!(
        "This removes kafes from {name} only; other repositories are kept."
    ));
    ensure!(prompt_confirm("Continue?")?, "stopped");
    remove_repo(&global, &repo)
}

pub fn remove_repo(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let name = repo.name.as_str();
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    let dir = repo.checkout.to_string_lossy();

    print(&format!("1. Settings on {fork}"));
    let before = repo
        .default_branch_before
        .as_deref()
        .unwrap_or("its original branch");
    prompt_instruction(&[
        &format!("Close leftover kafes and dummy PRs on {fork} without merging."),
        &format!("Delete the kafes-base/** ruleset: https://github.com/{fork}/settings/rules"),
        &format!("Restore the default branch of {fork} to '{before}':"),
        &format!("  https://github.com/{fork}/settings"),
        "Delete the KAFES_FORK and KAFES_EMAIL variables:",
        &format!("  https://github.com/{fork}/settings/variables/actions"),
        "Restore the Actions setting if you changed it only for this:",
        &format!("  https://github.com/{fork}/settings/actions"),
    ])?;
    if let Some(b) = &repo.default_branch_before {
        let current = gh_api(&format!("/repos/{fork}"))?["default_branch"]
            .as_str()
            .unwrap_or("")
            .to_string();
        ensure!(
            current == *b,
            "the default branch of {fork} is {current}, not {b} yet"
        );
    }

    print(&format!("2. kafes branches on {fork}"));
    let refs = kafes_refs(&fork)?;
    if refs.is_empty() {
        print_info("none left");
    } else {
        for r in &refs {
            print_info(&format!("    {r}"));
        }
        if prompt_confirm("Delete these branches? (save reviewed work first)")? {
            let push = format!("  git -C '{dir}' push origin --delete {}", refs.join(" "));
            prompt_instruction(&[
                "Delete them yourself, in another terminal, with your own credentials:",
                "",
                &push,
                "",
            ])?;
            if !kafes_refs(&fork)?.is_empty() {
                print_warning(&format!("some kafes branches are still on {fork}"));
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
    if has_local && prompt_confirm(&format!("Delete the local kafes-checks branch in {dir}?"))? {
        run_cmd("git", &["-C", &dir, "branch", "-D", "kafes-checks"])?;
        print_info("local kafes-checks deleted");
    }

    print("3. The kafes fork");
    let owner = &global.kafes_owner;
    let mut lines = Vec::new();
    if let Some(id) = global.app.installation_id {
        lines.push("Remove the kafes fork from the App's repository access:".to_string());
        lines.push(format!(
            "  https://github.com/organizations/{owner}/settings/installations/{id}"
        ));
        lines.push(format!("  Repository access: remove {kafes_fork}. Save."));
    }
    lines.push(format!(
        "Delete the kafes fork: https://github.com/{kafes_fork}/settings -> Delete this repository"
    ));
    lines.push("Never delete your own fork or upstream.".to_string());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    prompt_instruction(&refs)?;

    delete_repo_config(name)?;
    print(&format!("{name} removed"));
    Ok(())
}
