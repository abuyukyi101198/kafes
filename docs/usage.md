# Usage

## Before you start

- **Ubuntu host** with a desktop session (kafes runs its services as systemd
  user units), `sudo`, `git`, `curl`, `openssl`, `sha256sum` and
  `systemd-run`. kafes installs LXD, Workshop, Bun and OMP itself, after
  asking.
- **Rust** 1.88 or later (install with [rustup](https://rustup.rs/)) to build
  kafes.
- **For each repository:** your fork of the project, public, and a local
  checkout of it whose `origin` is that fork.
- **A browser signed in to GitHub** as you. kafes never asks for GitHub
  credentials and never pushes; every step that writes to GitHub is printed for
  you to do.

> [!WARNING]
> **Hosts running Docker:** Docker sets the `FORWARD` policy to `DROP`, which
> also blocks the LXD bridge, so a new workshop can't reach the Ubuntu archive.
> Allow the bridge (default `workshopbr0`) before the first task:
>
> ```bash
> B=workshopbr0
> sudo iptables -I DOCKER-USER -i "$B" -j ACCEPT
> sudo iptables -I DOCKER-USER -o "$B" -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT
> ```
>
> The `kafes-no-lan` ACL still blocks the host and private networks.

## Install

```bash
git clone https://github.com/abuyukyi101198/kafes.git
cd kafes
cargo build --release
./target/release/kafes init
```

`init` copies the binary to `~/kafes/sandbox/kafes` and links
`~/.local/bin/kafes` to it. From then on, run `kafes` from your home directory.
If `kafes` isn't found, `~/.local/bin` isn't on your `PATH` yet: log in again,
or use `~/kafes/sandbox/kafes`.

> [!IMPORTANT]
> To update, rebuild and run the **new** binary's `init`:
> `./target/release/kafes init`. Running the installed `kafes init` copies the
> old binary over itself. `init` also rewrites the embedded workshop setup
> script, agent policy and push proxy in `~/kafes/sandbox`. When only the code
> changed, copying the binary is enough:
> `install -m 755 target/release/kafes ~/kafes/sandbox/kafes`.

## One-time setup

```bash
kafes init
```

`init` walks through these stages. The ones marked _manual_ are yours; kafes
prints the URLs and values and then checks the result.

1. **Host tools.** Checks for a systemd user session and the host commands,
   then installs anything missing: the LXD snap (6/stable), the Workshop snap,
   Bun and the pinned OMP version.
2. **Model providers.** Logs the host's OMP broker in to the providers you
   pick: GitHub Copilot and OpenRouter. Don't approve Copilot's repository write
   access; for OpenRouter, use a dedicated key with a credit limit.
3. **kafes organization and GitHub App** _(manual)_. kafes asks for your
   GitHub username, then guides you through:
    - a **kafes organization** you own (default `<user>-kafes`), with no other
      members;
    - a **GitHub App** owned by it: Contents read and write, Metadata read-only,
      nothing else, no webhook, installable only on this account;
    - its **private key**. kafes offers the newest `*.private-key.pem` in
      `~/Downloads`, moves it to `~/.config/kafes/push/app.pem` and deletes the
      original;
    - the App's **Client ID** and **slug** (its name in the settings URL).
      kafes looks up the bot and records the identity the agent will commit as.
4. **Trusted files and push proxy.** Writes the workshop setup script, the
   agent policy and the proxy to `~/kafes/sandbox`, and installs the binary.
   Read `~/kafes/sandbox/kafes-push-proxy.ts` when asked.
5. **Network ACL.** Asks for `sudo`, lists the LXD bridges and attaches
   `kafes-no-lan` to the one workshops use (`workshopbr0` by default). The
   bridge's previous ACLs are recorded for teardown.
6. **First repository.** The same stages as `kafes repo add`, below.

## Repositories

kafes works on any number of repositories. The organization, App, broker,
gateway and proxy are shared; each repository has its own kafes fork, profile
and edit-rule check.

```bash
kafes repo add [<repo>] [--profile <preset>]
```

`<repo>` is your fork's name, without the owner. `repo add`:

1. **Checks your checkout.** Asks for its absolute path and checks that its
   `origin` is your fork.
2. **kafes fork** _(manual)_. Fork your fork (not upstream) into the kafes
   organization, keeping its name, then disable Actions on it.
3. **App access** _(manual)_. For the first repository, install the App on the
   organization with only the kafes fork selected, and enter the installation
   ID (the number at the end of the installation's URL). For later
   repositories, add the new kafes fork to that installation.
4. **Profile.** Shows the chosen [preset](#profiles) in full and saves a copy
   once you confirm. A repository that already has a profile keeps it unless
   you pass `--profile`.
5. **Edit-rule check** _(manual)_. kafes renders the check from the profile
   into `~/kafes/sandbox/repos/<repo>/kafes-pr-check.yml`. Read it when asked:
   it runs with a token that can write to your fork. Then:
    - push the `kafes-checks` branch kafes prepared, with the command it prints;
    - make `kafes-checks` your fork's default branch;
    - create the repository **variables** `KAFES_FORK` and `KAFES_EMAIL`, with
      the printed values, and enable Actions on your fork;
    - push the throwaway `kafes-base/setup` branch, open the dummy PR kafes
      describes, wait for a `kafes-pr-check` status (failing is fine), then
      close it without merging;
    - create the `kafes-base` ruleset exactly as printed: active, empty bypass
      list, `kafes-base/**`, restrict deletions, block force pushes, require
      `kafes-pr-check` from **GitHub Actions** (never _Any source_), and tick
      _Do not require status checks on creation_.

   If your fork's `kafes-checks` differs from the profile's check, kafes shows
   the diff and offers a force push that replaces it.

6. **Verify.** Starts the services, reads the kafes fork through the proxy and
   stops them again.

| Command                                     | What it does                                                                                                                              |
|---------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| `kafes repo list [--no-headers]`            | Lists the added repositories: forks, tasks and checkout                                                                                   |
| `kafes repo show <repo>`                    | Prints the repository's profile and the path of its rendered check                                                                        |
| `kafes repo edit <repo> [--profile <name>]` | Opens the profile in `$VISUAL` or `$EDITOR`, or replaces it with a preset, then brings `kafes-checks` in line                             |
| `kafes repo remove <repo>`                  | Prints the GitHub steps that undo kafes on that repository, and deletes its local configuration and profile; refuses while a task uses it |

### Profiles

A profile describes one repository: what the workshop installs, how the project
installs, and which paths the agent may change. kafes ships presets and copies
the chosen one to `~/kafes/sandbox/repos/<repo>/profile.toml`; updating kafes
never changes a saved copy. The only preset so far is `yarn`, for Vite, Vitest
and Cypress on Yarn 1:

```toml
preset = "yarn"
preset_version = 1
description = "Vite, Vitest and Cypress on Yarn 1"

[workshop]
node = "22/stable"                    # Workshop Node SDK channel
apt = ["build-essential", "python3"]  # apt-get install in the workshop
setup = ['npm install --global --prefix "$HOME/.local" yarn@1.22.22']

[project]
install = "~/.local/bin/yarn install --frozen-lockfile"  # run in the clone
env = { CYPRESS_INSTALL_BINARY = "0" }                   # for install only
test_hint = "For validation, run the smallest relevant Vitest selection ..."

[rules]  # JavaScript regular expressions over repository paths
allowed = ['^src/.+\.(ts|tsx|scss)$', '^cypress/e2e/.+\.(ts|feature)$', ...]
rejected = ['^src/app/apiclient/']
review_closely = ['^src/(setupTests\.ts|testing/)', '(^|/)__mocks__/', ...]
```

- `workshop` and `project` take effect from the next `task create`.
- `rules` and `test_hint` go into the agent's prompt from the next `task shell`
  or `task start`, and into the check once your fork's `kafes-checks` is
  updated. `repo edit` offers that update.
- kafes refuses unknown keys, invalid package or variable names, and an empty
  `allowed` list. Dotfiles, binaries, symlinks, executables, submodules and
  unusual path characters are always rejected; a profile can't allow them.

## Per task

```bash
kafes task create <repo>/<feature-branch> [--name <task>]
```

The feature branch must be on your fork. The task name defaults to the branch
name with `/` replaced by `-`, such as `feat-login` for `feat/login`. Task names
are unique across repositories until the task is deleted. kafes:

1. Checks the ruleset and asks for `sudo` once, to read the bridge's ACL and
   subnet. If the prompt seems to hang, your `sudo` may be waiting for a
   security key or approval.
2. Asks you to push `kafes-base/<task>`, at the feature branch's head, with the
   command it prints (or offers to reuse an existing one).
3. Starts the broker, gateway and push proxy, and copies your allowlisted host
   OMP settings (model roles, fallbacks, display). It warns about any model
   provider the gateway can't serve.
4. Launches the workshop, disconnects the package caches, connects the two
   tunnels, and checks that nothing else is connected and that the workshop is
   on the ACL bridge.
5. Installs the base tools, then the profile's apt packages and setup commands,
   and writes the agent's launcher, policy, prompt and settings.
6. Clones your fork at `kafes-base/<task>` onto a `kafes/<task>` branch and runs
   the profile's install command.

It then offers to open the workshop terminal. It doesn't start OMP.

| Command                          | What it does                                                                                                             |
|----------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| `kafes task list [--no-headers]` | Lists tasks with their repository and status: `running`, `stopped`, or `broken`/`leftover` when one needs `task delete`  |
| `kafes task shell <task>`        | Restores the agent's launcher, policy, prompt and model settings from the host, then opens the workshop terminal         |
| `kafes task stop <task>`         | Revokes the task's credentials, stops the services and the workshop. Files, OMP sessions and installed packages are kept |
| `kafes task start <task>`        | Restores the trusted workshop definition, starts the workshop, re-checks its connections and issues new credentials      |
| `kafes task delete <task>`       | Revokes the credentials, stops the services, removes the workshop and prints the review links. Also the emergency stop   |

In the workshop terminal, run `kafes-omp` to start the agent, or `kafes-omp -c`
to continue its last session. `/exit` returns you to the workshop terminal and
`exit` to the host. The workshop keeps running until you stop or delete the
task.

> [!WARNING]
> Start the agent only with `kafes-omp`. Plain `omp` runs with OMP's defaults,
> which approve every tool call. To restart the agent, `exit` the terminal and
> run `kafes task shell <task>` again, so its launcher and policy are restored
> first.

> [!CAUTION]
> Never run `workshop refresh` or `workshop restore` on a task workshop, and
> never `cd` into `~/kafes/tasks` or open it in an IDE: the agent can write
> there. kafes refuses to run from inside it.

Only one task is active at a time. If `task create` or `task start` stops
part-way or you press Ctrl+C, kafes prints the command that cleans up.

## Review and merge

`kafes task delete` prints, when the agent has pushed `kafes/<task>`:

- a link that opens the PR from the kafes fork into `kafes-base/<task>`;
- the branch's head commit.

In the browser:

1. Open the PR as yourself.
2. Wait for `kafes-pr-check` to pass on the current head.
3. Review the diff. Read every file under "Review closely" line by line, and
   treat test files as code that will run.
4. Merge only if the PR's head is still the commit kafes printed.

> [!CAUTION]
> Never fetch, clone or check out the agent's branch on the host before it is
> merged.

Then merge `kafes-base/<task>` into your feature branch on GitHub (kafes prints
that link too), pull it into your checkout, and test as usual. Agent PRs can
change only the paths the profile allows, so tooling and dependency files stay
human-authored; test files and test setup do run.

## Teardown

```bash
kafes teardown
```

Removes the whole setup, not one task. kafes deletes every task and removes
leftover task workshops, then, for each repository, prints the same GitHub
steps as `repo remove`:

- close leftover kafes PRs, delete the ruleset and the `KAFES_*` variables,
  and restore your fork's default branch (kafes remembers the old one);
- delete the kafes branches with the command it prints;
- remove the kafes fork from the App's installation, then delete it.

Then it prints the remaining GitHub steps, restores the bridge's ACLs, deletes
its local files and the `~/.local/bin/kafes` link, and offers to log the broker
out and uninstall OMP:

- delete the GitHub App **before** the kafes organization that owns it;
- revoke the OpenRouter key and the Copilot authorization used for the logins;
- last, delete the organization.

Workshop and LXD stay installed; remove them only if nothing else uses them.
