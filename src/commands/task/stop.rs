use crate::services::{revoke_gateway_clients, stop_proxy, stop_services};
use crate::state::{Want, require_task, save_active};
use crate::ui;
use crate::workshop::ws;
use anyhow::{Context, Result, ensure};

pub fn cmd_task_stop(task: &str) -> Result<()> {
    let mut state = require_task(task, Want::Any)?;

    ui::section(&format!("Stop {task}: revoke credentials and stop services"));
    let clean = stop_proxy();
    revoke_gateway_clients()?;
    stop_services()?;
    ui::done("credentials revoked; host services stopped");

    ui::section(&format!("Stop {task}: stop the workshop"));
    if state.stopped {
        ui::done("already stopped");
    } else {
        ws(task, &["stop", "dev"])
            .context("cannot stop the workshop; credentials are revoked, the workshop still runs")?;
        state.stopped = true;
        save_active(&state)?;
        ui::done("stopped; its files, OMP sessions and installed packages are kept");
    }
    ui::next(&[(format!("kafes task start {task}"), "resume it")]);
    ensure!(
        clean,
        "the push proxy did not stop cleanly; a token may stay valid for up to 1 h"
    );
    Ok(())
}
