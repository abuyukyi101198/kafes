use crate::config::{
    GlobalConfig, RepoConfig, load_global_config, load_repo_config, pick_repo, valid_name,
};
use crate::constants::{STATE_PATH, TASKS_PATH};
use crate::git::remote_sha;
use crate::services::{revoke_gateway_clients, stop_proxy, stop_services};
use crate::state::{TaskState, clear_active, load_active};
use crate::ui::{print, print_info, print_warning};
use crate::utils::home_path;
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};
use std::fs;

pub fn cmd_task_end(task: &str, repo: Option<String>) -> Result<()> {
    ensure!(valid_name(task), "invalid task name: {task}");

    print("Stop host services and revoke credentials");
    let clean = stop_proxy();
    revoke_gateway_clients()?;
    if let Err(e) = stop_services() {
        print_warning(&format!("{e:#}"));
    }

    // A corrupt state file must not block the rest of the cleanup
    let active = match load_active() {
        Ok(a) => a.filter(|s| s.name == task),
        Err(e) => {
            print_warning(&format!("{e:#}"));
            None
        }
    };

    print("Remove the workshop");
    let dir = home_path(TASKS_PATH)?.join(task);
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {
            ws(task, &["remove", "dev"]).with_context(|| {
                format!(
                    "workshop remove failed; keep {} and fix it (see 'sudo lxc list')",
                    dir.display()
                )
            })?;
            fs::remove_dir_all(&dir)?;
            print_info(&format!("workshop and {} removed", dir.display()));
        }
        Ok(_) => print_warning(&format!(
            "{} is not a plain directory; left in place, inspect it without entering it",
            dir.display()
        )),
        Err(_) => print_warning(&format!(
            "{} is gone; check 'sudo lxc list' for a leftover container",
            dir.display()
        )),
    }
    if active.is_some() {
        clear_active()?;
        let state = home_path(STATE_PATH)?;
        let _ = fs::remove_file(state.join("task-models.yml"));
        let _ = fs::remove_file(state.join("task-config.yml"));
    }

    print("Review");
    match resolve_repo(active.as_ref(), repo) {
        Ok((global, repo)) => {
            if let Err(e) = review(task, &global, &repo) {
                print_warning(&format!("review lookup failed: {e:#}"));
            }
        }
        Err(e) => print_warning(&format!("can't show the review links: {e:#}")),
    }

    ensure!(
        clean,
        "the push proxy did not stop cleanly; a token may stay valid for up to 1 h"
    );
    Ok(())
}

fn resolve_repo(
    active: Option<&TaskState>,
    repo: Option<String>,
) -> Result<(GlobalConfig, RepoConfig)> {
    let global = load_global_config()?;
    let repo = match active {
        Some(s) => load_repo_config(&s.repo)?,
        None => pick_repo(repo)?,
    };
    Ok((global, repo))
}

fn review(task: &str, global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    let branch = format!("kafes/{task}");
    match remote_sha(&kafes_fork, &branch)? {
        None => print_info(&format!("the agent pushed no {branch} branch")),
        Some(head) => {
            let (owner, name) = (&global.kafes_owner, &repo.name);
            print_info(
                "Open the PR as yourself (don't fetch or check out the branch on the host):",
            );
            print_info(&format!(
                "  https://github.com/{fork}/compare/kafes-base/{task}...{owner}:{name}:{branch}?expand=1"
            ));
            print_info(&format!("{branch} head now: {head}"));
            print_info(&format!(
                "Merge only if kafes-pr-check passed AND the PR's head commit is still {head}."
            ));
            print_info(
                "Read every file under 'Review closely' line by line; tests are code that runs.",
            );
            print_info(&format!(
                "Then merge kafes-base/{task} into your feature branch on GitHub, and pull:"
            ));
            print_info(&format!(
                "  https://github.com/{fork}/compare/<feature-branch>...kafes-base/{task}?expand=1"
            ));
        }
    }
    Ok(())
}
