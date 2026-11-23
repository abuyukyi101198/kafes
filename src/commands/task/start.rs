use crate::agent::{push_agent_files, push_credentials};
use crate::config::{load_global_config, load_repo_config};
use crate::guard::FailureHint;
use crate::network::{bridge_state, check_workshop_network};
use crate::services::start_services;
use crate::settings::{prepare_task_settings, save_task_settings};
use crate::state::{Want, require_task, save_active};
use crate::ui::{print, print_info};
use crate::workshop::{reset_definition, ws};
use anyhow::Result;
use std::time::Duration;

pub fn cmd_task_start(task: &str) -> Result<()> {
    let mut state = require_task(task, Want::Stopped)?;
    let global = load_global_config()?;
    let repo = load_repo_config(&state.repo)?;
    let subnet = bridge_state(&global)?;
    let hint = FailureHint::arm(&format!("kafes task stop {task}"));

    print("Host services");
    start_services(&global, &repo)?;
    let settings = prepare_task_settings()?;
    save_task_settings(&settings)?;

    print("Start the task workshop");
    reset_definition(task)?;
    ws(task, &["start", "dev"])?;
    state.stopped = false;
    save_active(&state)?;
    check_workshop_network(task, &subnet)?;
    push_agent_files(task, &repo.name)?;
    push_credentials(task, &settings.providers)?;
    std::thread::sleep(Duration::from_secs(10)); // let the gateway load the new token
    hint.disarm();

    print(&format!("Task {task} is running"));
    print_info(&format!(
        "kafes task shell {task}, then kafes-omp -c to continue the last session"
    ));
    Ok(())
}
