use crate::config::{GlobalConfig, RepoConfig, load_global_config, load_repo_config, valid_name};
use crate::constants::{STATE_PATH, TASKS_PATH};
use crate::git::remote_sha;
use crate::services::{revoke_gateway_clients, stop_proxy, stop_services};
use crate::state::{clear_active, delete_record, load_active, load_record};
use crate::ui::{self, Manual};
use crate::utils::home_path;
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};
use std::fs;

/// Also the emergency stop: credentials are revoked and services stopped
/// before any config or state that could fail is read
pub fn cmd_task_delete(task: &str) -> Result<()> {
    ensure!(valid_name(task), "invalid task name: \"{task}\"");

    ui::section(&format!("Delete {task}: revoke credentials and stop services"));
    let clean = stop_proxy();
    revoke_gateway_clients()?;
    if let Err(e) = stop_services() {
        ui::warn(&format!("{e:#}"));
    }
    ui::done("credentials revoked; host services stopped");

    // Corrupt state files must not block the rest of the cleanup
    let active = load_active()
        .inspect_err(|e| ui::warn(&format!("{e:#}")))
        .ok()
        .flatten()
        .filter(|s| s.name == task);
    let repo = load_record(task)
        .inspect_err(|e| ui::warn(&format!("{e:#}")))
        .ok()
        .flatten()
        .map(|r| r.repo);

    ui::section(&format!("Delete {task}: remove the workshop"));
    let dir = home_path(TASKS_PATH)?.join(task);
    let gone = match fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {
            ws(task, &["remove", "dev"]).with_context(|| {
                format!(
                    "cannot remove the workshop; keep {} and fix it (see 'sudo lxc list')",
                    dir.display()
                )
            })?;
            fs::remove_dir_all(&dir)?;
            ui::done(&format!("workshop and {} removed", dir.display()));
            true
        }
        Ok(_) => {
            ui::warn(&format!(
                "{} is not a plain directory; left in place, inspect it without entering it",
                dir.display()
            ));
            false
        }
        Err(_) => {
            ui::warn(&format!(
                "{} is gone; check 'sudo lxc list' for a leftover container",
                dir.display()
            ));
            true
        }
    };
    if active.is_some() {
        clear_active()?;
        let state = home_path(STATE_PATH)?;
        let _ = fs::remove_file(state.join("task-models.yml"));
        let _ = fs::remove_file(state.join("task-config.yml"));
    }
    // Kept while something is left in place, so the name stays taken
    if gone {
        delete_record(task)?;
    }

    ui::section(&format!("Delete {task}: review"));
    match repo {
        None => ui::warn(&format!(
            "no record of {task}'s repository; cannot show the review links"
        )),
        Some(name) => match load_global_config().and_then(|g| Ok((load_repo_config(&name)?, g))) {
            Ok((repo, global)) => {
                if let Err(e) = review(task, &global, &repo) {
                    ui::warn(&format!("cannot look up the pushed branch: {e:#}"));
                }
            }
            Err(e) => ui::warn(&format!("cannot show the review links: {e:#}")),
        },
    }

    ensure!(
        clean,
        "the push proxy did not stop cleanly; a token may stay valid for up to 1 h"
    );
    Ok(())
}

fn review(task: &str, global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    let branch = format!("kafes/{task}");
    match remote_sha(&kafes_fork, &branch)? {
        None => ui::info(&format!("the agent pushed no {branch} branch")),
        Some(head) => {
            let (owner, name) = (&global.kafes_owner, &repo.name);
            ui::fields(&[("pushed", format!("{branch} at {}", ui::em(&head)))]);
            // Printed without a Done? prompt: the review happens at the user's pace
            Manual::new("Review and merge the agent's work")
                .step("Open the PR as yourself (do not fetch or check out the branch on the host):")
                .link(format!(
                    "https://github.com/{fork}/compare/kafes-base/{task}...{owner}:{name}:{branch}?expand=1"
                ))
                .step("Read every file under 'Review closely' line by line; tests are code that runs")
                .step(format!(
                    "Merge only if kafes-pr-check passed and the PR's head commit is still {head}"
                ))
                .step(format!(
                    "Merge kafes-base/{task} into your feature branch on GitHub, then pull:"
                ))
                .link(format!(
                    "https://github.com/{fork}/compare/<feature-branch>...kafes-base/{task}?expand=1"
                ))
                .print();
        }
    }
    Ok(())
}
