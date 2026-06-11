# clServer

`clServer` is a small Rust CLI for managing CatLord Minecraft, Velocity, and future Hytale servers. It starts servers inside `screen` sessions, writes logs, and can stop Minecraft servers through RCON.

## Features

- Start configured servers in detached `screen` sessions
- Stop Minecraft servers through RCON
- Stop Velocity/Hytale-style servers by sending a configured stop command to `screen`
- Restart, attach, status, backup, and daily maintenance commands exposed through the CLI
- Per-server screen log files
- Global command log file
- Configurable Java installations per server

## Requirements

- Rust/Cargo, for building from source
- `screen`, for process/session management
- `bash`, used when launching generated start commands
- Java runtime(s) for the servers you want to run
- Minecraft RCON enabled for Minecraft stop/restart support
- `rsync`, for local mirror backups and local restores
- `restic`, for remote historical backups

## Building

From the project root:

```sh
cargo build --release
```

The compiled binary will be available at:

```text
target/release/clserver
```

For local development, you can run commands directly with Cargo:

```sh
cargo run -- --help
```

## Installation and deployment

### Install server dependencies

On the server that will run `clServer`, install the required runtime tools:

```sh
sudo apt update
sudo apt install screen bash rsync restic
```

Install the Java runtime versions required by your configured servers. For example:

```sh
sudo apt install openjdk-21-jre-headless
```

If you want to build from source on the server, install Rust with `rustup` and make sure build tools are available:

```sh
sudo apt install build-essential pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

### Build the release binary

Build on the server or on another compatible Ubuntu machine:

```sh
cargo build --release
```

The release binary is created at:

```text
target/release/clserver
```

### Install the binary

A common install location for locally managed CLI tools is:

```text
/usr/local/bin/clserver
```

Install it with:

```sh
sudo install -m 755 target/release/clserver /usr/local/bin/clserver
```

Confirm it is available:

```sh
clserver --help
```

Optional: install shell completions. For system-wide bash completions:

```sh
clserver completions bash | sudo tee /etc/bash_completion.d/clserver >/dev/null
```

For per-user bash completions:

```sh
mkdir -p ~/.local/share/bash-completion/completions
clserver completions bash > ~/.local/share/bash-completion/completions/clserver
```

Open a new shell after installing completions. Regenerate the completion file after updating `clserver` if commands or flags changed.

### Deploy configuration

Create the config directory for the user that will run `clServer`:

```sh
mkdir -p ~/.config/clserver
```

Create or copy the config file to:

```text
~/.config/clserver/clserver.toml
```

If `XDG_CONFIG_HOME` is set, the config path is instead:

```text
$XDG_CONFIG_HOME/clserver/clserver.toml
```

If you previously used an older `clServer` build, move your config from the legacy path:

```sh
mkdir -p ~/.config/clserver
mv ~/.config/cls/cls.toml ~/.config/clserver/clserver.toml
```

Make sure the paths in `clserver.toml` match the server filesystem, especially:

- `global.serverDir`
- `global.logDir`
- Java executable paths in `[java_environments]`
- each server's `jarFile`, `startCommand`, `stopCommand`, and RCON settings

### Runtime log locations

Application logs are written to:

```text
$XDG_STATE_HOME/clserver/clserver.log
```

If `XDG_STATE_HOME` is not set, this is usually:

```text
~/.local/state/clserver/clserver.log
```

Per-server `screen` logs are written under:

```text
<global.logDir>/servers/<server name>/<timestamp>.log
```

### Smoke test a deployment

After installing the binary and config, run:

```sh
clserver status <server-name>
```

Then check the application log:

```sh
tail -n 50 ~/.local/state/clserver/clserver.log
```

If the server should be running, check that `screen` can see the session:

```sh
screen -ls
```

### Updating an existing install

Build the new release binary:

```sh
cargo build --release
```

Replace the installed binary:

```sh
sudo install -m 755 target/release/clserver /usr/local/bin/clserver
```

Confirm the installed version still starts:

```sh
clserver --help
```

## Configuration

`clServer` reads its configuration from:

```text
$XDG_CONFIG_HOME/clserver/clserver.toml
```

On most Linux systems, if `XDG_CONFIG_HOME` is not set, this is usually:

```text
~/.config/clserver/clserver.toml
```

Older versions used `~/.config/cls/cls.toml`. If you are upgrading, move that file to the new path.

Create the config directory if it does not already exist:

```sh
mkdir -p ~/.config/clserver
```

Then create:

```text
~/.config/clserver/clserver.toml
```

### Example configuration

```toml
[global]
serverDir = "/srv/servers"
logDir = "/var/log/clserver"

