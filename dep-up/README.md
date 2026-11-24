# dep-up

Check and update all your developer tools from one interactive list.

dep-up is the Rust version of [`up`](../update-script). It updates the same tools, and also
the Rust toolchains, the cargo binaries, the uv tools and the gh extensions. It works like
[depsu](https://github.com/joonakeskitalo/depsu): it first checks what is outdated, then you
select what to update. The list stays open after an update, so you can update one thing after
another.

```
 dep-up   17 jobs · 6 outdated · 1 running   group: none
╭ jobs ─────────────────────────────────────────────────────────────────────╮
│ ○ ✔ Claude skills    node   00:04  2 changed                              │
│ ● ↑ npm globals      node          2 outdated                             │
│     ● githits        0.11.4 → 0.12.0                                      │
│     ● pnpm           9.0.0 → 9.1.0                                        │
│ ○ ✔ Homebrew index   brew   00:06  Already up-to-date.                    │
│ ○ ⠹ Homebrew apps    brew   00:18  ==> Upgrading jq                       │
│ ○ · Homebrew backup  brew          waiting for its lane                   │
│ ○ ↑ macOS updates    macos         1 outdated                             │
│     ○ macOS Sequoia 15.1           15.1  restart                          │
│ ○ — App Store apps   mas           mas is not installed                   │
│ ○ ✔ Rust toolchains  rust          up to date                             │
╰───────────────────────────────────────────────────────────────────────────╯
╭ log ──────────────────────────────────────────────────────────────────────╮
│ ✔ Claude skills  00:04  2 changed                                         │
╰───────────────────────────────────────────────────────────────────────────╯
 space select  a all  i run  / filter  g group  ? details  l log  c commit
```

When you quit, dep-up prints what changed in the session:

```
  All updates are done.   total 01:24

  npm globals 2 changed
    ↑ githits   0.11.4 → 0.12.0
    + newtool   1.0.0  (new)

  Homebrew apps 2 changed
    ↑ git       2.43.0 → 2.44.0
    − oldpkg    (removed)
```

## Features

- **Check first.** At the start, each job checks what is outdated. Nothing changes during the
  check (only `brew update` refreshes the Homebrew index, see below).
- **Select and run.** Run a whole job, or only the items that you select, for example only
  `git` from the outdated Homebrew formulae.
- **Stays open.** Press `i`, continue to browse, press `i` again. The jobs of one lane wait for
  each other. Different lanes run at the same time.
- **Change list.** dep-up compares the installed versions before and after each job, and shows
  each package that changed.
- **Backups.** After an update, dep-up writes the Brewfile and the npm package list again.
- **Commit.** `c` commits the Brewfile and the npm package list with a conventional commit
  title, for example `chore: upgrade githits 0.11.4 to 0.12.0`.
- **Details, log, filter, groups, cache.** `?` shows the details of a job and the end of its
  log. `/` filters. `g` groups by lane or by state. `--cache` keeps the check results.
- **No questions in cron.** `--all` runs every job with no TUI and prints plain lines.

## What it updates

| Job | Check | Update | Lane |
| --- | --- | --- | --- |
| Claude skills | — | `npx --yes skills update -g -y` | node |
| npm globals | `npm outdated -g --json` | `npm update -g`, or `npm install -g <name>@latest` | node |
| UI/UX Pro Max | — | `uipro update --global --ai <type>` for each type in `UIPRO_AI` | node |
| npm backup | — | `npm list --global --depth=0 --json` to a file | node |
| Node (nvm) | `nvm version-remote --lts` | `nvm install --lts --latest-npm`, then `npm install -g` for missing global packages | node |
| Homebrew index | — | `brew update` | brew |
| Homebrew apps | `brew outdated --json=v2` | `brew upgrade`, or `brew upgrade --formula/--cask <names>` | brew |
| nono packs | `nono outdated --json` | `nono update`, or `nono update <pack>` | brew |
| Homebrew backup | — | `brew bundle dump` to a Brewfile | brew |
| Homebrew cleanup | — | `brew cleanup --prune=all` | brew |
| macOS updates | `softwareupdate --list` | `softwareupdate --install --all`, or `--install <labels>` | macos |
| App Store apps | `mas outdated` | `mas upgrade`, or `mas upgrade <ids>` | mas |
| zsh plugins | `git fetch`, then the commits behind the upstream | `git pull --ff-only` in each git clone in `~/.zsh/plugins` | zsh |
| Rust toolchains | `rustup check` | `rustup update`, or `rustup update --no-self-update <toolchains>` | rust |
| Cargo binaries | `~/.cargo/.crates2.json` and the crates.io index | `cargo install <name>` for each outdated crate | rust |
| uv tools | `uv tool list --outdated` | `uv tool upgrade --all`, or `uv tool upgrade <names>` | uv |
| gh extensions | `gh extension upgrade --all --dry-run` | `gh extension upgrade --all`, or `gh extension upgrade <name>` | gh |

A job that needs a program that you do not have is not an error. The list shows it as *not
installed*, and dep-up does not run it.

Two jobs have no check, because their tools have no machine-readable check: Claude skills and
UI/UX Pro Max. The list shows them as *no check*. When you run them, the change list shows
what changed.

## Install

You need a Rust toolchain.

```sh
cd dep-up
make install          # builds and copies dep-up to ~/.local/bin (PREFIX=/usr/local to change)
```

or `cargo install --path .`.

dep-up has no other run-time dependencies. It does not need `jq`: it reads the JSON itself.
This is on purpose: an updater must not depend on the packages that it updates.

## Usage

```sh
dep-up                   # open the list and check all jobs
dep-up --only brew       # only the Homebrew lane
dep-up --cache ~/.cache/dep-up.json   # keep the check results between runs
dep-up --all             # run every job with no questions (for cron)
dep-up -v --only npm     # update the npm globals and show the raw output
```

| Option | Effect |
| --- | --- |
| `-a`, `--all` | Run every job with no questions and no TUI. Use this in cron or through a pipe. |
| `-s`, `--sequential` | Run all jobs one after the other, not lane by lane. |
| `-v`, `--verbose` | Show the raw output of each job. This implies `--all` and `--sequential`. |
| `-o`, `--only <name>` | Use only the jobs that match `<name>`, its lane or its program. |
| `-n`, `--dry-run` | Show the jobs, the commands and the checks. Do not run them. |
| `-l`, `--list` | List the jobs and stop. |
| `--cache <file>` | Keep the check results in `<file>`. See [The cache](#the-cache). |
| `--color <when>` | `always`, `never` or `auto`, for the text output. The default is `auto`. |
| `-h`, `--help` / `-V`, `--version` | |

Without `--all`, dep-up needs a terminal. Through a pipe it stops with a message. It does not
start the updates by itself.

### Keys

| Key | Action |
| --- | --- |
| `↑` `↓` / `j` `k`, `Home` `End` | Move. |
| `,` `.` (or `PgUp` `PgDn`) | Page up / page down. |
| `space` | Select the job, the item or the group under the cursor. To select an item of a selected job removes the job and keeps its other items. |
| `a` | Select all shown jobs that have work, or select none. A job has work when it has outdated items, or when it has no check. Backups and the Homebrew index are not selected: they run by themselves. |
| `i` / `Enter` | Run the selected jobs and items. Without a selection, run the row under the cursor: the job, the item, or the jobs of the group. A selected job runs its full update command (for example `brew upgrade`). Selected items run only those items. After the run, dep-up checks the job again. |
| `/` | Filter by name. The filter also finds items, for example `jq`. `Enter` keeps the filter, `Esc` clears it. |
| `g` | Group by: none → lane → state. |
| `?` | Show / hide the details panel: the lane, the program, the commands, the last check, the last run, the changes of the session and the end of the log. |
| `Shift+↑` / `Shift+↓` | Scroll the details panel. |
| `l` | Open / close the log of the session. |
| `c` | Commit the Brewfile and the npm package list. See [The commit](#the-commit). |
| `R` | Check all jobs again. This ignores the cache. |
| `y` | Copy the text that you selected with the mouse. |
| `q` / `Esc` | Quit. While a job runs, press `q` two times: the second press stops the jobs. |

`a` and `i` use only the rows that you can see, so a filter limits what they do.

### Mouse

- Click a row to move the cursor to it. Click the mark (`○`, `●`) to select it.
- Click a key hint in the footer to press that key. In a dialog, click its hints.
- Click the log to open it.
- The wheel moves the cursor in the list, and scrolls the details panel and the log.
- Drag over text in the details panel or the log to select it. Then press `y` (or click
  `y copy the selection`) to copy it with `pbcopy`, `wl-copy`, `xclip`, `xsel` or `clip.exe`.
  Hold Shift (Option in some terminals) to select text with the terminal instead.

## How it works

### Lanes

Each job belongs to a lane. Lanes run at the same time. The tasks in one lane run in sequence.
The `node` lane and the `brew` lane are separate, because two npm processes must not write to
the global directory together.

If a job fails, dep-up skips the other runs that wait in that lane. `brew upgrade` therefore
never runs after a failed `brew update`. This is the same rule as `brew update && brew upgrade`.

The rules of `up` stay the same:

- `nono packs` runs in the brew lane after `brew upgrade`. Homebrew installs the `nono`
  program, and the program must not change while the packs update.
- `UI/UX Pro Max` runs after `npm update -g`, because `uipro` is a global npm package. When the
  CLI changes, the job runs the update a second time with the new CLI.
- `Node (nvm)` runs last in the node lane. `nvm install --latest-npm` can write npm into the
  node that you use now. That must not happen while `npm update -g` runs.
- `App Store apps` has its own lane. A failed `softwareupdate` must not stop it.

The new jobs follow the same idea:

- `Rust toolchains` and `Cargo binaries` share the `rust` lane. `cargo install` builds with
  the toolchain, so it must not run while rustup replaces that toolchain.
- `uv tools` and `gh extensions` each have a lane of their own.

### The check

At the start, dep-up puts a check task in each lane. The checks change nothing, with one
exception: `brew outdated` needs a fresh index. The Homebrew index job (`brew update`)
therefore runs first in the brew lane, and the list shows its result. `brew update` changes
only the index, not the installed formulae.

The Cargo binaries check reads `~/.cargo/.crates2.json` and asks the crates.io sparse index
for the newest version of each crate. It skips yanked versions, and it skips prereleases
unless the installed version is a prerelease. Crates from git or from a path, and crates that
you installed with `--version`, are not checked. The update keeps the features of the first
install (`--features`, `--all-features`, `--no-default-features`).

### The backups

Two jobs write a list of what you have installed:

| File | Content |
| --- | --- |
| `~/.config/brew/Brewfile` | The taps, the formulae, the casks and the App Store apps. |
| `~/.config/npm/packages.json` | The global npm packages. |

When you run an update, dep-up adds the backup job of the same lane to the end of the run. The
files therefore hold the new state. You can also run a backup job yourself.

Restore with `brew bundle install --file=~/.config/brew/Brewfile`.

### The change list

dep-up does not read the output of the update commands, because that text changes between tool
versions. Each job instead has a snapshot that lists the installed items with their versions.
dep-up takes a snapshot before and after each run and compares the two.

| Job | Source of the versions |
| --- | --- |
| Claude skills | `skillFolderHash` in `~/.agents/.skill-lock.json` |
| npm globals | `npm ls -g --depth=0 --json` |
| UI/UX Pro Max | `uipro --version` |
| Node (nvm) | The directories in `~/.nvm/versions/node`, with the npm version inside each |
| Homebrew apps | `brew list --versions --formula` and `--cask` |
| nono packs | `nono list --installed --json` |
| App Store apps | `mas list` |
| zsh plugins | The commit of each git clone |
| Rust toolchains | `rustup toolchain list`, `rustc --version` of each toolchain, and `rustup --version` |
| Cargo binaries | `~/.cargo/.crates2.json` |
| uv tools | `uv tool list` |
| gh extensions | `gh extension list` |

Skills and zsh plugins have a hash, not a version. Their lines show the name only.

The jobs without a snapshot show a short note instead: the last line of the output, or for the
cleanup the freed disk space (`599.7MB freed`).

The list and the summary show the changes of the whole session. When a job runs two times, the
change goes from the first version to the last one.

### macOS updates and sudo

Most macOS updates need root. The jobs get no keyboard input, so a job cannot answer a password
question. When you run the macOS job and sudo needs a password, dep-up asks first:

- `s`: dep-up leaves the screen and runs `sudo -v`. You type the password in the normal
  terminal. Then the list comes back and the job starts.
- `n`: the job installs only the updates that need no root.

While the job runs, dep-up keeps the sudo timestamp fresh. dep-up never restarts the machine.
An update that needs a restart stays ready, and macOS asks you later.

With `--all`, there is no question: run `sudo -v && dep-up --all` to install all updates.

### No keyboard input

Each command runs in a new session without a terminal, and its input is empty. A tool that
asks for a password therefore fails at once, with a clear message. It does not stop and wait.
This can happen with a Homebrew cask that needs `sudo`. git gets `GIT_TERMINAL_PROMPT=0`.

### Dotfiles

If you have [chezmoi](https://chezmoi.io) and a source directory, dep-up runs `chezmoi re-add`
when all tasks are done. This copies the changed dotfiles, the Brewfile and the npm package
list into your dotfiles repository. dep-up never commits by itself. chezmoi never overwrites a
template, so a secret from a password manager cannot travel back into the repository.

Without chezmoi, this step does nothing.

### The commit

`c` makes a commit of the two package lists:

- With chezmoi, dep-up commits the copies in the chezmoi source directory.
- Without chezmoi, dep-up commits the files in `~/.config`, if they are in a git repository.

The commit holds only those two files. Other staged or changed files stay as they are. Git hooks
run, and git signs the commit if your git settings say so.

The title is a conventional commit of what changed since the last commit:

| Changes | Title |
| --- | --- |
| One npm upgrade | `chore: upgrade githits 0.11.4 to 0.12.0` |
| A new formula in the Brewfile | `chore: add jq` |
| Some changes | `chore: upgrade githits 0.11.4 to 0.12.0 and add cask iterm2` |
| More changes | `chore: upgrade githits and pnpm, add jq` |
| Too many for 72 characters | `chore: upgrade 12 packages, add 3 packages` |

The body lists every change, grouped by file. When the title names each change with its
versions, there is no body. You can edit the title and the body in the dialog: `Tab` changes
between them, `Enter` in the title commits, `Enter` in the body starts a new line, `Ctrl+S`
commits from both. Until you edit the body, it follows the title.

The Brewfile has no versions. A `brew upgrade` therefore does not change it. Only a new or a
removed formula, cask, tap or app does.

### The cache

`--cache <file>` keeps the last check result of each job. At the start, the jobs with a result
in the file are not checked again, so the list opens fast. The header and the rows show the age
of the cached results. `R` checks all jobs again, and each new result goes into the file. After
a run, dep-up checks the job again and writes the new result.

One file can serve several runs. A file that dep-up cannot read stays as it is and is not used.

### Logs

Each session writes a log file for each job to `~/.local/state/dep-up/<timestamp>/`. dep-up
keeps the last 10 sessions. The log holds each command, its output, and the result of each
check.

If a job fails, the summary at the end shows the end of its log, and dep-up exits with
status 1.

## Environment

| Variable | Default | Use |
| --- | --- | --- |
| `DEP_UP_LOG_DIR` | `~/.local/state/dep-up` | Where the logs go. |
| `DEP_UP_BACKUP_DIR` | `~/.config` | Where the backup jobs write. |
| `NVM_DIR` | `~/.nvm` | Where nvm keeps the node versions. |
| `SKILL_LOCK` | `~/.agents/.skill-lock.json` | The lock file of the skills. |
| `ZSH_PLUGIN_DIR` | `~/.zsh/plugins` | Where the zsh plugins are cloned. |
| `UIPRO_AI` | `claude universal` | The assistant types for UI/UX Pro Max. |
| `CARGO_HOME` | `~/.cargo` | Where cargo keeps `.crates2.json`. |

The backup files are the same as the files of `up`. You can use both tools.

## Add your own job

Add a module in `src/jobs/` and add the job to `registry` in `src/jobs/mod.rs`:

```rust
let mut j = Job::base("my-id", "My tool", "my-lane", Need::Bin("mytool"), update);
j.command = "mytool upgrade".into();   // for --list and the details panel
j.check_text = "mytool outdated";      // with a check only
j.check = Some(check);                 // fn(&Ctx, &Sh) -> Result<Vec<Item>, String>
j.per_item = true;                     // the update can do single items
j.snapshot = Some(snapshot);           // fn(&Ctx) -> Snapshot (name → version)
```

The update function gets `None` for the whole job, or `Some(items)` for the selected items.
Use `sh.run(cmd(...))` to run a command: its output goes to the log and to the list. Use a new
lane if the job can run at the same time as the others.

## Development

```sh
make check      # rustfmt check, clippy (warnings are errors) and the tests
make test
make live       # run the real checks of this machine; they change nothing
make run ARGS="--only brew"
make help
```

| File | |
| --- | --- |
| `main.rs`, `cli.rs` | Start, options |
| `engine.rs` | Job states, lane queues, worker threads |
| `jobs/` | One module for each tool: check, update, snapshot, parsers |
| `run/shell.rs`, `run/task.rs` | Run commands without a terminal; one check or run on its thread |
| `run/snapshot.rs`, `run/tail.rs`, `run/logs.rs` | The change list, clean output lines, log directories |
| `tui/app.rs`, `tui/view.rs`, `tui/ui.rs`, `tui/text.rs` | TUI state and keys, the list model, drawing, text input |
| `dotfiles/` | chezmoi sync and the commit of the package lists |
| `cache.rs`, `plain.rs`, `summary.rs` | `--cache`, `--all`, the report at the end |

`tests/fixtures/` holds sample output of the tools for the parser tests.

## Differences from `up`

- dep-up checks first and asks what to update. `up` updates everything at once. `dep-up --all`
  works like `up`.
- New jobs: Rust toolchains, Cargo binaries, uv tools, gh extensions.
- No `jq`: dep-up reads the JSON itself.
- The backup jobs run by themselves after the updates of their lane.
- The macOS job can ask for the sudo password before it starts.
- The logs go to `~/.local/state/dep-up`, not `~/.local/state/update-script`.

## Limitations

- The Homebrew index updates at each start (with no cache), because `brew outdated` needs it.
- Claude skills and UI/UX Pro Max have no check.
- `--all` prints plain lines. It has no live board.
- Only tested on macOS.

## Licence

MIT. See [LICENSE](LICENSE).
