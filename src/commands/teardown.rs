//! `kafes teardown`: dismantle the whole sandbox

use crate::commands::repo::remove::remove_repo;
use crate::commands::task::end::cmd_task_end;
use crate::config::{GlobalConfig, list_repos, load_global_config, load_repo_config, valid_name};
use crate::constants::{
    ACL, BUN_PATH, GATEWAY_CFG_PATH, OMP_PATH, PUSH_CFG_PATH, SANDBOX_PATH, STATE_PATH, TASKS_PATH,
};
use crate::services::stop_services;
use crate::state::{delete_record, list_records, load_active};
use crate::ui::{print, print_info, print_warning, prompt_confirm, prompt_instruction};
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
    print_warning(
        "This dismantles the whole sandbox: every task, every repository, the App and the local files.",
    );
    if !prompt_confirm("Continue?")? {
        return Ok(());
    }

    remove_tasks()?;
    remove_repos(&global)?;
    remove_remote_credentials(&global)?;
    restore_bridge(&global)?;
    remove_local_files()?;
    remove_tools()?;

    print("Teardown complete.");
    Ok(())
}

fn remove_tasks() -> Result<()> {
    print("1. Tasks and services");
    // Every recorded task, plus an active one without a record
    let mut names: Vec<String> = list_records()?.into_iter().map(|r| r.name).collect();
    if let Some(active) = load_active()? {
        if !names.contains(&active.name) {
            names.push(active.name);
        }
    }
    for name in names {
        if let Err(e) = cmd_task_end(&name) {
            print_warning(&format!("ending {name}: {e:#}"));
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
                .with_context(|| format!("workshop remove failed for {name}; fix it first"))?;
            fs::remove_dir_all(&path)?;
            delete_record(&name)?;
            print_info(&format!("removed task {name}"));
        }
    }

    if let Err(e) = stop_services() {
        print_warning(&format!("{e:#}"));
    }
    print_info("no task workshops or host services remain");
    Ok(())
}

fn remove_repos(global: &GlobalConfig) -> Result<()> {
    print("2. Repositories");
    let repos = list_repos()?;
    if repos.is_empty() {
        print_info("none left");
    }
    for name in repos {
        let repo = load_repo_config(&name)?;
        remove_repo(global, &repo)?;
    }
    Ok(())
}

fn remove_remote_credentials(global: &GlobalConfig) -> Result<()> {
    print("3. Remote credentials");
    let owner = &global.kafes_owner;
    let slug = global.app.bot_name.trim_end_matches("[bot]");
    prompt_instruction(&[
        &format!("Delete the GitHub App BEFORE deleting {owner}:"),
        &format!(
            "  https://github.com/organizations/{owner}/settings/apps/{slug}/advanced -> Delete GitHub App"
        ),
    ])?;
    prompt_instruction(&[
        "Revoke the provider grants used for the OMP logins:",
        "  OpenRouter key: https://openrouter.ai/settings/keys",
        "  GitHub Copilot OAuth grant: https://github.com/settings/applications",
        "  Revoke only grants that belong to this setup.",
    ])?;
    if prompt_confirm(
        "Log this host's broker out of github-copilot and openrouter? (all of the host, not one task)",
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
                print_warning(&format!("logout {provider}: {e:#}"));
            }
        }
    }
    prompt_instruction(&[
        &format!("LAST, once the App is deleted and {owner} has no other use, delete it:"),
        &format!("  https://github.com/organizations/{owner}/settings/profile"),
    ])
}

fn restore_bridge(global: &GlobalConfig) -> Result<()> {
    print("4. LXD network ACL");
    run_status("sudo", &["-v"])?;
    if let Some(net) = &global.network {
        if run_cmd("sudo", &["lxc", "network", "show", &net.bridge]).is_ok() {
            if net.acls_before.is_empty() {
                run_cmd(
                    "sudo",
                    &["lxc", "network", "unset", &net.bridge, "security.acls"],
                )?;
            } else {
                let value = format!("security.acls={}", net.acls_before);
                run_cmd("sudo", &["lxc", "network", "set", &net.bridge, &value])?;
            }
            print_info(&format!(
                "{} ACLs restored to '{}'",
                net.bridge, net.acls_before
            ));
        }
    }
    if run_cmd("sudo", &["lxc", "network", "acl", "show", ACL]).is_ok()
        && run_cmd("sudo", &["lxc", "network", "acl", "delete", ACL]).is_err()
    {
        print_warning(&format!(
            "{ACL} is still referenced elsewhere; left in place"
        ));
    }
    Ok(())
}

fn remove_local_files() -> Result<()> {
    print("5. Local files");
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
        print_warning(&format!(
            "not empty, inspect instead of deleting recursively: {}",
            left.join(", ")
        ));
    }
    print_info("delete any other copy of the App key by exact path");
    Ok(())
}

fn remove_tools() -> Result<()> {
    print("6. Optional: sandbox-only tools");
    if prompt_confirm("Uninstall the global OMP package? (n if you use OMP elsewhere)")? {
        let bun = home_path(BUN_PATH)?.to_string_lossy().into_owned();
        if let Err(e) = run_status(&bun, &["remove", "--global", "@oh-my-pi/pi-coding-agent"]) {
            print_warning(&format!("{e:#}"));
        }
    }
    print_info("Remove Workshop and LXD only if installed for this and holding nothing else:");
    print_info("  sudo snap remove workshop; sudo snap remove lxd  (never purge a shared LXD)");
    Ok(())
}
