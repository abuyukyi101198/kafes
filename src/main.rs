mod agent;
mod commands;
mod config;
mod constants;
mod git;
mod github;
mod guard;
mod network;
mod profile;
mod services;
mod settings;
mod state;
mod ui;
mod utils;
mod workshop;

use crate::commands::task::{end, list, new, shell, start, stop};
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
    /// Start a task on a feature branch of a repository
    New {
        /// <repo>/<feature-branch>, e.g. app/feat/login
        target: String,
        /// Task name; defaults to the branch name with '/' as '-'
        #[arg(long)]
        name: Option<String>,
    },
    /// List tasks and their state
    #[command(visible_alias = "ls")]
    List,
    /// Open the task workshop's terminal
    Shell { task: String },
    /// Pause: revoke credentials and stop the workshop
    Stop { task: String },
    /// Resume a stopped task
    Start { task: String },
    /// Finish the task and remove its workshop; also the emergency stop
    End { task: String },
}

#[derive(Subcommand)]
enum RepoCmd {
    /// Add a repository under the kafes organization
    Add {
        repo: Option<String>,
        /// Built-in profile to start from (keeps a saved profile if omitted)
        #[arg(long)]
        profile: Option<String>,
    },
    /// List the added repositories
    #[command(visible_alias = "ls")]
    List,
    /// Edit a repository's profile in $EDITOR, or replace it with a preset
    Edit {
        repo: String,
        /// Replace the profile with this built-in preset instead of editing it
        #[arg(long)]
        profile: Option<String>,
    },
    /// Show a repository's effective profile and edit rules
    Show { repo: String },
    /// Remove kafes from one repository
    Remove { repo: String },
}

#[derive(Subcommand)]
enum Cmd {
    /// One-time host setup (re-runnable)
    Setup,
    /// Manage repositories
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    /// Manage the task
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Remove the whole setup
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
        Cmd::Repo { cmd } => match cmd {
            RepoCmd::Add { repo, profile } => commands::repo::add::cmd_add(repo, profile),
            RepoCmd::List => commands::repo::list::cmd_list(),
            RepoCmd::Edit { repo, profile } => commands::repo::profile::cmd_edit(&repo, profile),
            RepoCmd::Show { repo } => commands::repo::profile::cmd_show(&repo),
            RepoCmd::Remove { repo } => commands::repo::remove::cmd_remove(&repo),
        },
        Cmd::Task { cmd } => match cmd {
            TaskCmd::New { target, name } => new::cmd_task_new(&target, name),
            TaskCmd::List => list::cmd_task_list(),
            TaskCmd::Shell { task } => shell::cmd_task_shell(&task),
            TaskCmd::Stop { task } => stop::cmd_task_stop(&task),
            TaskCmd::Start { task } => start::cmd_task_start(&task),
            TaskCmd::End { task } => end::cmd_task_end(&task),
        },
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
