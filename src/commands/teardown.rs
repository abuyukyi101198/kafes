//! `kafes teardown`: dismantle the whole sandbox

use crate::commands::repo::remove::remove_repo;
use crate::commands::task::delete::cmd_task_delete;
use crate::config::{GlobalConfig, list_repos, load_global_config, load_repo_config, valid_name};
use crate::constants::{
    ACL, BUN_PATH, GATEWAY_CFG_PATH, OMP_PATH, PUSH_CFG_PATH, SANDBOX_PATH, STATE_PATH, TASKS_PATH,
};
use crate::services::stop_services;
use crate::state::{delete_record, list_records, load_active};
use crate::ui::{self, Manual};
use crate::utils::{home_path, run_cmd, run_status, tmp_path};
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};
use std::fs;

const SANDBOX_FILES: [&str; 9] = [
    "dev.yaml",
    "setup-workshop.sh",
    "kafes-policy.yml",
    "kafes-prompt.md",
    "bunfig.toml",
    "kafes-push-proxy.ts",
    "kafes-pr-check.yml",
    "config.toml",
    "kafes",
];

pub fn cmd_teardown() -> Result<()> {
    let global = load_global_config()?;
    ui::warn(
        "this dismantles the whole sandbox: every task, every repository, the App and the local files",
    );
    if !ui::confirm_danger("Continue?", None)? {
        return Ok(());
    }

    remove_tasks()?;
    remove_repos(&global)?;
    remove_remote_credentials(&global)?;
    restore_bridge(&global)?;
    remove_local_files()?;
    remove_tools()?;

    ui::section("Teardown complete");
    Ok(())
}

fn remove_tasks() -> Result<()> {
    ui::section("Teardown 1/6: tasks and services");
    // Every recorded task, plus an active one without a record
    let mut names: Vec<String> = list_records()?.into_iter().map(|r| r.name).collect();
    if let Some(active) = load_active()?
        && !names.contains(&active.name)
    {
        names.push(active.name);
    }
    for name in names {
        if let Err(e) = cmd_task_delete(&name) {
            ui::warn(&format!("cannot delete task {name}: {e:#}"));
        }
    }

    let tasks = home_path(TASKS_PATH)?;
    if let Ok(entries) = fs::read_dir(&tasks) {
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            ensure!(
                valid_name(&name),
                "unexpected entry {}; inspect it without entering it",
                path.display()
            );
            // file_type doesn't follow symlinks, so a link here is refused, never removed through
            ensure!(
                entry.file_type()?.is_dir(),
                "{} is not a plain directory; inspect it",
                path.display()
            );
            ws(&name, &["remove", "dev"])
                .with_context(|| format!("cannot remove the workshop of {name}; fix it first"))?;
            fs::remove_dir_all(&path)?;
            delete_record(&name)?;
            ui::done(&format!("removed task {name}"));
        }
    }

    if let Err(e) = stop_services() {
        ui::warn(&format!("{e:#}"));
    }
    ui::done("no task workshops or host services remain");
    Ok(())
}

fn remove_repos(global: &GlobalConfig) -> Result<()> {
    ui::section("Teardown 2/6: repositories");
    let repos = list_repos()?;
    if repos.is_empty() {
        ui::done("none left");
    }
    for name in repos {
        let repo = load_repo_config(&name)?;
        remove_repo(global, &repo)?;
    }
    Ok(())
}

fn remove_remote_credentials(global: &GlobalConfig) -> Result<()> {
    ui::section("Teardown 3/6: remote credentials");
    let owner = &global.kafes_owner;
    let slug = global.app.bot_name.trim_end_matches("[bot]");
    Manual::new(format!("Delete the GitHub App, before deleting {owner}"))
        .step("Open:")
        .link(format!(
            "https://github.com/organizations/{owner}/settings/apps/{slug}/advanced"
        ))
        .step("Delete GitHub App")
        .confirm()?;
    Manual::new("Revoke the provider grants used for the OMP logins")
        .text("revoke only grants that belong to this setup")
        .step("OpenRouter key:")
        .link("https://openrouter.ai/settings/keys")
        .step("GitHub Copilot OAuth grant:")
        .link("https://github.com/settings/applications")
        .confirm()?;
    if ui::confirm_danger(
        "Log this host's broker out of github-copilot and openrouter?",
        Some("all of the host, not one task"),
    )? {
        let omp = home_path(OMP_PATH)?.to_string_lossy().into_owned();
        for provider in ["github-copilot", "openrouter"] {
            let args = [
                "-u",
                "OMP_AUTH_BROKER_URL",
                omp.as_str(),
                "auth-broker",
                "logout",
                provider,
            ];
            if let Err(e) = run_status("env", &args) {
                ui::warn(&format!("logout {provider}: {e:#}"));
            }
        }
    }
    Manual::new(format!("Delete {owner}, last"))
        .text("only once the App is deleted and the organization has no other use")
        .step("Open:")
        .link(format!("https://github.com/organizations/{owner}/settings/profile"))
        .step("Delete this organization")
        .confirm()
}

