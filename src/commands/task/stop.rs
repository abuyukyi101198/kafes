use crate::services::{revoke_gateway_clients, stop_proxy, stop_services};
use crate::state::{Want, require_task, save_active};
use crate::ui::{print, print_info};
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};

pub fn cmd_task_stop(task: &str) -> Result<()> {
    let mut state = require_task(task, Want::Any)?;

    print("Revoke credentials and stop host services");
    let clean = stop_proxy();
    revoke_gateway_clients()?;
    stop_services()?;

    print("Stop the workshop");
    if state.stopped {
        print_info("already stopped");
    } else {
        ws(task, &["stop", "dev"])
            .context("workshop stop failed; credentials are revoked, the workshop still runs")?;
        state.stopped = true;
        save_active(&state)?;
        print_info("stopped; its files, OMP sessions and installed packages are kept");
    }
    print_info(&format!("Resume with: kafes task start {task}"));
    ensure!(
        clean,
        "the push proxy did not stop cleanly; a token may stay valid for up to 1 h"
    );
    Ok(())
}
