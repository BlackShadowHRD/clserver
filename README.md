# clServer

`clServer` is a small Rust CLI for managing CatLord Minecraft, Velocity, and future Hytale servers. It starts servers inside `screen` sessions, writes logs, and can stop Minecraft servers through RCON.

## Features

- Start configured servers in detached `screen` sessions
- Stop Minecraft servers through RCON
- Stop Velocity/Hytale-style servers by sending a configured stop command to `screen`
- Restart, attach, status, and backup commands exposed through the CLI
- Per-server screen log files
- Global command log file
- Configurable Java installations per server

> **Note:** `status` and `backup` are currently placeholders and are not implemented yet.

## Requirements

- Rust/Cargo, for building from source
- `screen`, for process/session management
- `bash`, used when launching generated start commands
- Java runtime(s) for the servers you want to run
- Minecraft RCON enabled for Minecraft stop/restart support

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

## Configuration

`clServer` reads its configuration from:

```text
$XDG_CONFIG_HOME/cls/cls.toml
```

On most Linux systems, if `XDG_CONFIG_HOME` is not set, this is usually:

```text
~/.config/cls/cls.toml
```

Create the config directory if it does not already exist:

```sh
mkdir -p ~/.config/cls
```

Then create:

```text
~/.config/cls/cls.toml
```

### Example configuration

```toml
[global]
serverDir = "/srv/servers"
logDir = "/var/log/clserver"

[java_environments]
default = "/usr/bin/java"
java17 = "/usr/lib/jvm/java-17-openjdk/bin/java"
java21 = "/usr/lib/jvm/java-21-openjdk/bin/java"

[servers.survival]
name = "survival"
type = "minecraft"
javaVersion = "java21"
javaParams = "-Xms4G -Xmx4G"
jarFile = "server.jar"
rconPort = 25575
rconPassword = "change-me"
backup = true

[servers.velocity]
name = "velocity"
type = "velocity"
javaVersion = "java17"
javaParams = "-Xms1G -Xmx1G"
jarFile = "velocity.jar"
stopCommand = "end"
backup = false
```

## Configuration reference

### `[global]`

| Field | Required | Description |
| --- | --- | --- |
| `serverDir` | Yes | Base directory containing all server directories. Each server is expected at `<serverDir>/<server name>`. |
| `logDir` | Yes | Base directory for per-server screen logs. |

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

### `[servers.<key>]`

Each server is configured under a table such as:

```toml
[servers.survival]
```

| Field | Required | Description |
| --- | --- | --- |
| `name` | Yes | Server name. This is also used as the `screen` session name. |
| `type` | Yes | Server type. Supported values: `minecraft`, `velocity`, `hytale`. |
| `javaBin` | No | Direct path to a Java executable. Overrides `javaVersion`. |
| `javaVersion` | No | Key from `[java_environments]`. Defaults to `default` if `javaBin` is not set. |
| `javaParams` | No | Additional JVM parameters, such as memory settings. |
| `jarFile` | Usually | Jar file to run when `startCommand` is not provided. |
| `startCommand` | No | Full custom command used to start the server. If set, this overrides generated Java command behavior. |
| `stopCommand` | Required for Velocity/Hytale stop/restart | Command sent to the server's `screen` session when stopping. |
| `rconPort` | Required for Minecraft | RCON port for Minecraft servers. |
| `rconPassword` | Required for Minecraft | RCON password for Minecraft servers. |
| `backup` | No | Whether this server should be considered for backup behavior. Backup is not implemented yet. |

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

Status output includes the configured server type, whether the `screen` session is running, server/log paths, latest screen log, Java executable, start mode, and whether stop/RCON/backup settings are configured.

Run a backup:

```sh
clserver backup survival
```

> `backup` is currently not implemented.

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
$XDG_STATE_HOME/cls/clserver.log
```

On most Linux systems, if `XDG_STATE_HOME` is not set, this is usually:

```text
~/.local/state/cls/clserver.log
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

The values must match the server's `rconPort` and `rconPassword` in `cls.toml`.

When `server.properties` is available, `clServer` checks its `rcon.password` value against `cls.toml` while loading the config. If the two passwords differ, you will be prompted to use the `server.properties` password for the current run. If the command is running non-interactively, config loading fails and asks you to update `cls.toml` manually.

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
