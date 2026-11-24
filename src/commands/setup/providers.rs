use crate::constants::{GATEWAY_CFG_PATH, OMP_PATH, SANDBOX_PATH, TASKS_PATH};
use crate::ui;
use crate::utils::{home_path, run_status, write_atomic};
use anyhow::Result;
use std::fs::create_dir_all;

const PROVIDERS: [&str; 2] = ["github-copilot", "openrouter"];

pub fn setup_providers() -> Result<()> {
    ui::section("Model providers");

    let chosen = ui::multiselect(
        "Providers to log in to",
        PROVIDERS.iter().map(|p| p.to_string()).collect(),
    )?;

    let omp = home_path(OMP_PATH)?;
    let omp = omp.to_string_lossy();
    for provider in chosen {
        run_status(
            "env",
            &[
                "-u",
                "OMP_AUTH_BROKER_URL",
                "-u",
                "OPENROUTER_API_KEY",
                &omp,
                "auth-broker",
                "login",
                provider.as_str(),
            ],
        )?;
    }

    let gateway_cfg = home_path(GATEWAY_CFG_PATH)?;
    create_dir_all(&gateway_cfg)?;
    let clients = gateway_cfg.join("clients.json");
    if !clients.exists() {
        write_atomic(&clients, "{}\n")?;
    }
    create_dir_all(home_path(TASKS_PATH)?)?;
    create_dir_all(home_path(SANDBOX_PATH)?)?;
    Ok(())
}

