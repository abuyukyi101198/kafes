//! `kafes repo list`: the repositories added to kafes

use crate::config::{list_repos, load_global_config, load_repo_config};
use crate::state::list_records;
use crate::ui::{self, Tone};
use anyhow::Result;

/// The table goes to stdout, so it can be piped; the empty state goes to stderr
pub fn cmd_list(no_headers: bool) -> Result<()> {
    let names = list_repos()?;
    if names.is_empty() {
        eprintln!("No repositories added.");
        ui::next(&[("kafes repo add <repo>".into(), "add one")]);
        return Ok(());
    }
    let global = load_global_config().ok();
    let records = list_records()?;

    let dash = || ("-".to_string(), Tone::Plain);
    let rows: Vec<Vec<(String, Tone)>> = names
        .into_iter()
        .map(|name| match load_repo_config(&name) {
            Ok(repo) => {
                let tasks: Vec<&str> = records
                    .iter()
                    .filter(|t| t.repo == repo.name)
                    .map(|t| t.name.as_str())
                    .collect();
                let tasks = if tasks.is_empty() {
                    "-".to_string()
                } else {
                    tasks.join(",")
                };
                let (fork, kafes_fork) = match &global {
                    Some(g) => (repo.fork(g), repo.kafes_fork(g)),
                    None => ("-".into(), "-".into()),
                };
                vec![
                    (name, Tone::Bold),
                    (fork, Tone::Plain),
                    (kafes_fork, Tone::Plain),
                    (tasks, Tone::Plain),
                    (repo.checkout.display().to_string(), Tone::Plain),
                    ("-".into(), Tone::Plain),
                ]
            }
            Err(e) => {
                ui::warn(&format!("{name}: {e:#}"));
                vec![
                    (name, Tone::Bold),
                    dash(),
                    dash(),
                    dash(),
                    dash(),
                    ("config-unreadable".into(), Tone::Bad),
                ]
            }
        })
        .collect();
    ui::table(
        &["Repo", "Fork", "Kafes-fork", "Tasks", "Checkout", "Notes"],
        &rows,
        no_headers,
    );
    Ok(())
}

