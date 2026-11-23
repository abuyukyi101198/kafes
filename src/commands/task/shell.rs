use crate::agent::push_agent_files;
use crate::state::{Want, require_task};
use crate::ui::print_info;
use crate::workshop::open_shell;
use anyhow::Result;

pub fn cmd_task_shell(task: &str) -> Result<()> {
    let state = require_task(task, Want::Running)?;
    print_info("restoring the agent's launcher, policy, prompt and model settings from the host");
    push_agent_files(task, &state.repo)?;
    open_shell(task)
}
