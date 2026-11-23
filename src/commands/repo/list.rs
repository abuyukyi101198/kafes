//! `kafes repo list`: the repositories added to kafes

use crate::config::{list_repos, load_global_config, load_repo_config};
use crate::state::list_records;
use crate::ui::{print, print_info, print_warning};
use anyhow::Result;

pub fn cmd_list() -> Result<()> {
    let names = list_repos()?;
    if names.is_empty() {
        print_info("no repositories; add one with: kafes repo add <repo>");
        return Ok(());
    }
    let global = load_global_config().ok();
    let records = list_records()?;

    print("Repositories");
    let width = names.iter().map(String::len).max().unwrap_or(0);
    for name in &names {
        let repo = match load_repo_config(name) {
            Ok(r) => r,
            Err(e) => {
                print_warning(&format!("{name}: {e:#}"));
                continue;
            }
        };
        let tasks: Vec<&str> = records
            .iter()
            .filter(|t| t.repo == repo.name)
            .map(|t| t.name.as_str())
            .collect();
        let tasks = if tasks.is_empty() {
            "no tasks".to_string()
        } else {
            format!("tasks: {}", tasks.join(", "))
        };
        let forks = global
            .as_ref()
            .map(|g| format!("{} -> {}  ", repo.fork(g), repo.kafes_fork(g)))
            .unwrap_or_default();
        print_info(&format!(
            "{name:<width$}  {forks}{}  ({tasks})",
            repo.checkout.display()
        ));
    }
    Ok(())
}

