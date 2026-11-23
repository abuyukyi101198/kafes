//! `kafes setup`: host, organization and first repository.

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
use crate::ui::{print, print_info};
use anyhow::Result;

pub fn cmd_setup() -> Result<()> {
    tools::setup_tools()?;
    providers::setup_providers()?;
    org::setup_org()?;
    files::setup_proxy_files()?;
    acl::setup_acl()?;
    repo_stages(None)?;
    print("Setup complete. Start a task with:");
    print_info("kafes task new <repo>/<feature-branch> [--name <task>]");
    Ok(())
}

