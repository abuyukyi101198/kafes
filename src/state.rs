//! Host-side task state. Never kept in a task directory, which the agent can write

use crate::config::valid_name;
use crate::constants::{STATE_PATH, TASKS_PATH};
use crate::utils::{home_path, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, read_dir, read_to_string, remove_file};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// The one task that may run; its repository is in its TaskRecord
#[derive(Serialize, Deserialize)]
pub struct TaskState {
    pub name: String,
    pub stopped: bool,
}

/// One per task from task new until task end; names are unique across repositories
#[derive(Serialize, Deserialize)]
pub struct TaskRecord {
    pub name: String,
    pub repo: String,
}

fn active_path() -> Result<PathBuf> {
    Ok(home_path(STATE_PATH)?.join("active-task.toml"))
}

fn records_dir() -> Result<PathBuf> {
    Ok(home_path(STATE_PATH)?.join("tasks"))
}

fn record_path(name: &str) -> Result<PathBuf> {
    ensure!(valid_name(name), "invalid task name: {name}");
    Ok(records_dir()?.join(format!("{name}.toml")))
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match read_to_string(path) {
        Ok(text) => Ok(Some(
            toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?,
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn remove_if_present(path: &Path) -> Result<()> {
    match remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

pub fn load_active() -> Result<Option<TaskState>> {
    read_toml(&active_path()?)
}

pub fn save_active(state: &TaskState) -> Result<()> {
    create_dir_all(home_path(STATE_PATH)?)?;
    write_atomic(&active_path()?, &toml::to_string(state)?)
}

pub fn clear_active() -> Result<()> {
    remove_if_present(&active_path()?)
}

pub fn load_record(name: &str) -> Result<Option<TaskRecord>> {
    read_toml(&record_path(name)?)
}

pub fn save_record(record: &TaskRecord) -> Result<()> {
    create_dir_all(records_dir()?)?;
    write_atomic(&record_path(&record.name)?, &toml::to_string(record)?)
}

pub fn delete_record(name: &str) -> Result<()> {
    remove_if_present(&record_path(name)?)
}

pub fn list_records() -> Result<Vec<TaskRecord>> {
    let entries = match read_dir(records_dir()?) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if path.extension().is_some_and(|x| x == "toml")
            && valid_name(stem)
            && let Some(r) = load_record(stem)?
        {
            records.push(r);
        }
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

pub fn task_repo(name: &str) -> Result<String> {
    Ok(load_record(name)?
        .with_context(|| format!("no record of task {name}; end it with: kafes task end {name}"))?
        .repo)
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
