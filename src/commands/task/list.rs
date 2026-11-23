//! `kafes task list`: the tasks from task new until task end, and their state

use crate::constants::TASKS_PATH;
use crate::state::{list_records, load_active};
use crate::ui::{print, print_info};
use crate::utils::home_path;
use anyhow::Result;
use colored::Colorize;

pub fn cmd_task_list() -> Result<()> {
    let records = list_records()?;
    let active = load_active()?;
    let tasks_dir = home_path(TASKS_PATH)?;

    // (task, repo) pairs, including an active task whose record is missing
    let mut rows: Vec<(String, String)> = records
        .into_iter()
        .map(|r| (r.name, r.repo))
        .collect();
    if let Some(a) = &active
        && !rows.iter().any(|(n, _)| *n == a.name)
    {
        rows.push((a.name.clone(), "?".into()));
    }
    if rows.is_empty() {
        print_info("no tasks; start one with: kafes task new <repo>/<feature-branch>");
        return Ok(());
    }

    print("Tasks");
    let name_w = rows.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let repo_w = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
    for (name, repo) in &rows {
        let has_dir = tasks_dir.join(name).is_dir();
        let status = match &active {
            Some(a) if a.name == *name && !has_dir => {
                format!("{}; end it: kafes task end {name}", "broken".red())
            }
            Some(a) if a.name == *name && a.stopped => "stopped".yellow().to_string(),
            Some(a) if a.name == *name => "running".green().to_string(),
            _ => format!("{}; end it: kafes task end {name}", "leftover".red()),
        };
        print_info(&format!("{name:<name_w$}  {repo:<repo_w$}  {status}"));
    }
    Ok(())
}

