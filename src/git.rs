//! Git helpers. Reads are anonymous; pushes are printed for the user to run.

use crate::config::RepoConfig;
use crate::ui::{print_warning, prompt_instruction};
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn git_anon(dir: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "credential.helper=", "-c", "core.askPass="]);
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let out = cmd.args(args).output().context("failed to run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

pub fn git_stdin(dir: &Path, args: &[&str], input: &str, env: &[(&str, &str)]) -> Result<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to start git")?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(input.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

pub fn remote_sha(repo: &str, branch: &str) -> Result<Option<String>> {
    let url = format!("https://github.com/{repo}.git");
    let out = git_anon(None, &["ls-remote", &url, &format!("refs/heads/{branch}")])?;
    Ok(out.split_whitespace().next().map(str::to_string))
}

pub fn anon_fetch(checkout: &Path, repo: &str, branch: &str) -> Result<()> {
    let url = format!("https://github.com/{repo}.git");
    git_anon(
        Some(checkout),
        &[
            "fetch",
            "-q",
            "--no-tags",
            "--no-write-fetch-head",
            &url,
            &format!("refs/heads/{branch}"),
        ],
    )?;
    Ok(())
}

pub fn manual_push(
    repo: &RepoConfig,
    fork: &str,
    sha: &str,
    refspec: &str,
    why: &str,
) -> Result<()> {
    let branch = refspec.rsplit("refs/heads/").next().unwrap_or(refspec);
    let push = format!(
        "  git -C '{}' push origin {refspec}",
        repo.checkout.display()
    );
    let check = format!(
        "kafes then checks that {fork} has {branch} at {}.",
        &sha[..12]
    );
    loop {
        prompt_instruction(&[
            why,
            "Run this push yourself, in another terminal:",
            "",
            &push,
            "",
            &check,
        ])?;
        if remote_sha(fork, branch)?.as_deref() == Some(sha) {
            return Ok(());
        }
        print_warning(&format!("{fork}'s {branch} is not at {sha} yet"));
    }
}

pub fn kafes_refs(repo: &str) -> Result<Vec<String>> {
    let url = format!("https://github.com/{repo}.git");
    let out = git_anon(
        None,
        &[
            "ls-remote",
            &url,
            "refs/heads/kafes-base/*",
            "refs/heads/kafes-checks",
        ],
    )?;
    Ok(out
        .lines()
        .filter_map(|l| l.split_whitespace().nth(1))
        .map(str::to_string)
        .collect())
}
