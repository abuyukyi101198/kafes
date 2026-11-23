mod agent;
mod commands;
mod config;
mod constants;
mod git;
mod github;
mod guard;
mod network;
mod services;
mod settings;
mod state;
mod ui;
mod utils;
mod workshop;

use crate::commands::task::{end, new, shell, start, stop};
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
enum TaskCmd {
    New {
        task: String,
        feature: String,
        #[arg(long)]
        repo: Option<String>,
    },
    Shell {
        task: String,
    },
    Stop {
        task: String,
    },
    Start {
        task: String,
    },
    End {
        task: String,
        #[arg(long)]
        repo: Option<String>,
    },
}

#[derive(Subcommand)]
enum Cmd {
    Setup,
    Add {
        repo: Option<String>,
    },
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    Remove {
        repo: String,
    },
    Teardown,
}

fn main() -> ExitCode {
    // Files created by kafes or its child processes are private by default. `umask` can't fail
    // SAFETY: a plain syscall that only changes this process's file-creation mask
    unsafe {
        libc::umask(0o077);
    }

    if let Err(e) = guard::install_signal_handler() {
        eprintln!("can't install the Ctrl+C handler: {e}");
    }

    let cli = Cli::parse();
    let result = check_cwd().and_then(|()| match cli.command {
        Cmd::Setup => commands::setup::cmd_setup(),
        Cmd::Add { repo } => commands::add::cmd_add(repo),
        Cmd::Task { cmd } => match cmd {
            TaskCmd::New {
                task,
                feature,
                repo,
            } => new::cmd_task_new(&task, &feature, repo),
            TaskCmd::Shell { task } => shell::cmd_task_shell(&task),
            TaskCmd::Stop { task } => stop::cmd_task_stop(&task),
            TaskCmd::Start { task } => start::cmd_task_start(&task),
            TaskCmd::End { task, repo } => end::cmd_task_end(&task, repo),
        },
        Cmd::Remove { repo } => commands::remove::cmd_remove(&repo),
        Cmd::Teardown => commands::teardown::cmd_teardown(),
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
