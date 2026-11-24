use crate::config::{GlobalConfig, RepoConfig, save_repo_config};
use crate::constants::ACTIONS_APP_ID;
use crate::git::{anon_fetch, git_stdin, manual_push, remote_sha};
use crate::github::gh_api;
use crate::profile::{check_path, load_profile};
use crate::ui::{self, Manual};
use crate::utils::{run_cmd, write_atomic_mode};
use anyhow::{Context, Result, bail};
use std::path::Path;

pub fn setup_check(global: &GlobalConfig, repo: &mut RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let dir = repo.checkout.clone();
    ui::section(&format!("Edit-rule check on {fork}"));

    // Rendered from the repository's profile, kept for you to read
    let profile = load_profile(&repo.name)?;
    let src = check_path(&repo.name)?;
    write_atomic_mode(&src, &profile.check_workflow()?, 0o644)?;
    let want = checks_commit(&dir, &src)?;

    let remote = match remote_sha(&fork, "kafes-checks")? {
        Some(sha) => sha,
        None => {
            ui::info(&format!("workflow to publish: {}", src.display()));
            if !ui::confirm_help(
                "Have you read it?",
                "it runs with a token that can write to your fork",
            )? {
                bail!("read the workflow first");
            }
            let local = run_cmd(
                "git",
                &[
                    "-C",
                    &dir.to_string_lossy(),
                    "rev-parse",
                    "-q",
                    "--verify",
                    "refs/heads/kafes-checks",
                ],
            )
            .ok();
            match local.as_deref().map(str::trim) {
                None => {
                    run_cmd(
                        "git",
                        &[
                            "-C",
                            &dir.to_string_lossy(),
                            "update-ref",
                            "refs/heads/kafes-checks",
                            &want,
                            "",
                        ],
                    )?;
                    ui::done("created local branch kafes-checks (nothing checked out)");
                }
                Some(sha) if sha == want => ui::detail("kafes-checks already exists locally"),
                Some(sha) => {
                    bail!(
                        "a local kafes-checks at {sha} has other content; inspect it, and if kafes \
                         made it for an earlier profile, delete it: git -C '{}' branch -D kafes-checks",
                        dir.display()
                    )
                }
            }
            manual_push(
                repo,
                &fork,
                &want,
                "kafes-checks",
                &format!("Publish the edit-rule check as {fork}'s kafes-checks branch"),
            )?;
            want.clone()
        }
    };

    anon_fetch(&dir, &fork, "kafes-checks")?;
    let d = dir.to_string_lossy();
    let files = run_cmd("git", &["-C", &d, "ls-tree", "-r", "--name-only", &remote])?;
    if files.trim() != ".github/workflows/kafes-pr-check.yml" {
        bail!("{fork}'s kafes-checks holds more than the check workflow; inspect it");
    }
    let mut remote = remote;
    if run_cmd("git", &["-C", &d, "diff", "--quiet", &remote, &want, "--"]).is_err() {
        ui::warn(&format!(
            "{fork}'s kafes-checks differs from {}:",
            src.display()
        ));
        let diff = run_cmd("git", &["-C", &d, "diff", &remote, &want, "--"])?;
        ui::diff(&diff);
        if ui::confirm_danger(
            &format!("Replace {fork}'s kafes-checks with this version?"),
            Some("a force push; the old check is replaced"),
        )? {
            // kafes-checks has no parent by design, so the update replaces it
            manual_push(
                repo,
                &fork,
                &want,
                &format!("+{want}:refs/heads/kafes-checks"),
                &format!(
                    "Replace {fork}'s kafes-checks with the check for this profile (a force push)"
                ),
            )?;
            remote = want.clone();
        } else {
            ui::warn(&format!(
                "{fork}'s kafes-checks keeps its current rules; kafes PRs are checked against those"
            ));
        }
    }
    ui::done(&format!("kafes-checks is on {fork} at {}", &remote[..12]));

    let default_branch = |f: &str| -> Result<String> {
        Ok(gh_api(&format!("/repos/{f}"))?["default_branch"]
            .as_str()
            .unwrap_or("")
            .to_string())
    };
    let current = default_branch(&fork)?;
    if current != "kafes-checks" {
        if repo.default_branch_before.is_none() {
            repo.default_branch_before = Some(current.clone());
            save_repo_config(repo)?;
        }
        Manual::new(format!("Make kafes-checks the default branch of {fork}"))
            .step("Open:")
            .link(format!("https://github.com/{fork}/settings"))
            .step(format!(
                "Default branch → switch from {current} to kafes-checks"
            ))
            .confirm()?;
        if default_branch(&fork)? != "kafes-checks" {
            bail!("the default branch is still not kafes-checks");
        }
    }
    ui::done("kafes-checks is the default branch");

    if rules_ok(&fork) {
        ui::done("ruleset on kafes-base/** verified");
        return Ok(());
    }

    let kafes_fork = repo.kafes_fork(global);
    Manual::new(format!("Variables and Actions on {fork}"))
        .step("Create two repository variables (not secrets), or check they're set:")
        .link(format!("https://github.com/{fork}/settings/variables/actions"))
        .field("KAFES_FORK", kafes_fork.as_str())
        .field("KAFES_EMAIL", global.app.bot_email.as_str())
        .step("Allow Actions (forks start with workflows disabled):")
        .link(format!("https://github.com/{fork}/actions"))
        .text("→ 'I understand my workflows, go ahead and enable them'")
        .confirm()?;

    bootstrap_status(global, repo)?;

    Manual::new(format!("Create the kafes-base ruleset on {fork}"))
        .step("Open (if a kafes-base ruleset already exists, edit it instead):")
        .link(format!("https://github.com/{fork}/settings/rules/new?target=branch"))
        .step("Fill in:")
        .field("Ruleset name", "kafes-base")
        .field("Enforcement status", "Active")
        .field("Bypass list", "empty")
        .field("Target branches", "Add target → Include by pattern → kafes-base/**")
        .step("Rules:")
        .field("Restrict deletions", "on")
        .field("Block force pushes", "on")
        .field("Require status checks to pass", "on")
        .field("  Add checks", "kafes-pr-check, source 'GitHub Actions' (never 'Any source')")
        .field("  Do not require status checks on creation", "on")
        .text("if the check or source is missing, refresh the editor and add it again")
        .confirm()?;

    if !rules_ok(&fork) {
        bail!("the ruleset on kafes-base/** is not as required; fix it and re-run");
    }

    ui::done("ruleset on kafes-base/** verified");
    Ok(())
}