fn restore_bridge(global: &GlobalConfig) -> Result<()> {
    ui::section("Teardown 4/6: LXD network ACL");
    ui::info("sudo may ask for your password");
    run_status("sudo", &["-v"])?;
    if let Some(net) = &global.network
        && run_cmd("sudo", &["lxc", "network", "show", &net.bridge]).is_ok()
    {
        if net.acls_before.is_empty() {
            run_cmd(
                "sudo",
                &["lxc", "network", "unset", &net.bridge, "security.acls"],
            )?;
        } else {
            let value = format!("security.acls={}", net.acls_before);
            run_cmd("sudo", &["lxc", "network", "set", &net.bridge, &value])?;
        }
        let before = if net.acls_before.is_empty() {
            "none"
        } else {
            net.acls_before.as_str()
        };
        ui::done(&format!("{} ACLs restored to: {before}", net.bridge));
    }
    if run_cmd("sudo", &["lxc", "network", "acl", "show", ACL]).is_ok()
        && run_cmd("sudo", &["lxc", "network", "acl", "delete", ACL]).is_err()
    {
        ui::warn(&format!(
            "{ACL} is still referenced elsewhere; left in place"
        ));
    }
    Ok(())
}

fn remove_local_files() -> Result<()> {
    ui::section("Teardown 5/6: local files");
    let push = home_path(PUSH_CFG_PATH)?;
    let gateway = home_path(GATEWAY_CFG_PATH)?;
    let sandbox = home_path(SANDBOX_PATH)?;
    let state = home_path(STATE_PATH)?;

    let mut files = vec![
        push.join("app.pem"),
        push.join("client.token"),
        push.join("client.gitconfig"),
        gateway.join("clients.json"),
        state.join("active-task.toml"),
        state.join("task-models.yml"),
        state.join("task-config.yml"),
    ];
    files.extend(SANDBOX_FILES.iter().map(|f| sandbox.join(f)));
    // Leftovers of an interrupted write_atomic
    let tmps: Vec<_> = files.iter().map(|f| tmp_path(f)).collect();
    files.extend(tmps);
    for f in &files {
        let _ = fs::remove_file(f);
    }

    // Only kafes's own link; another `kafes` on PATH stays
    let link = home_path(".local/bin/kafes")?;
    if fs::read_link(&link).is_ok_and(|t| t == sandbox.join("kafes")) {
        let _ = fs::remove_file(&link);
    }

    // Innermost first; remove_dir refuses non-empty directories on purpose
    let dirs = [
        state.join("tasks"),
        state,
        sandbox.join("repos"),
        sandbox,
        home_path(TASKS_PATH)?,
        home_path("kafes")?,
        push,
        gateway,
        home_path(".config/kafes")?,
    ];
    let left: Vec<String> = dirs
        .iter()
        .filter(|d| d.exists() && fs::remove_dir(d).is_err())
        .map(|d| d.display().to_string())
        .collect();
    if !left.is_empty() {
        ui::warn(&format!(
            "not empty, inspect instead of deleting recursively: {}",
            left.join(", ")
        ));
    }
    ui::done("local files removed");
    ui::info("delete any other copy of the App key by exact path");
    Ok(())
}

fn remove_tools() -> Result<()> {
    ui::section("Teardown 6/6: sandbox-only tools (optional)");
    if ui::confirm_danger(
        "Uninstall the global OMP package?",
        Some("answer no if you use OMP elsewhere"),
    )? {
        let bun = home_path(BUN_PATH)?.to_string_lossy().into_owned();
        if let Err(e) = run_status(&bun, &["remove", "--global", "@oh-my-pi/pi-coding-agent"]) {
            ui::warn(&format!("{e:#}"));
        }
    }
    Manual::new("Remove Workshop and LXD, only if installed for kafes and holding nothing else")
        .text("never purge a shared LXD")
        .cmd("sudo snap remove workshop")
        .cmd("sudo snap remove lxd")
        .print();
    Ok(())
}
