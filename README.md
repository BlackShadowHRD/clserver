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
- `rsync`, for backup and maintenance workflows that copy server files

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
sudo apt install screen bash rsync
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
backupDir = "/srv/backups/clserver"

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
| `backupDir` | Required when any server has `backup = true` | Base directory for `rsync` backups. Each server is backed up to `<backupDir>/<server name>`. |

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
| `backup` | No | Whether `backup` and `maintenance` should copy this server with `rsync`. Requires `global.backupDir` when true. |

## CLI usage

Display help:

```sh
clserver --help
```

Enable verbose logging for debugging details:

```sh
clserver --verbose start survival
```

The verbose flag is global, so this is also valid:

```sh
clserver start survival --verbose
```

Validate the configuration file:

```sh
clserver validate-config
```

This also checks Minecraft `rconPassword` values against `server.properties` where available. If mismatches are found, the command prints a hint to run:

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

Run a backup:

```sh
clserver backup survival
```

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
   - run `rsync -av --delete` for servers with `backup = true`
   - start servers with `enabled = true`

## How backups work

Backups use `rsync` and copy:

```text
<global.serverDir>/<server name>/
```

to:

```text
<global.backupDir>/<server name>
```

The trailing slash on the source is intentional: it backs up the contents of the server directory into the per-server backup directory. `--delete` removes files from the backup that no longer exist in the source.

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
