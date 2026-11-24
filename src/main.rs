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

use crate::commands::task::{create, delete, list, shell, start, stop};
use crate::utils::check_cwd;
use clap::builder::PossibleValuesParser;
use clap::{ArgAction, Args, Parser, Subcommand};
use colored::Colorize;
use inquire::InquireError;
use std::process::ExitCode;

const HELP_TEMPLATE: &str = "\
{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}";

const TOP_EXAMPLES: &str = "\
Examples:
  kafes init
  kafes repo add my-app --profile yarn
  kafes task create my-app/feat/login
  kafes task list

Run 'kafes help <command>' for more about a command.";

fn presets() -> PossibleValuesParser {
    PossibleValuesParser::new(constants::PRESETS.iter().map(|(name, _)| *name))
}

#[derive(Parser)]
#[command(
    name = "kafes",
    version,
    about = "Run OMP coding agents in an isolated workshop, pushing only through a GitHub App",
    help_template = HELP_TEMPLATE,
    after_help = TOP_EXAMPLES,
    disable_help_flag = true,
    disable_version_flag = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(flatten)]
    global: GlobalArgs,
    /// Show the version
    #[arg(long, action = ArgAction::Version)]
    version: Option<bool>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Args)]
#[command(next_help_heading = "Global options")]
struct GlobalArgs {
    /// Show help
    #[arg(long, short_alias = 'h', action = ArgAction::Help, global = true)]
    help: Option<bool>,
    /// Show only warnings, errors, manual steps and prompts
    #[arg(long, global = true, conflicts_with = "verbose")]
    quiet: bool,
    /// Also show each execution step
    #[arg(long, global = true)]
    verbose: bool,
    /// Never prompt; stop with an error where an answer is needed
    #[arg(long, global = true)]
    no_input: bool,
    /// Plain output; colour is also off with NO_COLOR or when not on a terminal
    #[arg(long, global = true)]
    no_color: bool,
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Create a task on a feature branch of a repository
    #[command(after_help = "\
Examples:
  kafes task create my-app/feat/login           # task 'feat-login'
  kafes task create my-app/fix/42 --name fix-42")]
    Create {
        /// <repo>/<feature-branch>: an added repository and a branch on your fork
        target: String,
        /// Task name [default: the branch name, with '/' as '-']
        #[arg(long)]
        name: Option<String>,
    },
    /// List tasks and their status
    #[command(visible_alias = "ls")]
    List {
        /// Print the table without its header row
        #[arg(long)]
        no_headers: bool,
    },
    /// Open the terminal of a running task's workshop
    Shell {
        /// Task name
        task: String,
    },
    /// Pause a task: revoke its credentials and stop its workshop
    Stop {
        /// Task name
        task: String,
    },
    /// Resume a stopped task
    Start {
        /// Task name
        task: String,
    },
    /// Delete a task and its workshop, then show the review links; also the emergency stop
    Delete {
        /// Task name
        task: String,
    },
}

#[derive(Subcommand)]
enum RepoCmd {
    /// Add a repository under the kafes organization
    #[command(after_help = "\
Examples:
  kafes repo add                       # asks for the name
  kafes repo add my-app --profile yarn")]
    Add {
        /// Repository name, as on your fork; asked for if omitted
        repo: Option<String>,
        /// Built-in profile to start from [default: keep the saved profile, or ask]
        #[arg(long, value_parser = presets())]
        profile: Option<String>,
    },
    /// List the added repositories
    #[command(visible_alias = "ls")]
    List {
        /// Print the table without its header row
        #[arg(long)]
        no_headers: bool,
    },
    /// Edit a repository's profile in $VISUAL or $EDITOR, or replace it with a preset
    #[command(after_help = "\
Examples:
  kafes repo edit my-app                  # opens the profile in your editor
  kafes repo edit my-app --profile yarn   # replaces it with the preset")]
    Edit {
        /// Repository name
        repo: String,
        /// Replace the profile with this built-in preset instead of editing it
        #[arg(long, value_parser = presets())]
        profile: Option<String>,
    },
    /// Show a repository's profile and edit rules
    Show {
        /// Repository name
        repo: String,
    },
    /// Remove kafes from one repository
    Remove {
        /// Repository name
        repo: String,
    },
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up this host, the kafes organization and a first repository; safe to run again
    Init,
    /// Add, list, edit, show and remove repositories
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    /// Create, list, open, stop, start and delete tasks
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Remove kafes from this host and from GitHub
    Teardown,
    /// Show the version
    Version,
}

fn main() -> ExitCode {
    // Files created by kafes or its child processes are private by default. `umask` cannot fail
    // SAFETY: a plain syscall that only changes this process's file-creation mask
    unsafe {
        libc::umask(0o077);
    }

    let cli = Cli::parse();
    let g = &cli.global;
    let verbosity = if g.quiet {
        ui::Verbosity::Quiet
    } else if g.verbose {
        ui::Verbosity::Verbose
    } else {
        ui::Verbosity::Brief
    };
    ui::init(g.no_color, g.no_input, verbosity);

    if let Err(e) = guard::install_signal_handler() {
        ui::warn(&format!("cannot install the Ctrl+C handler: {e}"));
    }

    let result = check_cwd().and_then(|()| match cli.command {
        Cmd::Init => commands::setup::cmd_init(),
        Cmd::Repo { cmd } => match cmd {
            RepoCmd::Add { repo, profile } => commands::repo::add::cmd_add(repo, profile),
            RepoCmd::List { no_headers } => commands::repo::list::cmd_list(no_headers),
            RepoCmd::Edit { repo, profile } => commands::repo::profile::cmd_edit(&repo, profile),
            RepoCmd::Show { repo } => commands::repo::profile::cmd_show(&repo),
            RepoCmd::Remove { repo } => commands::repo::remove::cmd_remove(&repo),
        },
        Cmd::Task { cmd } => match cmd {
            TaskCmd::Create { target, name } => create::cmd_task_create(&target, name),
            TaskCmd::List { no_headers } => list::cmd_task_list(no_headers),
            TaskCmd::Shell { task } => shell::cmd_task_shell(&task),
            TaskCmd::Stop { task } => stop::cmd_task_stop(&task),
            TaskCmd::Start { task } => start::cmd_task_start(&task),
            TaskCmd::Delete { task } => delete::cmd_task_delete(&task),
        },
        Cmd::Teardown => commands::teardown::cmd_teardown(),
        Cmd::Version => {
            println!("kafes {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if matches!(
                e.downcast_ref::<InquireError>(),
                Some(InquireError::OperationCanceled | InquireError::OperationInterrupted)
            ) {
                eprintln!(
                    "\n{}",
                    "Cancelled; run the same command again to continue.".yellow()
                );
                return ExitCode::from(130);
            }
            // Outermost context first, then each cause on its own line
            eprintln!("\n{} {e}", "error:".red().bold());
            for cause in e.chain().skip(1) {
                eprintln!("  {} {cause}", "caused by:".dimmed());
            }
            ExitCode::FAILURE
        }
    }
}
