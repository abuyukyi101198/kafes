use crate::config::{GlobalConfig, RepoConfig};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Confirm, MultiSelect, Text, required};
use serde_json::Value;
use std::fs::{OpenOptions, remove_file, rename};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use ureq::Agent;

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

pub fn prompt_text(msg: &str, default: Option<&str>) -> Result<String> {
    let mut prompt = Text::new(msg).with_validator(required!());
    if let Some(d) = default.filter(|d| !d.is_empty()) {
        prompt = prompt.with_default(d);
    }
    Ok(prompt.prompt()?.trim().to_string())
}

pub fn prompt_instruction(lines: &[&str]) -> Result<()> {
    println!("\n{}", "[manual]".cyan());
    for line in lines {
        println!("      {line}");
    }
    if !prompt_confirm("Done?")? {
        bail!("stopped; re-run the same command once this step is done")
    }
    Ok(())
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

pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let _ = remove_file(&tmp);

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .with_context(|| format!("can't create {}", tmp.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;

    rename(&tmp, path).with_context(|| format!("can't replace {}", path.display()))?;
    Ok(())
}

pub fn gh_api(path: &str) -> Result<Value> {
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let url = format!("https://api.github.com{path}");
    let value = agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "kafes")
        .call()
        .with_context(|| format!("GET {url} failed"))?
        .body_mut()
        .read_json::<Value>()?;
    Ok(value)
}

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
    let r#ref = refspec.rsplit("refs/heads/").next().unwrap_or(refspec);
    let push = format!(
        "  git -C '{}' push origin {refspec}",
        repo.checkout.display()
    );
    let check = format!(
        "kafes then checks that {fork} has {} at {}.",
        r#ref,
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
        if remote_sha(fork, r#ref)?.as_deref() == Some(sha) {
            return Ok(());
        }
        print_warning(&format!("{fork}'s {} is not at {sha} yet", r#ref));
    }
}

fn git_stdin(dir: &Path, args: &[&str], input: &str, env: &[(&str, &str)]) -> Result<String> {
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

pub fn checks_commit(dir: &Path, workflow: &Path) -> Result<String> {
    let wf = workflow.to_string_lossy();
    let blob = git_stdin(
        dir,
        &["hash-object", "-w", "--no-filters", "--", &wf],
        "",
        &[],
    )?;
    let t1 = git_stdin(
        dir,
        &["mktree"],
        &format!("100644 blob {blob}\tkafes-pr-check.yml\n"),
        &[],
    )?;
    let t2 = git_stdin(
        dir,
        &["mktree"],
        &format!("040000 tree {t1}\tworkflows\n"),
        &[],
    )?;
    let root = git_stdin(
        dir,
        &["mktree"],
        &format!("040000 tree {t2}\t.github\n"),
        &[],
    )?;
    let epoch = "1970-01-01T00:00:00Z";
    // Same author, email, and date used to reproduce the same SHA at each run for comparison
    let env = [
        ("GIT_AUTHOR_NAME", "kafes"),
        ("GIT_AUTHOR_EMAIL", "kafes@localhost"),
        ("GIT_AUTHOR_DATE", epoch),
        ("GIT_COMMITTER_NAME", "kafes"),
        ("GIT_COMMITTER_EMAIL", "kafes@localhost"),
        ("GIT_COMMITTER_DATE", epoch),
    ];
    git_stdin(
        dir,
        &[
            "commit-tree",
            &root,
            "-m",
            "feat: added kafes PR edit-rule check",
        ],
        "",
        &env,
    )
}

const ACTIONS_APP_ID: u64 = 15368;

pub fn rules_ok(fork: &str) -> bool {
    let Ok(rules) = gh_api(&format!(
        "/repos/{fork}/rules/branches/kafes-base%2Fsetup-check"
    )) else {
        return false;
    };
    let Some(list) = rules.as_array() else {
        return false;
    };
    let has = |t: &str| list.iter().any(|r| r["type"] == t);

    has("deletion")
        && has("non_fat_forward")
        && list.iter().any(|r| {
            let p = &r["parameters"];
            r["type"] == "required_status_checks"
                && p["do_not_enforce_on_create"] == true
                && p["required_status_chacks"]
                    .as_array()
                    .is_some_and(|checks| {
                        checks.iter().any(|c| {
                            c["context"] == "kafes-pr-check"
                                && c["integration_id"] == ACTIONS_APP_ID
                        })
                    })
        })
}

pub fn bootstrap_status(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    print_info("GitHub Actions only appears as a status source once it has posted one.");

    match remote_sha(&fork, "kafes-base/setup")? {
        Some(sha) => print_info(&format!(
            "kafes-base/setup already exists on {fork} ({}); reusing it",
            &sha[..12]
        )),
        None => {
            let default_seed = repo.default_branch_before.as_deref().unwrap_or("main");
            let seed = prompt_text(
                &format!("Branch of {fork} to cut kafes-base/setup from (never kafes-checks)"),
                Some(default_seed),
            )?;
            if seed == "kafes-checks" {
                bail!("never cut kafes-base branches from kafes-checks");
            }
            let sha =
                remote_sha(&fork, &seed)?.with_context(|| format!("{seed} not found on {fork}"))?;
            anon_fetch(&repo.checkout, &fork, &seed)?;
            manual_push(
                repo,
                &fork,
                &sha,
                &format!("{sha}:refs/heads/kafes-base/setup"),
                &format!("Create the throwaway base branch kafes-base/setup on {fork}, at {seed}."),
            )?;
        }
    }

    let kafes_default = gh_api(&format!("/repos/{kafes_fork}"))?["default_branch"]
        .as_str()
        .unwrap_or("main")
        .to_string();
    let (owner, name) = (&global.kafes_owner, &repo.name);
    prompt_instruction(&[
        "Open a dummy PR from the kafes fork into kafes-base/setup:",
        "  (if one is already open from an earlier run, use it and skip to 3)",
        &format!(
            "  1. https://github.com/{kafes_fork}/new/{kafes_default}?filename=docs/kafes-setup-dummy.md"
        ),
        "     Content: anything. Choose 'Create a new branch' (e.g. setup-dummy), propose.",
        &format!(
            "  2. https://github.com/{fork}/compare/kafes-base/setup...{owner}:{name}:setup-dummy?expand=1"
        ),
        "     Create the pull request.",
        "  3. Wait for a 'kafes-pr-check' status (failing is fine), then close the PR",
        "     WITHOUT merging and delete the setup-dummy branch on the kafes fork.",
    ])
}
