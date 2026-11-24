//! `kafes task list`: the tasks from task create until task delete, and their state

use crate::constants::TASKS_PATH;
use crate::state::{list_records, load_active};
use crate::ui::{self, Tone};
use crate::utils::home_path;
use anyhow::Result;

/// The table goes to stdout, so it can be piped; the empty state goes to stderr
pub fn cmd_task_list(no_headers: bool) -> Result<()> {
    let records = list_records()?;
    let active = load_active()?;
    let tasks_dir = home_path(TASKS_PATH)?;

    // (task, repo) pairs, including an active task whose record is missing
    let mut tasks: Vec<(String, Option<String>)> = records
        .into_iter()
        .map(|r| (r.name, Some(r.repo)))
        .collect();
    if let Some(a) = &active
        && !tasks.iter().any(|(n, _)| *n == a.name)
    {
        tasks.push((a.name.clone(), None));
    }
    if tasks.is_empty() {
        eprintln!("No tasks.");
        ui::next(&[(
            "kafes task create <repo>/<feature-branch>".into(),
            "start one",
        )]);
        return Ok(());
    }

    let mut needs_delete = false;
    let rows: Vec<Vec<(String, Tone)>> = tasks
        .into_iter()
        .map(|(name, repo)| {
            let is_active = active.as_ref().is_some_and(|a| a.name == name);
            let (status, tone, note) = match &active {
                _ if !is_active => ("leftover", Tone::Bad, "not-active"),
                _ if !tasks_dir.join(&name).is_dir() => ("broken", Tone::Bad, "dir-missing"),
                Some(a) if a.stopped => ("stopped", Tone::Caution, "-"),
                _ => ("running", Tone::Good, "-"),
            };
            needs_delete |= matches!(tone, Tone::Bad);
            let note = if repo.is_none() { "no-record" } else { note };
            vec![
                (name, Tone::Bold),
                (repo.unwrap_or_else(|| "-".into()), Tone::Plain),
                (status.into(), tone),
                (note.into(), Tone::Plain),
            ]
        })
        .collect();
    ui::table(&["Task", "Repo", "Status", "Notes"], &rows, no_headers);
    if needs_delete {
        ui::next(&[(
            "kafes task delete <task>".into(),
            "clean up a broken or leftover task",
        )]);
    }
    Ok(())
}

