# up

Update all your developer tools with one command.

`up` runs your update commands together, shows the progress on a live board,
and then tells you exactly which packages changed.

```
  Updating your tools   00:24

  ✔  Claude skills     00:04  2 changed
  ⠹  npm globals       00:24  working
  ·  npm backup               waiting
  ·  Node (nvm)               waiting
  ✔  Homebrew index    00:06  Already up-to-date.
  ⠹  Homebrew apps     00:18  working
  ·  nono packs               waiting
  ·  Homebrew backup         waiting
  ·  Homebrew cleanup        waiting
  ⠹  macOS updates     00:24  Scanning for updates…
  —  App Store apps          mas is not installed
  ✔  zsh plugins       00:02  1 changed

  ███████████████░░░░░░░░░░░░░  3/12
```

At the end you get the change list:

```
  All updates are done.   total 01:24

  Claude skills 2 changed
    ↑ code-review
    ↑ tdd

  npm globals 2 changed
    ↑ githits   0.11.4 → 0.12.0
    + newtool   1.0.0  (new)

  Homebrew apps 3 changed
    ↑ git       2.43.0 → 2.44.0
    ↑ jq        1.7 → 1.7.1
    − oldpkg    (removed)
```

## What it updates

| Job | Command | Lane |
| --- | --- | --- |
| Claude skills | `npx --yes skills update -g -y` | node |
| npm globals | `npm update -g` | node |
| npm backup | `npm list --global --depth=0 --json` to a file | node |
| Node (nvm) | `nvm install --lts --latest-npm`, with `--reinstall-packages-from` for a new node | node |
| Homebrew index | `brew update` | brew |
| Homebrew apps | `brew upgrade` | brew |
| nono packs | `nono update` | brew |
| Homebrew backup | `brew bundle dump` to a Brewfile | brew |
| Homebrew cleanup | `brew cleanup --prune=all` | brew |
| macOS updates | `softwareupdate --install --all` | macos |
| App Store apps | `mas upgrade` | mas |
| zsh plugins | `git pull --ff-only` in each git clone in `~/.zsh/plugins` | zsh |

A job that needs a program that you do not have is not an error. The board
shows it as *missing* and the run continues. You therefore do not need `mas`
or `nvm` to use the script.

## Install

```sh
git clone git@github.com:iler/up.git ~/projects/up
mkdir -p ~/.local/bin
ln -s ~/projects/up/up ~/.local/bin/up
```

Then type `up`. If `~/.local/bin` is not in your `PATH`, use an alias instead:

```sh
alias up=~/projects/up/up
```

## Requirements

- bash 3.2 or later. This is the version that macOS supplies.
- `jq`, for the change list of the skills, the npm globals and the node
  versions. Without `jq` those lists stay empty. The updates still run.

The script has no other dependencies. This is on purpose: an updater must not
depend on the packages that it updates.

## Options

| Option | Effect |
| --- | --- |
| `-s`, `--sequential` | Run all jobs one after the other. |
| `-v`, `--verbose` | Show the raw output. This turns off the live board. |
| `-o`, `--only <name>` | Run only the jobs that match `<name>` or lane `<name>`. |
| `-n`, `--dry-run` | Show the jobs and the commands. Do not run them. |
| `-l`, `--list` | List the jobs and stop. |
| `--color <when>` | `always`, `never` or `auto`. The default is `auto`. |
| `-h`, `--help` | Show the help. |
| `--version` | Show the version. |

Examples:

```sh
up                 # run all updates
up --only brew     # run only the Homebrew lane
up -v --only npm   # update the npm globals and show the raw output
up --only macos    # install the macOS updates only
```

## Environment

| Variable | Default | Use |
| --- | --- | --- |
| `UP_LOG_DIR` | `~/.local/state/update-script` | Where the logs go. |
| `UP_BACKUP_DIR` | `~/.config` | Where the backup jobs write. |
| `NVM_DIR` | `~/.nvm` | Where nvm keeps the node versions. |
| `SKILL_LOCK` | `~/.agents/.skill-lock.json` | The lock file of the skills. |
| `ZSH_PLUGIN_DIR` | `~/.zsh/plugins` | Where the zsh plugins are cloned. |

## The macOS updates and sudo

Most macOS updates need root. The jobs get no keyboard input, so the script
cannot answer a password question. It therefore uses `sudo` only when `sudo`
needs no password:

```sh
sudo -v && up
```

Without a fresh `sudo` timestamp the job still runs, but it installs only the
updates that need no root.

The script never restarts the machine. An update that needs a restart stays
ready, and macOS asks you later.

## The backups

Two jobs write a list of what you have installed. Keep these files in version
control. They let you rebuild the machine.

