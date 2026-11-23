use crate::agent::{push_agent_files, push_credentials};
use crate::commands::setup::rules_ok;
use crate::config::{load_global_config, pick_repo, valid_name};
use crate::constants::{SANDBOX_PATH, TASKS_PATH, WS_HOME};
use crate::git::{anon_fetch, manual_push, remote_sha};
use crate::guard::FailureHint;
use crate::network::{bridge_state, check_workshop_network};
use crate::services::start_services;
use crate::settings::{prepare_task_settings, save_task_settings};
use crate::state::{TaskState, load_active, save_active};
use crate::ui::{print, print_info, prompt_confirm};
use crate::utils::{home_path, run_cmd};
use crate::workshop::{cache_plugs, open_shell, ws, ws_exec_stdin};
use anyhow::{Context, Result, bail, ensure};
use std::fs;

pub fn cmd_task_new(task: &str, feature: &str, repo: Option<String>) -> Result<()> {
    ensure!(valid_name(task), "invalid task name: {task}");
    run_cmd("git", &["check-ref-format", "--branch", feature])
        .with_context(|| format!("invalid branch: {feature}"))?;
    let global = load_global_config()?;
    let repo = pick_repo(repo)?;
    if let Some(active) = load_active()? {
        bail!("task {} is still active; end it first", active.name);
    }
    let task_dir = home_path(TASKS_PATH)?.join(task);
    ensure!(!task_dir.exists(), "{} already exists", task_dir.display());
    let fork = repo.fork(&global);
    ensure!(
        rules_ok(&fork),
        "the kafes-base/** ruleset is missing or changed; re-run setup"
    );
    let subnet = bridge_state(&global)?;

    print("Base branch");
    let base = format!("kafes-base/{task}");
    let sha =
        remote_sha(&fork, feature)?.with_context(|| format!("{feature} not found on {fork}"))?;
    match remote_sha(&fork, &base)? {
        None => {
            anon_fetch(&repo.checkout, &fork, feature)?;
            manual_push(
                &repo,
                &fork,
                &sha,
                &format!("{sha}:refs/heads/{base}"),
                &format!("Create this task's base branch {base} on {fork}, at {feature}."),
            )?;
            print_info(&format!("created {base} at {feature} ({sha})"));
        }
        Some(existing) => {
            ensure!(
                prompt_confirm(&format!("{base} already exists at {existing}; reuse it?"))?,
                "stopped"
            );
        }
    }
    save_active(&TaskState {
        name: task.into(),
        repo: repo.name.clone(),
        stopped: false,
    })?;
    let hint = FailureHint::arm(&format!("kafes task end {task}"));

    print("Host services");
    start_services(&global, &repo)?;
    let settings = prepare_task_settings()?;
    save_task_settings(&settings)?;
    print("Launch the task workshop");
    let def_dir = task_dir.join(".workshop");
    fs::create_dir_all(&def_dir)?;
    fs::copy(
        home_path(SANDBOX_PATH)?.join("dev.yaml"),
        def_dir.join("dev.yaml"),
    )?;
    ws(task, &["launch", "dev"])?;
    print_info("reading the workshop's connections");
    for plug in cache_plugs(task)? {
        print_info(&format!("disconnecting package cache {plug}"));
        ws(task, &["disconnect", &plug])?;
    }
    print_info("connecting the gateway and push tunnels");
    ws(
        task,
        &[
            "connect",
            "dev/node:kafes-gateway",
            "dev/system:kafes-gateway",
        ],
    )?;
    ws(
        task,
        &["connect", "dev/node:kafes-push", "dev/system:kafes-push"],
    )?;
    check_workshop_network(task, &subnet)?;

    print("Set up the workshop");
    let script = fs::read_to_string(home_path(SANDBOX_PATH)?.join("setup-workshop.sh"))?;
    ws_exec_stdin(task, "cat > /tmp/setup-workshop.sh", &script, &[])?;
    ws(task, &["exec", "dev", "--", "sh", "/tmp/setup-workshop.sh"])?;
    push_agent_files(task, &repo.name)?;
    push_credentials(task, &settings.providers)?;

    let src = format!("{WS_HOME}/src/{}", repo.name);
    print(&format!("Clone {fork} at {base}"));
    let url = format!("https://github.com/{fork}.git");
    let branch = format!("kafes/{task}");
    let remote = format!("http://localhost:4100/{}.git", repo.kafes_fork(&global));
    let git = |args: &[&str]| -> Result<()> {
        let mut full = vec!["exec", "dev", "--", "git", "-C", &src];
        full.extend_from_slice(args);
        ws(task, &full)
    };
    ws(
        task,
        &[
            "exec", "dev", "--", "git", "clone", "--branch", &base, &url, &src,
        ],
    )?;
    git(&["switch", "-c", &branch])?;
    git(&["remote", "add", "kafes", &remote])?;
    git(&["config", "user.name", &global.app.bot_name])?;
    git(&["config", "user.email", &global.app.bot_email])?;
    let install = format!("cd '{src}' && ~/.local/bin/yarn install --frozen-lockfile");
    ws(
        task,
        &[
            "exec",
            "--env",
            "CYPRESS_INSTALL_BINARY=0",
            "dev",
            "--",
            "sh",
            "-c",
            &install,
        ],
    )?;
    hint.disarm();

    print(&format!("Task {task} is ready"));
    print_info(&format!(
        "kafes task shell {task}   open its terminal; run kafes-omp there to start the agent"
    ));
    print_info(&format!(
        "kafes task stop {task}    pause: revoke its credentials and stop the workshop"
    ));
    print_info(&format!(
        "kafes task end {task}     finish, once the agent has pushed {branch}"
    ));
    if prompt_confirm("Open the workshop terminal now?")? {
        open_shell(task)?;
    }
    Ok(())
}
