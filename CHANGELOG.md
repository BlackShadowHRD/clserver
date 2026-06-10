# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) where practical.

## [Unreleased]

### Added

- Project documentation in `README.md`.
- Configuration validation with aggregated error messages for missing or invalid settings.
- Minecraft RCON password consistency check against `server.properties`.
- Global `--verbose` / `-v` CLI option for debugging logs.
- Useful `status` output for configured servers, including screen state, paths, latest log, Java executable, and configured capability summary.
- `tracing`-based application logging with stderr output and persistent file logging under `$XDG_STATE_HOME/cls/clserver.log`.
- Unit tests for configuration validation, `server.properties` RCON password parsing, and CLI subcommand parsing.

### Changed

- Configuration loading now fails early when required global, Java, or server fields are invalid.
- Minecraft server loading now prompts when `cls.toml` and `server.properties` contain different RCON passwords.
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

### Fixed

- Nothing yet.

## [0.1.0] - Initial development release

### Added

- Rust CLI application for managing configured game servers.
- Configuration loading from `$XDG_CONFIG_HOME/cls/cls.toml`.
- Global configuration for server and log directories.
- Per-server configuration for Minecraft, Velocity, and Hytale server types.
- Java environment resolution through `[java_environments]`.
- Server startup in detached `screen` sessions.
- Per-server `screen` log file generation.
- Global command logging under `$XDG_STATE_HOME/cls/clserver.log`.
- Minecraft RCON client support.
- Minecraft immediate stop support through RCON `stop` command.
- Minecraft friendly stop support with player-count check and shutdown warnings.
- Generic server stop support by sending configured `stopCommand` to `screen`.
- Restart support for Minecraft and generic servers.
- Attach support for existing `screen` sessions.
- CLI options for start, stop, restart, attach, status, and backup actions.

### Known limitations

- `status` command is exposed but not implemented yet.
- `backup` command is exposed but not implemented yet.
- `screen` session detection uses substring matching against `screen -ls` output.
- Minecraft RCON responses are currently read as a single packet.
- There are currently no automated tests.