| File | Content |
| --- | --- |
| `~/.config/brew/Brewfile` | The taps, the formulae, the casks and the App Store apps. |
| `~/.config/npm/packages.json` | The global npm packages. |

Restore with `brew bundle install --file=~/.config/brew/Brewfile`.

The backup jobs run after the update jobs of the same lane. The files
therefore hold the new state.

## How it works

### Lanes

Each job belongs to a lane. Lanes run at the same time. The jobs in one lane
run in sequence. The `node` lane and the `brew` lane are separate, because two
npm processes must not write to the global directory together.

`mas` has its own lane. A failed `softwareupdate` must not stop the App Store
updates.

The nono packs job runs in the `brew` lane, after `brew upgrade`. Homebrew
installs the `nono` program. A new pack can need the new program, and the
program must not change while the packs update. `nono update` skips pinned
packs.

If a job fails, the script skips the remaining jobs of that lane. `brew upgrade`
therefore never runs after a failed `brew update`. This is the same rule as
`brew update && brew upgrade`.

The node update runs last in the `node` lane, and not in a lane of its own.
`nvm install --latest-npm` can write npm into the node that you use now. That
must not happen while `npm update -g` runs. The last place also protects the
other jobs: a failed download of a new node version then loses nothing.

A new node version starts with no global packages. When the newest LTS is not
installed yet, the node job therefore copies the global packages of the node
that you use now into the new node. nvm fails if the two versions are the same,
so the job does not copy when the newest LTS is already installed.

### The change list

The script does not read the output of the update commands. Output text changes
between tool versions. Instead each job has a snapshot function. The function
prints one line for each installed item:

```
<name> <version>
```

The script runs the function before and after the job, and then compares the two
lists. The version source is:

| Job | Source |
| --- | --- |
| Claude skills | `skillFolderHash` in `~/.agents/.skill-lock.json` |
| npm globals | `npm ls -g --depth=0 --json` |
| Node (nvm) | The directories in `~/.nvm/versions/node`. The version column holds the npm version inside that node. |
| Homebrew apps | `brew list --versions` |
| nono packs | `nono list --installed --json` |
| App Store apps | `mas list` |
| zsh plugins | The commit of each git clone in `~/.zsh/plugins` |

This also works for `npm update -g`, which only reports *"changed 3 packages"*
and never says which ones.

Skills have no version number, only a content hash. Those lines therefore show
the name alone.

An App Store name can hold spaces. The name must be one field, so the snapshot
joins the words with an underscore: `Final_Cut_Pro`.

The backup jobs and the cleanup job install nothing. They have no snapshot
function. Their line on the board shows the last line of their output.

### The board

The board redraws in place. It reads the terminal size from `stty size`, because
`tput cols` cannot see the terminal inside a command substitution and reports 80.
The board also turns the line wrap off while it runs. A long line can then never
become two screen lines and break the redraw.

The layout follows the window height. The full board needs `jobs + 5` rows.
With fewer rows the script drops the blank lines. With fewer than `jobs + 3`
rows it shows one status line only. The twelve default jobs therefore give a
full board at 17 rows, a compact board at 15, and one line at 14.

Without a terminal — in `cron`, or through a pipe — the script prints plain
lines instead of the board.

## Add your own job

Add one line to `register_jobs`:

```sh
add_job "<name>" <lane> "<necessary program>" "<command>" [snapshot function]
```

For example:

```sh
add_job "Rust" rust "rustup" "rustup update" snap_rust
```

Use a new lane name if the job can run at the same time as the others. Give the
job a snapshot function if it installs packages. The snapshot function is
optional.

The *necessary program* is normally a name in the `PATH`. A value with a slash
is a file test instead. Use a path for a job that reads a shell file, like the
node job, which reads `~/.nvm/nvm.sh`.

The *command* can be a shell function of the script. Use one when the job needs
more than one line. `up_node_update` and `up_brew_backup` are examples.

## Logs

Each job writes its own log file to `~/.local/state/update-script/<timestamp>/`.
The script keeps the last 10 runs. Set `UP_LOG_DIR` to change the location.

If a job fails, the script prints the end of that log and exits with status 1.

## Notes

The jobs get no keyboard input. A job that asks for a password therefore fails
with a clear message. It does not stop and wait. This can happen with a Homebrew
cask that needs `sudo`.

`npx skills update` normally asks for the scope. The script uses `-g -y` to stop
the question and to always update the global skills. This also matches the lock
file that the change list reads. Remove the `-g` in `register_jobs` if you
prefer the project scope.

`brew cleanup --prune=all` removes the old versions and the whole download
cache. A later install therefore downloads again. Remove `--prune=all` in
`register_jobs` if you prefer to keep the cache.

## Licence

MIT. See [LICENSE](LICENSE).
