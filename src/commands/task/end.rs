use crate::config::{load_global_config, load_repo_config, pick_repo, valid_name};
use crate::constants::{STATE_PATH, TASKS_PATH};
use crate::git::remote_sha;
use crate::services::{revoke_gateway_clients, stop_proxy, stop_services};
use crate::state::{clear_active, load_active};
use crate::ui::{print, print_info, print_warning};
use crate::utils::home_path;
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};
use std::fs;

pub fn cmd_task_end(task: &str, repo: Option<String>) -> Result<()> {
    ensure!(valid_name(task), "invalid task name: {task}");
    let global = load_global_config()?;
    let active = load_active()?.filter(|s| s.name == task);
    let repo = match &active {
        Some(s) => load_repo_config(&s.repo)?,
        None => pick_repo(repo)?,
    };

    print("Stop host services and revoke credentials");
    let clean = stop_proxy();
    revoke_gateway_clients()?;
    stop_services()?;

    print("Remove the workshop");
    let dir = home_path(TASKS_PATH)?.join(task);
    let is_plain_dir = fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir());
    if is_plain_dir {
        ws(task, &["remove", "dev"]).with_context(|| {
            format!(
                "workshop remove failed; keep {} and fix it (see 'sudo lxc list')",
                dir.display()
            )
        })?;
        fs::remove_dir_all(&dir)?;
        print_info(&format!("workshop and {} removed", dir.display()));
    } else {
        print_warning(&format!(
            "{} is gone; check 'sudo lxc list' for a leftover container",
            dir.display()
        ));
    }
    if active.is_some() {
        clear_active()?;
        let state = home_path(STATE_PATH)?;
        let _ = fs::remove_file(state.join("task-models.yml"));
        let _ = fs::remove_file(state.join("task-config.yml"));
    }

    print("Review");
    let fork = repo.fork(&global);
    let kafes_fork = repo.kafes_fork(&global);
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
    ensure!(
        clean,
        "the push proxy did not stop cleanly; a token may stay valid for up to 1 h"
    );
    Ok(())
}
