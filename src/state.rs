use crate::config::valid_name;
use crate::constants::{STATE_PATH, TASKS_PATH};
use crate::utils::{home_path, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, read_to_string, remove_file};
use std::io::ErrorKind;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct TaskState {
    pub name: String,
    pub repo: String,
    pub stopped: bool,
}

fn active_path() -> Result<PathBuf> {
    Ok(home_path(STATE_PATH)?.join("active-task.toml"))
}

pub fn load_active() -> Result<Option<TaskState>> {
    let path = active_path()?;
    match read_to_string(&path) {
        Ok(text) => Ok(Some(
            toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?,
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn save_active(state: &TaskState) -> Result<()> {
    create_dir_all(home_path(STATE_PATH)?)?;
    write_atomic(&active_path()?, &toml::to_string(state)?)
}

pub fn clear_active() -> Result<()> {
    match remove_file(active_path()?) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

pub enum Want {
    Running,
    Stopped,
    Any,
}

pub fn require_task(name: &str, want: Want) -> Result<TaskState> {
    ensure!(valid_name(name), "invalid task name: {name}");
    let state = match load_active()? {
        Some(s) if s.name == name => s,
        other => {
            let active = other.map_or("none".to_string(), |s| s.name);
            bail!("{name} is not the active task (active: {active})");
        }
    };
    ensure!(
        home_path(TASKS_PATH)?.join(name).is_dir(),
        "~/kafes/tasks/{name} is missing; end the task with: kafes task end {name}"
    );
    match want {
        Want::Running => ensure!(
            !state.stopped,
            "{name} is stopped; resume it with: kafes task start {name}"
        ),
        Want::Stopped => ensure!(state.stopped, "{name} is already running"),
        Want::Any => {}
    }

    Ok(state)
}
