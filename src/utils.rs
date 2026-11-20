use anyhow::{Context, Result, bail};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, MultiSelect};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub fn print(msg: &str) {
    println!("\n{}", format!("{msg}").bold());
}

pub fn print_info(msg: &str) {
    println!("  {msg}");
}

pub fn print_warning(msg: &str) {
    eprintln!("{} {msg}", "WARNING".bold().yellow());
}

pub fn prompt_confirm(msg: &str) -> Result<bool> {
    Ok(Confirm::new(msg).with_default(true).prompt()?)
}

pub fn prompt_multiselect(msg: &str, options: Vec<String>) -> Result<Vec<String>> {
    Ok(MultiSelect::new(msg, options).prompt()?)
}

pub fn check_cmd(cmd: &str) -> Result<()> {
    which::which(cmd).with_context(|| format!("{cmd} not found"))?;
    Ok(())
}

pub fn run_cmd(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to start {cmd}"))?;
    if !out.status.success() {
        bail!(
            "{cmd} {} exited with status {}: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

pub fn run_status(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("failed to start {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {} exited with status {status}", args.join(" "));
    }
    Ok(())
}

pub fn get_cmd_version(cmd: &str, args: &[&str], index: usize) -> Result<String> {
    let out = run_cmd(cmd, args)?;
    out.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(index))
        .map(str::to_string)
        .with_context(|| format!("can't read {cmd} version"))
}

pub fn step<F>(label: &str, f: F) -> Result<String>
where
    F: FnOnce() -> Result<String>,
{
    let pb = ProgressBar::new_spinner();
    pb.set_style(ProgressStyle::with_template("{spinner} {msg}")?);
    pb.set_message(label.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));

    let res = f();
    pb.set_style(ProgressStyle::with_template("{msg}")?);
    match &res {
        Ok(detail) => pb.finish_with_message(format!("{} {label} {detail}", "✔".green())),
        Err(e) => pb.finish_with_message(format!("{} {label}: {e}", "✘".red())),
    }
    res
}

pub fn home_path(rel: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(rel))
}

pub fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}
