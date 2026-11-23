mod commands;
mod config;
mod consts;
mod git;
mod github;
mod services;
mod ui;
mod utils;

use crate::utils::check_cwd;
use clap::{Parser, Subcommand};
use colored::Colorize;
use inquire::InquireError;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "kafes", version, about = "Run OMP agents in a cage")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Setup,
    Add { repo: Option<String> },
}

fn main() -> ExitCode {
    // Files created by kafes or its child processes are private by default. `umask` can't fail
    // SAFETY: a plain syscall that only changes this process's file-creation mask
    unsafe {
        libc::umask(0o077);
    }

    let cli = Cli::parse();
    let result = check_cwd().and_then(|()| match cli.command {
        Cmd::Setup => commands::setup::cmd_setup(),
        Cmd::Add { repo } => commands::add::cmd_add(repo),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if matches!(
                e.downcast_ref::<InquireError>(),
                Some(InquireError::OperationCanceled | InquireError::OperationInterrupted)
            ) {
                eprintln!(
                    "{}",
                    "cancelled; re-run the same command to continue".yellow()
                );
                return ExitCode::from(130);
            }
            eprintln!("{} {e:#}", "ERROR:".red().bold());
            ExitCode::FAILURE
        }
    }
}