fn checks_commit(dir: &Path, workflow: &Path) -> Result<String> {
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
        && has("non_fast_forward")
        && list.iter().any(|r| {
            let p = &r["parameters"];
            r["type"] == "required_status_checks"
                && p["do_not_enforce_on_create"] == true
                && p["required_status_checks"]
                    .as_array()
                    .is_some_and(|checks| {
                        checks.iter().any(|c| {
                            c["context"] == "kafes-pr-check"
                                && c["integration_id"] == ACTIONS_APP_ID
                        })
                    })
        })
}

fn bootstrap_status(global: &GlobalConfig, repo: &RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let kafes_fork = repo.kafes_fork(global);
    ui::info("GitHub Actions only appears as a status source once it has posted one");

    match remote_sha(&fork, "kafes-base/setup")? {
        Some(sha) => ui::detail(&format!(
            "kafes-base/setup already exists on {fork} ({}); reusing it",
            &sha[..12]
        )),
        None => {
            let default_seed = repo.default_branch_before.as_deref().unwrap_or("main");
            let seed = ui::text_help(
                &format!("Branch of {fork} to cut kafes-base/setup from"),
                Some(default_seed),
                Some("never kafes-checks"),
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
                &format!("Create the throwaway base branch kafes-base/setup on {fork}, at {seed}"),
            )?;
        }
    }

    let kafes_default = gh_api(&format!("/repos/{kafes_fork}"))?["default_branch"]
        .as_str()
        .unwrap_or("main")
        .to_string();
    let (owner, name) = (&global.kafes_owner, &repo.name);
    Manual::new("Open a dummy PR from the kafes fork into kafes-base/setup")
        .text("if one is already open from an earlier run, use it and skip to step 3")
        .step("Create a file on a new branch of the kafes fork:")
        .link(format!(
            "https://github.com/{kafes_fork}/new/{kafes_default}?filename=docs/kafes-setup-dummy.md"
        ))
        .text("content: anything; choose 'Create a new branch' (e.g. setup-dummy), propose")
        .step("Create the pull request:")
        .link(format!(
            "https://github.com/{fork}/compare/kafes-base/setup...{owner}:{name}:setup-dummy?expand=1"
        ))
        .step("Wait for a 'kafes-pr-check' status (failing is fine)")
        .step("Close the PR without merging, and delete setup-dummy on the kafes fork")
        .confirm()
}
