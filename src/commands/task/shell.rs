use crate::agent::push_agent_files;
use crate::state::{Want, require_task, task_repo};
use crate::ui::print_info;
use crate::workshop::open_shell;
use anyhow::Result;

pub fn cmd_task_shell(task: &str) -> Result<()> {
    require_task(task, Want::Running)?;
    let repo = task_repo(task)?;
    print_info("restoring the agent's launcher, policy, prompt and model settings from the host");
    push_agent_files(task, &repo)?;
    open_shell(task)
}
