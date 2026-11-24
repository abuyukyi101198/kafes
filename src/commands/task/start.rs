use crate::agent::{push_agent_files, push_credentials};
use crate::config::{load_global_config, load_repo_config};
use crate::guard::FailureHint;
use crate::network::{bridge_state, check_workshop_network};
use crate::profile::load_profile;
use crate::services::start_services;
use crate::settings::{prepare_task_settings, save_task_settings};
use crate::state::{Want, require_task, save_active, task_repo};
use crate::ui;
use crate::workshop::{reset_definition, ws};
use anyhow::Result;
use std::time::Duration;

pub fn cmd_task_start(task: &str) -> Result<()> {
    let mut state = require_task(task, Want::Stopped)?;
    let global = load_global_config()?;
    let repo_name = task_repo(task)?;
    let repo = load_repo_config(&repo_name)?;
    let profile = load_profile(&repo_name)?;
    let subnet = bridge_state(&global)?;
    let hint = FailureHint::arm(&format!("kafes task stop {task}"));

    ui::section(&format!("Start {task}: host services"));
    start_services(&global, &repo)?;
    let settings = prepare_task_settings()?;
    save_task_settings(&settings)?;
    ui::done("broker, gateway and push proxy are running");

    ui::section(&format!("Start {task}: start the workshop"));
    reset_definition(task, &profile.dev_yaml())?;
    ws(task, &["start", "dev"])?;
    state.stopped = false;
    save_active(&state)?;
    check_workshop_network(task, &subnet)?;
    push_agent_files(task, &repo.name)?;
    push_credentials(task, &settings.providers)?;
    ui::step("gateway", || {
        std::thread::sleep(Duration::from_secs(10)); // let the gateway load the new token
        Ok("loaded the new token".into())
    })?;
    hint.disarm();

    ui::section(&format!("Task {task} is running"));
    ui::next(&[(
        format!("kafes task shell {task}"),
        "then kafes-omp -c to continue the last session",
    )]);
    Ok(())
}