[backup]
localDir = "/srv/backups/clserver"
resticEnvFile = "/home/blackshadow/.config/clserver/secrets/restic.env"

[java_environments]
default = "/usr/bin/java"
java17 = "/usr/lib/jvm/java-17-openjdk/bin/java"
java21 = "/usr/lib/jvm/java-21-openjdk/bin/java"

[servers.CLS4]
name = "CatLordSurvival"
type = "minecraft"
javaVersion = "java21"
javaParams = "-Xms4G -Xmx4G"
jarFile = "server.jar"
rconPort = 25575
rconPassword = "change-me"
enabled = true
backup = true
restore = "world"

[servers.proxy]
name = "velocity"
type = "velocity"
javaVersion = "java17"
javaParams = "-Xms1G -Xmx1G"
jarFile = "velocity.jar"
stopCommand = "end"
enabled = true
backup = false
```

## Configuration reference

### `[global]`

| Field | Required | Description |
| --- | --- | --- |
| `serverDir` | Yes | Base directory containing all server directories. Each server is expected at `<serverDir>/<server name>`. |
| `logDir` | Yes | Base directory for per-server screen logs. |

### `[backup]`

| Field | Required | Description |
| --- | --- | --- |
| `localDir` | Required when any server has `backup = true` | Base directory for local mirror backups. Each server is backed up to `<localDir>/<server name>`. |
| `resticEnvFile` | No | Path to a shell-style env file loaded for `restic` commands. If omitted, `restic` inherits the environment from the `clserver` process. |

Example:

```toml
[backup]
localDir = "/srv/backups/clserver"
resticEnvFile = "/home/blackshadow/.config/clserver/secrets/restic.env"
```

### `[java_environments]`

Maps Java environment names to Java executable paths.

Example:

```toml
[java_environments]
default = "/usr/bin/java"
java21 = "/usr/lib/jvm/java-21-openjdk/bin/java"
```

A server can reference one of these entries with `javaVersion`.

If a server does not specify `javaBin` or `javaVersion`, `clServer` uses the `default` Java environment.

### `[servers.<id>]`

Each server is configured under a table such as:

```toml
[servers.CLS4]
name = "CatLordSurvival"
```

The table key, `CLS4` in this example, is the server ID/shortcut used in CLI commands:

```sh
clserver status CLS4
```

The `name` field is the real server directory and `screen` session name. Server IDs must be unique; TOML enforces this because duplicate table keys are invalid. Server `name` values must also be unique because they map to directories, screen sessions, and log paths.

| Field | Required | Description |
| --- | --- | --- |
| `name` | Yes | Real server directory and `screen` session name. This can differ from the server ID. |
| `type` | Yes | Server type. Supported values: `minecraft`, `velocity`, `hytale`. |
| `javaBin` | No | Direct path to a Java executable. Overrides `javaVersion`. |
| `javaVersion` | No | Key from `[java_environments]`. Defaults to `default` if `javaBin` is not set. |
| `javaParams` | No | Additional JVM parameters, such as memory settings. |
| `jarFile` | Usually | Jar file to run when `startCommand` is not provided. |
| `startCommand` | No | Full custom command used to start the server. If set, this overrides generated Java command behavior. |
| `stopCommand` | Required for Velocity/Hytale stop/restart | Command sent to the server's `screen` session when stopping. |
| `rconPort` | Required for Minecraft | RCON port for Minecraft servers. |
| `rconPassword` | Required for Minecraft | RCON password for Minecraft servers. |
| `enabled` | No | Whether whole-fleet maintenance should start this server. Missing values are treated as `false` for maintenance. |
| `backup` | No | Whether `backup --all` and `maintenance` should back up this server. Requires `backup.localDir` when true. |
| `restore` | No | Restore scope for `clserver restore <id>`. Supported values are `"world"` and `"all"`. Defaults to `"world"`. |

## CLI usage

Display help:

```sh
clserver --help
```

Display the installed version:

```sh
clserver --version
clserver -V
```

Generate shell completions:

```sh
clserver completions bash
clserver completions zsh
clserver completions fish
clserver completions powershell
clserver completions elvish
```

For bash, save the output to a location loaded by bash completion, such as `/etc/bash_completion.d/clserver` or `~/.local/share/bash-completion/completions/clserver`.

Enable verbose logging for debugging details:

```sh
clserver --verbose start survival
```

The verbose flag is global, so this is also valid:

```sh
clserver start survival --verbose
```

Use a non-default config file for staging or testing:

```sh
clserver --config /path/to/clserver.toml validate-config
clserver start survival --config /path/to/clserver.toml
```

The `--config` flag is global, so it can appear before or after the subcommand. If omitted, `clserver` uses `$XDG_CONFIG_HOME/clserver/clserver.toml`.

Validate the configuration file:

```sh
clserver validate-config
```

This also checks Minecraft `rconPassword` values against `server.properties` where available. If any server has `backup = true`, it also validates that local mirror and restic remote backup settings are usable, including required restic repository/password environment variables. If RCON password mismatches are found, the command prints a hint to run:

```sh
clserver validate-config --fix
```

With `--fix`, `clServer` prompts before updating each mismatched `rconPassword` in `clserver.toml` from the corresponding `server.properties` value. Password values are never printed. Updated passwords are written as single-quoted TOML literal strings, which is safer for generated passwords containing characters such as backslashes.

List all configured server IDs, real server names, types, and whether their `screen` sessions are running:

```sh
clserver list
```

Start a server:

```sh
clserver start survival
```

Stop a server immediately:

```sh
clserver stop survival immediate
```

The stop type defaults to `immediate`, so this is equivalent:

```sh
clserver stop survival
```

Perform a friendly Minecraft shutdown:

```sh
clserver stop survival friendly
```

Stop types are case-insensitive, so `friendly`, `FRIENDLY`, and `Friendly` are equivalent.

A friendly stop checks the online player count through RCON. If players are online, it broadcasts shutdown warnings before stopping the server.

Restart a server:

```sh
clserver restart survival
```

Attach to a server's `screen` session:

```sh
clserver attach survival
```

Show server status:

```sh
clserver status survival
```

Status output includes the server ID, real server name, configured server type, whether the `screen` session is running, server/log paths, latest screen log, Java executable, start mode, and whether stop/RCON/backup settings are configured.

Run a local mirror backup for one server:

```sh
clserver backup local survival
```

Run a remote restic backup for one server:

```sh
clserver backup remote survival
```

Run local or remote backups for all servers with `backup = true`:

```sh
clserver backup local --all
clserver backup remote --all
```

Show local mirror and remote restic backup status:

```sh
clserver backup status
```

The status command reports whether each server has `backup = true`, whether its local mirror exists, the latest local mirror modification time, whether Restic environment variables are valid, and the latest remote Restic snapshot found for each server.

List remote Restic snapshots for one server:

```sh
clserver backup snapshots survival
clserver backup snapshots survival --latest 10
```

Use the snapshot ID from this output with:

```sh
clserver restore remote survival --snapshot <snapshot-id>
```

Run remote restic retention cleanup immediately:

```sh
clserver backup cleanup
```

Preview remote restic retention cleanup without forgetting or pruning snapshots:

```sh
clserver backup cleanup --dry-run
```

Named interactive backups ignore the server's `backup` setting, so `clserver backup local survival` and `clserver backup remote survival` force that specific backup. `--all` only processes servers with `backup = true`.

If the target server is running, backup stops it, waits for the `screen` session to exit, runs the backup, and then starts it again. Minecraft servers use friendly shutdown for this flow; other server types use their configured `stopCommand`. If the server was already stopped, it is backed up without being started afterward.

Restore a server from its local backup:

```sh
clserver restore survival
```

Preview local restore changes without copying, overwriting, deleting files, or stopping a running server:

```sh
clserver restore survival --dry-run
```

Restore from a remote Restic snapshot instead of the local mirror:

```sh
clserver restore remote survival
clserver restore remote survival --snapshot latest
clserver restore remote survival --snapshot abc12345
```

Override the restore mode for a remote restore:

```sh
clserver restore remote survival --mode world
clserver restore remote survival --mode all
```

Preview a remote restore:

```sh
clserver restore remote survival --dry-run
```

Remote restore stages Restic data under `<backup.localDir>/.restic-restore/<server-id>-<timestamp>`, then uses `rsync -av --delete` from the staged data into the live server path. For real remote restores, Restic staging happens before the server is stopped; the server is stopped only for the final rsync into the live directory. The temporary restore directory is removed on success and kept for inspection on failure.

A real restore always asks for confirmation before copying files back into the server directory. The configured `restore` mode controls what is restored:

```toml
restore = "world" # default; restore only the world directory
restore = "all"   # restore the full server backup
```

TOML string values must be quoted, so use `restore = "world"`, not `restore = world`.

If the target server is running, `restore` stops it, waits for the `screen` session to exit, performs the restore, and starts it again. If the server was already stopped, it is restored without being started afterward.

Run daily maintenance across the configured fleet:

```sh
clserver maintenance
```

Maintenance performs this workflow:

1. If any Velocity server is running, stop it, wait for its `screen` session to exit, then start it again before touching other servers. If a Velocity server is not running but has `enabled = true`, start it in this pre-backend phase.
2. For all non-Velocity servers, determine which servers are currently running.
3. Process non-Velocity servers in parallel:
   - stop only servers that were running
   - use `friendly` shutdown for Minecraft; if no players are online, this becomes immediate
   - use the configured `stopCommand` immediately for other server types
   - wait for the `screen` session to exit
   - run local mirror and remote restic backups for servers with `backup = true`
   - start servers with `enabled = true`
4. On Mondays, if backup-enabled servers were processed successfully, run remote restic cleanup with `--keep-daily 56 --prune`.

## How backups work

Local backups and restores use `rsync --delete`, so the destination is made to match the source. For restore operations, that means destination files that do not exist in the backup can be deleted.

Local backups use `rsync` and copy:

```text
<global.serverDir>/<server name>/
```

to:

```text
<backup.localDir>/<server name>
```

The trailing slash on the source is intentional: it backs up the contents of the server directory into the per-server backup directory. `--delete` removes files from the backup that no longer exist in the source.

Remote backups use `restic`. If `[backup].resticEnvFile` is configured, `clserver` reads that file and passes its variables to `restic`; otherwise `restic` inherits the environment from the shell or cron job. A typical protected env file should provide values such as:

```sh
AWS_ACCESS_KEY_ID='...'
AWS_SECRET_ACCESS_KEY='...'
RESTIC_REPOSITORY='s3:s3.eu-west-3.idrivee2.com/clserver'
RESTIC_PASSWORD_FILE='/home/blackshadow/.config/clserver/secrets/restic.pwd'
```

`clserver backup remote` runs restic with tags for `clserver`, `server-id:<id>`, and `server-name:<name>`. `clserver backup status` uses those tags to query the latest snapshot per server with `restic snapshots --latest 1`; `clserver backup snapshots <server>` lists snapshots for one server using the same tags. `clserver restore remote` restores a selected snapshot into `<backup.localDir>/.restic-restore/<server-id>-<timestamp>` before rsyncing the requested restore scope into the live server directory. Remote cleanup uses:

```sh
restic forget --keep-daily 56 --prune
```

Cleanup dry-run uses:

```sh
restic forget --keep-daily 56 --dry-run
```

This keeps daily restore points for roughly eight weeks.

Restore mode `"world"` copies:

```text
<backup.localDir>/<server name>/world/
```

to:

```text
<global.serverDir>/<server name>/world
```

Restore mode `"all"` copies:

```text
<backup.localDir>/<server name>/
```

to:

```text
<global.serverDir>/<server name>
```

## How servers are started

If `startCommand` is configured for a server, `clServer` uses it directly.

If `startCommand` is not configured, `clServer` generates a Java command from:

- `javaBin`, or `javaVersion` resolved through `[java_environments]`
- `javaParams`
- `jarFile`

For example, this config:

```toml
javaVersion = "java21"
javaParams = "-Xms4G -Xmx4G"
jarFile = "server.jar"
```

may produce a command similar to:

```sh
/usr/lib/jvm/java-21-openjdk/bin/java -Xms4G -Xmx4G -jar server.jar
```

The command is launched in a detached `screen` session using the configured server name.

Generated start commands are not written to the command log by default, because custom commands may contain secrets. To log generated commands for debugging, run with `--verbose` or `-v`.

## Logging

`clServer` uses `tracing` for application logs. Logs are written to stderr for interactive feedback and to a persistent log file in the user state directory:

```text
$XDG_STATE_HOME/clserver/clserver.log
```

On most Linux systems, if `XDG_STATE_HOME` is not set, this is usually:

```text
~/.local/state/clserver/clserver.log
```

This is the preferred location for the persistent application log because it is runtime state, not user-editable configuration. Normal runs emit `INFO` and above. Passing `--verbose` or `-v` enables `DEBUG` logs, including generated start commands.

Per-server `screen` logs are written under:

```text
<global.logDir>/servers/<server name>/<timestamp>.log
```

For example:

```text
/var/log/clserver/servers/survival/2026-06-09_18:30:00.log
```

## Minecraft RCON setup

For Minecraft servers, RCON must be enabled in `server.properties`:

```properties
enable-rcon=true
rcon.port=25575
rcon.password=change-me
```

The values must match the server's `rconPort` and `rconPassword` in `clserver.toml`. You can check all configured Minecraft servers with:

```sh
clserver validate-config
```

If password mismatches are reported, you can choose to update `clserver.toml` from `server.properties` with the following command. Updated passwords are written as single-quoted TOML literal strings:

```sh
clserver validate-config --fix
```

When `server.properties` is available, `clServer` checks its `rcon.password` value against `clserver.toml` for the targeted Minecraft server before actions that need RCON, such as `stop` and `restart`. If the two passwords differ, you will be prompted to use the `server.properties` password for the current run. If the command is running non-interactively, the command fails and asks you to update `clserver.toml` manually. Commands that do not need RCON, such as `status` and `list`, do not perform this password check.

## Development

Most application code lives in `src/lib.rs` and its modules. `src/main.rs` is intentionally kept as a small binary entry point.

Useful development commands:

```sh
cargo fmt
cargo check
cargo test
cargo clippy --all-targets --all-features
```

## Current limitations

- `backup` is exposed by the CLI but not implemented yet.
- Status output is based on local process/configuration checks and does not currently query Minecraft player state through RCON.
- Minecraft RCON responses are currently read as a single packet, which is enough for simple commands but may not handle very large responses.
