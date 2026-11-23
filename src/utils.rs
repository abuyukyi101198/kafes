//! Process and filesystem helpers.

use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn home_path(rel: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(rel))
}

pub fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("can't create {}", tmp.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;

    fs::rename(&tmp, path).with_context(|| format!("can't replace {}", path.display()))?;
    Ok(())
}

pub fn write_atomic_mode(path: &Path, contents: &str, mode: u32) -> Result<()> {
    write_atomic(path, contents)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("can't chmod {}", path.display()))
}
