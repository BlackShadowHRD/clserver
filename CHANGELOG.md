# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) where practical.

## [Unreleased]

### Added

- Project documentation in `README.md`, including installation and deployment guidance.
- Configuration validation with aggregated error messages for missing or invalid settings.
- Minecraft RCON password consistency check against `server.properties`.
- Global `--verbose` / `-v` CLI option for debugging logs.
- Useful `status` output for configured servers, including screen state, paths, latest log, Java executable, and configured capability summary.
- `list` command showing all configured servers with type and running/stopped state.
- `validate-config` command for checking configuration validity and Minecraft RCON password consistency without targeting a server.
- `validate-config --fix` option to prompt before updating mismatched `rconPassword` values in `clserver.toml` from `server.properties`, writing passwords as single-quoted TOML literal strings.
- `tracing`-based application logging with stderr output and persistent file logging under `$XDG_STATE_HOME/clserver/clserver.log`.
- Unit tests for configuration validation, `server.properties` RCON password parsing, and CLI subcommand parsing.
- `maintenance` command for daily fleet maintenance with Velocity-first handling and parallel backend stop/backup/start processing.
- `enabled` server setting for maintenance restart decisions.
- `[backup].localDir` setting and `rsync`-based local mirror server backups.
- `backup local`, `backup remote`, `backup status`, and `backup cleanup` subcommands.
- Restic-based remote backups tagged by clServer, server ID, and server name.
- Remote backup cleanup using `restic forget --keep-daily 56 --prune`.
- `backup status` reporting backup-enabled state, local mirror status, latest local mirror timestamp, Restic env validity, and latest remote Restic snapshot per server.
- Optional `[backup].resticEnvFile` setting so `clserver` can load restic environment variables itself.
- `validate-config` now checks restic repository/password settings when backups are enabled.
- `--version` / `-V` CLI flag powered by the Cargo package version.
- `restore <server>` command for restoring either the `world` directory or the full server backup with confirmation.
- Per-server `restore` setting with supported values `"world"` and `"all"`, defaulting to `"world"`.

### Changed

- Configuration loading now fails early when required global, Java, or server fields are invalid.
- Minecraft RCON password mismatch prompts now run only for the targeted server and only for actions that need RCON (`stop` and `restart`).
- Failed external `screen` commands now return errors instead of being logged as failures while exiting successfully.
- Filesystem errors while creating/opening/writing log files now include context and are propagated instead of being ignored.
- Internal server and global filesystem paths now use `PathBuf` instead of string concatenation.
- `screen` session detection now parses exact session names instead of using substring matching.
- CLI actions now use Clap subcommands, e.g. `clserver start survival`, instead of mutually exclusive action flags.
- Generated server start commands are only logged when verbose mode is enabled.
- Stop type parsing is now case-insensitive.
- Application logic has been split into `src/lib.rs`, leaving `src/main.rs` as a minimal binary entry point.
- Removed `unwrap()` usage in production RCON parsing and CLI tests.
- Replaced custom application logging with `tracing` events and level-based verbose logging.
- Replaced `toml` dependency with `toml_edit 0.25.12` for both deserialization and targeted config edits.
- XDG config and state paths now use `clserver` instead of the legacy short name `cls`:
  - `$XDG_CONFIG_HOME/clserver/clserver.toml`
  - `$XDG_STATE_HOME/clserver/clserver.log`
- Server table keys now act as command IDs/shortcuts, while `name` is the real server directory and `screen` session name.
- Server `name` values are now validated as unique; duplicate TOML table IDs are rejected by TOML parsing.
- `backup local <server>` now runs `rsync -av --delete` instead of the old placeholder backup.
- `backup local <server>` and `backup remote <server>` stop a running server before backup and restart it afterward.
- Daily maintenance now runs both local mirror and remote restic backups for servers with `backup = true`.
- Daily maintenance logging now includes clearer phase, decision, skip, backup, stop, and start messages.

### Fixed

- Nothing yet.

## [0.1.0] - Initial development release

### Added

- Rust CLI application for managing configured game servers.
- Configuration loading from `$XDG_CONFIG_HOME/clserver/clserver.toml`.
- Global configuration for server and log directories.
- Per-server configuration for Minecraft, Velocity, and Hytale server types.
- Java environment resolution through `[java_environments]`.
- Server startup in detached `screen` sessions.
- Per-server `screen` log file generation.
- Global command logging under `$XDG_STATE_HOME/clserver/clserver.log`.
- Minecraft RCON client support.
- Minecraft immediate stop support through RCON `stop` command.
- Minecraft friendly stop support with player-count check and shutdown warnings.
- Generic server stop support by sending configured `stopCommand` to `screen`.
- Restart support for Minecraft and generic servers.
- Attach support for existing `screen` sessions.
- CLI options for start, stop, restart, attach, status, and backup actions.

### Known limitations

- `status` command is exposed but not implemented yet.
- `screen` session detection uses substring matching against `screen -ls` output.
- Minecraft RCON responses are currently read as a single packet.
- There are currently no automated tests.
