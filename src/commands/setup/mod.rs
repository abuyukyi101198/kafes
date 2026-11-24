//! `kafes init`: host, organization and first repository.

mod acl;
mod check;
mod files;
mod org;
mod providers;
mod tools;
mod verify;

pub use check::{setup_check, rules_ok};
pub use verify::setup_verify;

use crate::commands::repo::add::repo_stages;
use crate::ui;
use anyhow::Result;

pub fn cmd_init() -> Result<()> {
    tools::setup_tools()?;
    providers::setup_providers()?;
    org::setup_org()?;
    files::setup_proxy_files()?;
    acl::setup_acl()?;
    let name = repo_stages(None, None)?;
    ui::section("Initialised");
    ui::next(&[
        (
            format!("kafes task create {name}/<feature-branch>"),
            "start a task; --name <task> to name it",
        ),
        ("kafes repo add <repo>".into(), "add another repository"),
    ]);
    Ok(())
}

