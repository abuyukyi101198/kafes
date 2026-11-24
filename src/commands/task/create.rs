use crate::agent::{push_agent_files, push_credentials};
use crate::commands::setup::rules_ok;
use crate::config::{load_global_config, load_repo_config, valid_name};
use crate::constants::{SANDBOX_PATH, TASKS_PATH, WS_HOME};
use crate::git::{anon_fetch, manual_push, remote_sha};
use crate::guard::FailureHint;
use crate::network::{bridge_state, check_workshop_network};
use crate::profile::load_profile;
use crate::services::start_services;
use crate::settings::{prepare_task_settings, save_task_settings};
use crate::state::{TaskRecord, TaskState, load_active, load_record, save_active, save_record};
use crate::ui;
use crate::utils::{home_path, run_cmd};
use crate::workshop::{cache_plugs, open_shell, reset_definition, ws, ws_exec_stdin};
use anyhow::{Context, Result, bail, ensure};
use std::fs;

fn parse_target(target: &str) -> Result<(&str, &str)> {
    let (repo, feature) = target
        .split_once('/')
        .with_context(|| format!("expected <repo>/<feature-branch>, got {target}"))?;
    ensure!(valid_name(repo), "invalid repository name: {repo}");
    run_cmd("git", &["check-ref-format", "--branch", feature])
        .with_context(|| format!("invalid branch: {feature}"))?;
    Ok((repo, feature))
}

fn task_from_branch(branch: &str) -> String {
    branch
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

pub fn cmd_task_create(target: &str, name: Option<String>) -> Result<()> {
    let (repo_name, feature) = parse_target(target)?;
    let task = name.unwrap_or_else(|| task_from_branch(feature));
    let task = task.as_str();
    ensure!(
        valid_name(task),
        "invalid task name: \"{task}\"; pass another with --name"
    );
    // The task name goes into kafes-base/<task> and kafes/<task>
    run_cmd(
        "git",
        &["check-ref-format", "--branch", &format!("kafes/{task}")],
    )
    .with_context(|| format!("invalid task name: \"{task}\" cannot be part of a branch name; pass another with --name"))?;
    let global = load_global_config()?;
    let repo = load_repo_config(repo_name)?;
    // Before anything starts: a broken profile must not leave a half-made task
    let profile = load_profile(repo_name)?;
    if let Some(active) = load_active()? {
        bail!(
            "task {} is still active; delete it first: kafes task delete {}",
            active.name,
            active.name
        );
    }
    // Task names are unique across repositories until task delete
    if let Some(existing) = load_record(task)? {
        bail!(
            "task {task} already exists on {}; delete it first or pass another --name",
            existing.repo
        );
    }
    let task_dir = home_path(TASKS_PATH)?.join(task);
    ensure!(!task_dir.exists(), "{} already exists", task_dir.display());
    let fork = repo.fork(&global);
    ensure!(
        rules_ok(&fork),
        "the kafes-base/** ruleset is missing or changed; run kafes repo add {repo_name} again"
    );
    let subnet = bridge_state(&global)?;

    ui::section(&format!("Task {task}: base branch"));
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
                &format!("Create this task's base branch {base} on {fork}, at {feature}"),
            )?;
        }
        Some(existing) => {
            ensure!(
                ui::confirm(&format!(
                    "{base} already exists at {}; reuse it?",
                    &existing[..12]
                ))?,
                "stopped"
            );
        }
    }
    // Record first: from here on, kafes task delete <task> knows the repository
    save_record(&TaskRecord {
        name: task.into(),
        repo: repo.name.clone(),
    })?;
    save_active(&TaskState {
        name: task.into(),
        stopped: false,
    })?;
    let hint = FailureHint::arm(&format!("kafes task delete {task}"));

    ui::section(&format!("Task {task}: host services"));
    start_services(&global, &repo)?;
    let settings = prepare_task_settings()?;
    save_task_settings(&settings)?;
    ui::done("broker, gateway and push proxy are running");

    ui::section(&format!("Task {task}: launch the workshop"));
    fs::create_dir_all(task_dir.join(".workshop"))?;
    reset_definition(task, &profile.dev_yaml())?;
    ws(task, &["launch", "dev"])?;
    for plug in cache_plugs(task)? {
        ui::detail(&format!("disconnecting package cache {plug}"));
        ws(task, &["disconnect", &plug])?;
    }
    ui::detail("connecting the gateway and push tunnels");
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

    ui::section(&format!("Task {task}: set up the workshop"));
    let script = fs::read_to_string(home_path(SANDBOX_PATH)?.join("setup-workshop.sh"))?;
    ws_exec_stdin(task, "cat > /tmp/setup-workshop.sh", &script)?;
    ws(task, &["exec", "dev", "--", "sh", "/tmp/setup-workshop.sh"])?;
    let tools = &profile.workshop;
    if !tools.apt.is_empty() {
        ui::info(&format!("apt packages: {}", tools.apt.join(" ")));
        let mut args = vec![
            "exec",
            "dev",
            "--",
            "sudo",
            "apt-get",
            "install",
            "-y",
            "--no-install-recommends",
        ];
        args.extend(tools.apt.iter().map(String::as_str));
        ws(task, &args)?;
    }
    for step in &tools.setup {
        ui::info(&ui::cmd(&format!("$ {step}")));
        ws(task, &["exec", "dev", "--", "sh", "-c", step])?;
    }
    push_agent_files(task, &repo.name)?;
    push_credentials(task, &settings.providers)?;
    ui::done("workshop ready");

    let src = format!("{WS_HOME}/src/{}", repo.name);
    ui::section(&format!("Task {task}: clone {fork} at {base}"));
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
    ui::done(&format!("cloned into {src} on branch {branch}"));
    if let Some(install) = &profile.project.install {
        ui::section(&format!("Task {task}: install the project"));
        ui::info(&ui::cmd(&format!("$ {install}")));
        let script = format!("cd '{src}' && {install}");
        let env: Vec<String> = profile
            .project
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let mut args = vec!["exec"];
        for e in &env {
            args.extend(["--env", e.as_str()]);
        }
        args.extend(["dev", "--", "sh", "-c", script.as_str()]);
        ws(task, &args)?;
    }
    hint.disarm();

    ui::section(&format!("Task {task} is ready"));
    ui::next(&[
        (
            format!("kafes task shell {task}"),
            "open its terminal; run kafes-omp there to start the agent",
        ),
        (
            format!("kafes task stop {task}"),
            "pause: revoke its credentials and stop the workshop",
        ),
        (
            format!("kafes task delete {task}"),
            "finish, once the agent has pushed its branch",
        ),
    ]);
    // Optional: the task is complete either way
    if ui::interactive() {
        eprintln!();
        if ui::confirm("Open the workshop terminal now?")? {
            open_shell(task)?;
        }
    }
    Ok(())
}
