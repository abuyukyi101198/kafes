use crate::config::{GlobalConfig, RepoConfig, save_repo_config};
use crate::constants::{ACTIONS_APP_ID, SANDBOX_PATH};
use crate::git::{anon_fetch, git_stdin, manual_push, remote_sha};
use crate::github::gh_api;
use crate::ui::{
    print, print_info, print_warning, prompt_confirm, prompt_instruction, prompt_text,
};
use crate::utils::{home_path, run_cmd};
use anyhow::{Context, Result, bail};
use std::path::Path;

pub fn setup_check(global: &GlobalConfig, repo: &mut RepoConfig) -> Result<()> {
    let fork = repo.fork(global);
    let dir = repo.checkout.clone();
    print(&format!("Edit-rule check on {fork}"));

    let src = home_path(SANDBOX_PATH)?.join("kafes-pr-check.yml");
    if !src.is_file() {
        bail!("{} missing; run kafes setup first", src.display());
    }
    let want = checks_commit(&dir, &src)?;

    let remote = match remote_sha(&fork, "kafes-checks")? {
        Some(sha) => sha,
        None => {
            if !prompt_confirm(&format!(
                "Have you read {}? It runs with a token that can write to your fork.",
                src.display()
            ))? {
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
                    print_info("created local branch kafes-checks (nothing checked out)");
                }
                Some(sha) if sha == want => print_info("kafes-checks already exists locally"),
                Some(sha) => {
                    bail!("a local kafes-checks at {sha} has other content; inspect or rename it")
                }
            }
            manual_push(
                repo,
                &fork,
                &want,
                "kafes-checks",
                &format!("Publish the edit-rule check as {fork}'s kafes-checks branch."),
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
    if run_cmd("git", &["-C", &d, "diff", "--quiet", &remote, &want, "--"]).is_err() {
        print_warning(&format!(
            "{fork}'s kafes-checks differs from {}: review and update it",
            src.display()
        ));
    }
    print_info(&format!("kafes-checks is on {fork} at {}", &remote[..12]));

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
        let url = format!(
            "  https://github.com/{fork}/settings  -> Default branch -> switch to kafes-checks"
        );
        prompt_instruction(&[
            &format!("Make kafes-checks the default branch of {fork} (currently {current}):"),
            &url,
        ])?;
        if default_branch(&fork)? != "kafes-checks" {
            bail!("the default branch is still not kafes-checks");
        }
    }

    if rules_ok(&fork) {
        print_info("ruleset on kafes-base/** verified");
        return Ok(());
    }

    let kafes_fork = repo.kafes_fork(global);
    prompt_instruction(&[
        &format!("Create two repository VARIABLES (not secrets) on {fork}, or check they're set:"),
        &format!("  https://github.com/{fork}/settings/variables/actions"),
        &format!("  KAFES_FORK  = {kafes_fork}"),
        &format!("  KAFES_EMAIL = {}", global.app.bot_email),
        &format!("Allow Actions on {fork} (forks start with workflows disabled):"),
        &format!(
            "  https://github.com/{fork}/actions  -> 'I understand my workflows, go ahead and enable them'"
        ),
    ])?;

    bootstrap_status(global, repo)?;

    prompt_instruction(&[
        &format!("Create the ruleset: https://github.com/{fork}/settings/rules/new?target=branch"),
        "  (if a kafes-base ruleset already exists, edit it instead)",
        "  Name: kafes-base. Enforcement: Active. Bypass list: EMPTY.",
        "  Target branches -> Add target -> Include by pattern: kafes-base/**",
        "  Rules: Restrict deletions; Block force pushes;",
        "    Require status checks to pass -> Add checks -> kafes-pr-check,",
        "    and set its source to 'GitHub Actions' (never 'Any source').",
        "    Also tick 'Do not require status checks on creation'.",
        "  If the check or source is missing, refresh the editor and add it again.",
    ])?;

    if !rules_ok(&fork) {
        bail!("the ruleset on kafes-base/** is not as required; fix it and re-run");
    }

    print_info("ruleset on kafes-base/** verified");
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
