use crate::config::{GlobalConfig, RepoConfig};
use crate::consts::{PUSH_CFG_PATH, UNIT_PROXY};
use crate::services::{start_services, stop_services};
use crate::ui::{print, print_info, step};
use crate::utils::{home_path, run_cmd};
use anyhow::{Context, Result};

pub fn setup_verify(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    print("Verifying the services...");
    start_services(global, repo)?;
    print_info("broker, gateway and push proxy are running");

    let kafes_fork = repo.kafes_fork(global);
    let gitconfig = home_path(PUSH_CFG_PATH)?.join("client.gitconfig");
    let probe = step("push proxy", || {
        run_cmd(
            "git",
            &[
                "-c",
                &format!("include.path={}", gitconfig.display()),
                "ls-remote",
                &format!("http://localhost:4100/{kafes_fork}.git"),
                "HEAD",
            ],
        )
        .map(|_| format!("can read {kafes_fork}"))
        .with_context(|| format!("check: journalctl --user -u {UNIT_PROXY}"))
    });

    // Stop the services even when the probe failed, then report.
    let stopped = stop_services();
    probe?;
    stopped
}
