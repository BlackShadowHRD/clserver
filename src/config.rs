use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Value};

/// Top-level application configuration loaded from `clserver.toml`.
///
/// The config file is expected to contain global paths, named Java environments,
/// and one or more configured servers under `[servers.<name>]` tables. Serde
/// defaults are used here so that validation can report friendly, aggregated
/// errors instead of failing immediately on missing sections.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Global filesystem locations shared by all configured servers.
    #[serde(default)]
    pub global: GlobalConfig,

    /// Named Java executables referenced by server `javaVersion` settings.
    ///
    /// For example, `javaVersion = "java21"` resolves through this map. If a
    /// server does not set `javaBin` or `javaVersion`, it falls back to the
    /// `default` entry.
    #[serde(default)]
    pub java_environments: HashMap<String, String>,

    /// Server definitions keyed by server ID/shortcut.
    ///
    /// The table key is what users pass to CLI commands, e.g. `[servers.CLS4]`
    /// is managed with `clserver status CLS4`. The contained `name` field is
    /// the real server directory and `screen` session name.
    #[serde(default)]
    pub servers: HashMap<String, ServerConfig>,

    /// Backup tool settings.
    #[serde(default)]
    pub backup: BackupConfig,
}

/// Global filesystem settings shared by every server.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GlobalConfig {
    /// Base directory containing server directories.
    ///
    /// A server named `survival` is expected to live at
    /// `<serverDir>/survival`.
    #[serde(rename = "serverDir")]
    pub server_dir: PathBuf,

    /// Base directory for per-server `screen` logs.
    #[serde(rename = "logDir")]
    pub log_dir: PathBuf,
}

/// Backup tool settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BackupConfig {
    /// Base directory for local mirror backups.
    #[serde(rename = "localDir")]
    pub local_dir: Option<PathBuf>,

    /// Shell-style env file loaded for restic commands.
    #[serde(rename = "resticEnvFile")]
    pub restic_env_file: Option<PathBuf>,
}

/// Supported server runtime categories.
///
/// The type controls how stop/restart actions are performed. Minecraft uses
/// RCON, while Velocity and Hytale use a configured command sent to `screen`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    /// Minecraft server managed through Java, `screen`, and RCON.
    Minecraft,
    /// Velocity proxy server managed through Java and `screen`.
    Velocity,
    /// Placeholder for future Hytale server support.
    Hytale,
}

impl fmt::Display for ServerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerType::Minecraft => write!(f, "minecraft"),
            ServerType::Velocity => write!(f, "velocity"),
            ServerType::Hytale => write!(f, "hytale"),
        }
    }
}

/// Scope to restore from a server backup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMode {
    /// Restore only the `world` directory.
    #[default]
    World,
    /// Restore every backed-up file for the server.
    All,
}

impl fmt::Display for RestoreMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreMode::World => write!(f, "world"),
            RestoreMode::All => write!(f, "all"),
        }
    }
}

/// Per-server configuration loaded from a `[servers.<id>]` table.
///
/// A server can either provide a complete `startCommand`, or allow clServer to
/// generate a Java command from `javaBin`/`javaVersion`, `javaParams`, and
/// `jarFile`.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Real server directory and `screen` session name.
    ///
    /// This may differ from the surrounding table key. For example,
    /// `[servers.CLS4]` can use `name = "CatLordSurvival"`, allowing `CLS4` to
    /// act as a shorter command-line ID.
    pub name: String,

    /// Server implementation type.
    #[serde(rename = "type")]
    pub server_type: ServerType,

    /// Direct path to the Java executable for this server.
    ///
    /// When set, this takes precedence over `javaVersion`.
    #[serde(rename = "javaBin")]
    pub java_bin: Option<String>,

    /// Named Java environment to resolve through `[java_environments]`.
    ///
    /// If neither `javaBin` nor `javaVersion` is set, `default` is used.
    #[serde(rename = "javaVersion")]
    pub java_version: Option<String>,

    /// Additional JVM arguments used when generating a Java start command.
    #[serde(rename = "javaParams")]
    pub java_params: Option<String>,

    /// Jar file to execute when `startCommand` is not provided.
    #[serde(rename = "jarFile")]
    pub jar_file: Option<String>,

    /// Full custom command used to start the server.
    ///
    /// When present, this bypasses generated Java command construction.
    #[serde(rename = "startCommand")]
    pub start_command: Option<String>,

    /// Command sent to the server's `screen` session for generic shutdowns.
    ///
    /// Required for Velocity and Hytale servers.
    #[serde(rename = "stopCommand")]
    pub stop_command: Option<String>,

    /// Minecraft RCON port.
    ///
    /// Required for Minecraft stop/restart actions.
    #[serde(rename = "rconPort")]
    pub rcon_port: Option<u16>,

    /// Minecraft RCON password.
    ///
    /// Required for Minecraft stop/restart actions.
    #[serde(rename = "rconPassword")]
    pub rcon_password: Option<String>,

    /// Whether this server should be started by whole-fleet maintenance.
    ///
    /// Missing values default to false for maintenance restart decisions.
    pub enabled: Option<bool>,

    /// Whether this server should participate in backup workflows.
    pub backup: Option<bool>,

    /// Restore scope used by `clserver restore <server>`.
    ///
    /// Missing values default to `world`.
    pub restore: Option<RestoreMode>,
}

/// Load, parse, and validate the user's `clserver.toml` configuration file.
///
/// The file path is resolved by `crate::paths::config_file`. TOML syntax errors
/// are reported separately from semantic validation errors so users can tell
/// whether the file is malformed or merely incomplete.
pub fn load_config() -> Result<Config> {
    let config_file = crate::paths::config_file()?;

    let text = fs::read_to_string(&config_file).with_context(|| {
        format!(
            "Configuration file '{}' not found or unreadable",
            config_file.display()
        )
    })?;

    let config: Config = toml_edit::de::from_str(&text).context("Invalid TOML file")?;
    validate_config(&config)?;
    Ok(config)
}

/// Validate a parsed configuration and return all discovered problems together.
///
/// This function intentionally accumulates validation errors instead of failing
/// fast. That makes config editing less frustrating because a single run can
/// report every missing field or invalid reference that needs attention.
pub fn validate_config(config: &Config) -> Result<()> {
    let mut errors = Vec::new();

    if is_blank_path(&config.global.server_dir) {
        errors.push("global.serverDir is required and cannot be empty".to_string());
    }

    if is_blank_path(&config.global.log_dir) {
        errors.push("global.logDir is required and cannot be empty".to_string());
    }

    if config
        .backup
        .local_dir
        .as_ref()
        .is_some_and(|path| is_blank_path(path))
    {
        errors.push("backup.localDir cannot be empty when configured".to_string());
    }

    if config
        .backup
        .restic_env_file
        .as_ref()
        .is_some_and(|path| is_blank_path(path))
    {
        errors.push("backup.resticEnvFile cannot be empty when configured".to_string());
    }

    if config.servers.is_empty() {
        errors.push("at least one server must be configured under [servers]".to_string());
    }

    for (name, java_bin) in &config.java_environments {
        if name.trim().is_empty() {
            errors.push("java environment names cannot be empty".to_string());
        }

        if java_bin.trim().is_empty() {
            errors.push(format!(
                "java environment '{name}' has an empty executable path"
            ));
        }
    }

    validate_unique_server_names(config, &mut errors);

    for (server_key, server) in &config.servers {
        validate_server_config(server_key, server, config, &mut errors);
    }

    let any_backup_enabled = has_backup_enabled_servers(config);

    if any_backup_enabled
        && config
            .backup
            .local_dir
            .as_ref()
            .is_none_or(|path| is_blank_path(path))
    {
        errors.push("backup.localDir is required when any server has backup = true".to_string());
    }

    if any_backup_enabled {
        validate_restic_environment_config(&config.backup, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        bail!("Invalid configuration:\n- {}", errors.join("\n- "))
    }
}

pub fn restic_environment_check_success_message(config: &Config) -> Option<String> {
    has_backup_enabled_servers(config).then(|| {
        if let Some(env_file) = config.backup.restic_env_file.as_ref() {
            format!(
                "Remote backup Restic environment check valid using '{}'.",
                env_file.display()
            )
        } else {
            "Remote backup Restic environment check valid using process environment.".to_string()
        }
    })
}

fn has_backup_enabled_servers(config: &Config) -> bool {
    config
        .servers
        .values()
        .any(|server| server.backup.unwrap_or(false))
}

fn validate_restic_environment_config(backup: &BackupConfig, errors: &mut Vec<String>) {
    let entries = if let Some(env_file) = backup.restic_env_file.as_ref() {
        match load_restic_env_file(env_file) {
            Ok(entries) => entries,
            Err(err) => {
                errors.push(format!("backup.resticEnvFile is invalid: {err:#}"));
                return;
            }
        }
    } else {
        std::env::vars().collect()
    };

    if restic_env_value(&entries, "RESTIC_REPOSITORY").is_none() {
        errors.push(restic_missing_setting_message(
            "RESTIC_REPOSITORY",
            backup.restic_env_file.as_ref(),
        ));
    }

    if restic_env_value(&entries, "RESTIC_PASSWORD").is_none()
        && restic_env_value(&entries, "RESTIC_PASSWORD_FILE").is_none()
        && restic_env_value(&entries, "RESTIC_PASSWORD_COMMAND").is_none()
    {
        errors.push(restic_missing_setting_message(
            "RESTIC_PASSWORD, RESTIC_PASSWORD_FILE, or RESTIC_PASSWORD_COMMAND",
            backup.restic_env_file.as_ref(),
        ));
    }
}

fn restic_missing_setting_message(setting: &str, env_file: Option<&PathBuf>) -> String {
    if let Some(env_file) = env_file {
        format!(
            "backup.resticEnvFile '{}' must define {setting}",
            env_file.display()
        )
    } else {
        format!(
            "backup.resticEnvFile is not configured and environment must define {setting} when any server has backup = true"
        )
    }
}

fn load_restic_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read restic env file '{}'", path.display()))?;

    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_restic_env_line(path, index + 1, line).transpose())
        .collect()
}

fn parse_restic_env_line(
    path: &Path,
    line_number: usize,
    line: &str,
) -> Result<Option<(String, String)>> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
    let (key, value) = line.split_once('=').ok_or_else(|| {
        anyhow!(
            "Invalid env file line {} in '{}': expected KEY=value",
            line_number,
            path.display()
        )
    })?;
    let key = key.trim();

    if !is_valid_env_key(key) {
        bail!(
            "Invalid env variable name '{}' on line {} in '{}'",
            key,
            line_number,
            path.display()
        );
    }

    Ok(Some((
        key.to_string(),
        parse_restic_env_value(value.trim()),
    )))
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn parse_restic_env_value(value: &str) -> String {
    let value = strip_inline_comment(value).trim();

    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_string();
        }
    }

    value.to_string()
}

fn strip_inline_comment(value: &str) -> &str {
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;

    for (index, ch) in value.char_indices() {
        match ch {
            '\'' if !in_double_quotes => in_single_quotes = !in_single_quotes,
            '"' if !in_single_quotes => in_double_quotes = !in_double_quotes,
            '#' if !in_single_quotes && !in_double_quotes => return &value[..index],
            _ => {}
        }
    }

    value
}

fn restic_env_value(entries: &[(String, String)], key: &str) -> Option<String> {
    entries
        .iter()
        .rev()
        .find_map(|(entry_key, value)| (entry_key == key).then(|| value.clone()))
}

fn validate_unique_server_names(config: &Config, errors: &mut Vec<String>) {
    let mut names: HashMap<&str, &str> = HashMap::new();

    for (server_key, server) in &config.servers {
        let name = server.name.trim();
        if name.is_empty() {
            continue;
        }

        if let Some(existing_key) = names.insert(name, server_key.as_str()) {
            errors.push(format!(
                "server '{}' and server '{}' both use name '{}'. Server names must be unique because they map to directories and screen sessions.",
                existing_key, server_key, server.name
            ));
        }
    }
}

/// Validate one `[servers.<id>]` entry and append any problems to `errors`.
fn validate_server_config(
    server_key: &str,
    server: &ServerConfig,
    config: &Config,
    errors: &mut Vec<String>,
) {
    let label = format!("server '{server_key}'");

    if server_key.trim().is_empty() {
        errors.push("server table names cannot be empty".to_string());
    }

    if server.name.trim().is_empty() {
        errors.push(format!("{label} is missing a non-empty name"));
    }

    validate_optional_non_empty(&label, "javaBin", server.java_bin.as_deref(), errors);
    validate_optional_non_empty(
        &label,
        "javaVersion",
        server.java_version.as_deref(),
        errors,
    );
    validate_optional_non_empty(&label, "javaParams", server.java_params.as_deref(), errors);
    validate_optional_non_empty(&label, "jarFile", server.jar_file.as_deref(), errors);
    validate_optional_non_empty(
        &label,
        "startCommand",
        server.start_command.as_deref(),
        errors,
    );
    validate_optional_non_empty(
        &label,
        "stopCommand",
        server.stop_command.as_deref(),
        errors,
    );
    validate_optional_non_empty(
        &label,
        "rconPassword",
        server.rcon_password.as_deref(),
        errors,
    );

    if is_blank(server.start_command.as_deref()) && is_blank(server.jar_file.as_deref()) {
        errors.push(format!(
            "{label} needs either startCommand or jarFile to start"
        ));
    }

    if let Some(java_version) = server
        .java_version
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        if is_blank(server.java_bin.as_deref())
            && !config.java_environments.contains_key(java_version)
        {
            errors.push(format!(
                "{label} references unknown javaVersion '{java_version}'"
            ));
        }
    } else if is_blank(server.java_bin.as_deref())
        && !config.java_environments.contains_key("default")
    {
        errors.push(format!(
            "{label} does not set javaBin or javaVersion, but no 'default' Java environment is configured"
        ));
    }

    match server.server_type {
        ServerType::Minecraft => {
            if server.rcon_port.is_none() {
                errors.push(format!("{label} is a Minecraft server but has no rconPort"));
            }

            if is_blank(server.rcon_password.as_deref()) {
                errors.push(format!(
                    "{label} is a Minecraft server but has no rconPassword"
                ));
            }
        }
        ServerType::Velocity | ServerType::Hytale => {
            if is_blank(server.stop_command.as_deref()) {
                errors.push(format!(
                    "{label} is a {} server but has no stopCommand",
                    server.server_type
                ));
            }
        }
    }
}

/// Reject optional string fields that are present but blank.
///
/// Missing optional fields may be valid, but blank strings usually indicate a
/// partially edited config and should not satisfy required-field checks.
fn validate_optional_non_empty(
    label: &str,
    field_name: &str,
    value: Option<&str>,
    errors: &mut Vec<String>,
) {
    if value.is_some_and(|value| value.trim().is_empty()) {
        errors.push(format!("{label} has an empty {field_name}"));
    }
}

/// Treat missing strings and whitespace-only strings as blank.
fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|value| value.trim().is_empty())
}

/// Treat empty and whitespace-only paths as blank.
fn is_blank_path(value: &Path) -> bool {
    value.as_os_str().to_string_lossy().trim().is_empty()
}

/// Compare one Minecraft server's RCON password against `server.properties` when available.
///
/// Minecraft stores its active RCON password in `server.properties`. If the
/// password configured in `clserver.toml` differs, RCON commands will fail even though
/// the clServer config looks valid. In that case, prompt the user to trust the
/// `server.properties` value for the current process.
///
/// This function does not rewrite `clserver.toml`; it updates the in-memory config so
/// the requested command can continue safely. The prompt tells the user to update
/// `clserver.toml` manually so future runs stay consistent.
///
/// This should only be called for actions that actually need RCON, such as
/// Minecraft stop/restart. Commands like `status` and `list` should not prompt
/// for unrelated RCON password mismatches.
pub fn reconcile_minecraft_rcon_password(config: &mut Config, server_name: &str) -> Result<()> {
    let server_dir = config.global.server_dir.clone();
    let server = config
        .servers
        .get_mut(server_name)
        .ok_or_else(|| anyhow!("Server '{server_name}' not found in configuration file."))?;

    if server.server_type != ServerType::Minecraft {
        return Ok(());
    }

    let properties_file = server_dir.join(&server.name).join("server.properties");
    let properties_text = match fs::read_to_string(&properties_file) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to read server.properties for Minecraft server '{}' at '{}'",
                    server.name,
                    properties_file.display()
                )
            });
        }
    };

    let Some(properties_password) = parse_server_properties_rcon_password(&properties_text) else {
        return Ok(());
    };
    let Some(config_password) = server.rcon_password.as_deref() else {
        return Ok(());
    };

    if config_password == properties_password {
        return Ok(());
    }

    if prompt_to_use_server_properties_rcon_password(&server.name, &properties_file)? {
        server.rcon_password = Some(properties_password);
        tracing::info!(
            server = %server.name,
            properties_file = %properties_file.display(),
            "using RCON password from server.properties for current run"
        );
        println!(
            "Using RCON password from server.properties for server '{}'. Please update clserver.toml to keep future runs in sync.",
            server.name
        );
    } else {
        bail!(
            "RCON password mismatch for server '{}'. Update clserver.toml to match '{}' or rerun and accept the server.properties value.",
            server.name,
            properties_file.display()
        );
    }

    Ok(())
}

/// Extract `rcon.password` from a Minecraft `server.properties` file.
///
/// Java properties files commonly use `key=value`, but `key:value` is also
/// accepted. Comments and empty lines are ignored.
fn parse_server_properties_rcon_password(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            return None;
        }

        let separator_index = match (line.find('='), line.find(':')) {
            (Some(equals), Some(colon)) => equals.min(colon),
            (Some(equals), None) => equals,
            (None, Some(colon)) => colon,
            (None, None) => return None,
        };

        let key = line[..separator_index].trim();
        let value = line[separator_index + 1..].trim();

        (key == "rcon.password").then(|| value.to_string())
    })
}

/// Prompt before replacing the configured RCON password for the current run.
fn prompt_to_use_server_properties_rcon_password(
    server_name: &str,
    properties_file: &Path,
) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "RCON password mismatch for server '{server_name}', but stdin is not interactive. Update clserver.toml to match '{}'.",
            properties_file.display()
        );
    }

    print!(
        "RCON password mismatch for server '{server_name}'. Use password from '{}' for this run? [y/N] ",
        properties_file.display()
    );
    io::stdout().flush().context("Failed to flush prompt")?;

    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .context("Failed to read prompt response")?;

    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[derive(Debug)]
struct RconPasswordMismatch {
    server_key: String,
    server_name: String,
    properties_file: PathBuf,
    properties_password: String,
}

pub fn validate_or_fix_minecraft_rcon_passwords(config: &Config, fix: bool) -> Result<()> {
    let mismatches = find_minecraft_rcon_password_mismatches(config)?;

    if mismatches.is_empty() {
        print_validate_config_success(config);
        return Ok(());
    }

    if !fix {
        println!("Configuration is structurally valid, but RCON password mismatches were found:");
        for mismatch_ in &mismatches {
            println!(
                "- Minecraft server '{}': clserver.toml rconPassword differs from {}",
                mismatch_.server_name,
                mismatch_.properties_file.display()
            );
        }
        println!();
        println!(
            "Run `clserver validate-config --fix` to update clserver.toml from server.properties."
        );
        bail!("RCON password mismatches found");
    }

    fix_minecraft_rcon_passwords(&mismatches)?;
    if let Some(message) = restic_environment_check_success_message(config) {
        println!("{message}");
    }
    Ok(())
}

fn print_validate_config_success(config: &Config) {
    if let Some(message) = restic_environment_check_success_message(config) {
        println!("{message}");
    }
    println!("Configuration is valid.");
}

fn find_minecraft_rcon_password_mismatches(config: &Config) -> Result<Vec<RconPasswordMismatch>> {
    let mut mismatches = Vec::new();

    for (server_key, server) in &config.servers {
        if let Some(mismatch_) =
            minecraft_rcon_password_mismatch(&config.global.server_dir, server_key, server)?
        {
            mismatches.push(mismatch_);
        }
    }

    mismatches.sort_by(|a, b| a.server_name.cmp(&b.server_name));
    Ok(mismatches)
}

fn minecraft_rcon_password_mismatch(
    server_dir: &Path,
    server_key: &str,
    server: &ServerConfig,
) -> Result<Option<RconPasswordMismatch>> {
    if server.server_type != ServerType::Minecraft {
        return Ok(None);
    }

    let properties_file = server_dir.join(&server.name).join("server.properties");
    let properties_text = match fs::read_to_string(&properties_file) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Failed to read server.properties for Minecraft server '{}' at '{}'",
                    server.name,
                    properties_file.display()
                )
            });
        }
    };

    let Some(properties_password) = parse_server_properties_rcon_password(&properties_text) else {
        return Ok(None);
    };
    let Some(config_password) = server.rcon_password.as_deref() else {
        return Ok(None);
    };

    if config_password == properties_password {
        return Ok(None);
    }

    Ok(Some(RconPasswordMismatch {
        server_key: server_key.to_string(),
        server_name: server.name.clone(),
        properties_file,
        properties_password,
    }))
}

fn fix_minecraft_rcon_passwords(mismatches: &[RconPasswordMismatch]) -> Result<()> {
    let config_file = crate::paths::config_file()?;
    let text = fs::read_to_string(&config_file).with_context(|| {
        format!(
            "Configuration file '{}' not found or unreadable",
            config_file.display()
        )
    })?;
    let mut document: DocumentMut = text
        .parse()
        .with_context(|| format!("Failed to parse '{}' for editing", config_file.display()))?;

    let mut fixed = 0;
    let mut skipped = 0;

    for mismatch_ in mismatches {
        if prompt_to_update_toml_rcon_password(mismatch_)? {
            document["servers"][mismatch_.server_key.as_str()]["rconPassword"] =
                literal_string_item(&mismatch_.properties_password).with_context(|| {
                    format!(
                        "Failed to format rconPassword for server '{}' as a TOML literal string",
                        mismatch_.server_name
                    )
                })?;
            fixed += 1;
            println!(
                "Updated rconPassword for server '{}' from server.properties.",
                mismatch_.server_name
            );
        } else {
            skipped += 1;
        }
    }

    if fixed > 0 {
        fs::write(&config_file, document.to_string())
            .with_context(|| format!("Failed to write '{}'", config_file.display()))?;
        println!(
            "Updated {} server configuration(s) in {}.",
            fixed,
            config_file.display()
        );
    }

    if skipped > 0 {
        bail!(
            "{} RCON password mismatch(es) remain. Re-run `clserver validate-config --fix` or update clserver.toml manually.",
            skipped
        );
    }

    println!("Configuration is valid.");
    Ok(())
}

fn literal_string_item(value: &str) -> Result<Item> {
    if value.contains('\'') {
        bail!(
            "Cannot write rconPassword as a single-quoted TOML literal string because it contains a single quote. Update clserver.toml manually for this server."
        );
    }

    let literal = format!("'{value}'");
    let value = literal
        .parse::<Value>()
        .with_context(|| "Password cannot be represented as a single-line TOML literal string")?;

    Ok(Item::Value(value))
}

fn prompt_to_update_toml_rcon_password(mismatch_: &RconPasswordMismatch) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "RCON password mismatch for server '{}', but stdin is not interactive. Update clserver.toml to match '{}'.",
            mismatch_.server_name,
            mismatch_.properties_file.display()
        );
    }

    print!(
        "RCON password mismatch for server '{}'. Update clserver.toml from '{}'? [y/N] ",
        mismatch_.server_name,
        mismatch_.properties_file.display()
    );
    io::stdout().flush().context("Failed to flush prompt")?;

    let mut response = String::new();
    io::stdin()
        .read_line(&mut response)
        .context("Failed to read prompt response")?;

    Ok(matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Resolve the Java executable path for a server.
///
/// Resolution order:
///
/// 1. Use `javaBin` directly when configured.
/// 2. Use `javaVersion` to look up a named Java environment.
/// 3. Fall back to the `default` Java environment.
///
/// Config validation should catch unknown Java versions before this function is
/// called during normal application startup, but this still returns a descriptive
/// error for direct callers and tests.
pub fn resolve_java_bin(
    config: &ServerConfig,
    java_environments: &HashMap<String, String>,
) -> Result<String> {
    if let Some(java_bin) = &config.java_bin {
        return Ok(java_bin.clone());
    }

    let java_version = config.java_version.as_deref().unwrap_or("default");
    java_environments.get(java_version).cloned().ok_or_else(|| {
        let mut valid: Vec<_> = java_environments.keys().cloned().collect();
        valid.sort();
        anyhow!(
            "Unknown javaVersion '{}' for server '{}'. Valid options: {}",
            java_version,
            config.name,
            valid.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_config(toml: &str) -> Config {
        toml_edit::de::from_str(toml).expect("test config should parse")
    }

    fn write_test_restic_env(name: &str, text: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("clserver-{name}-{}-restic.env", std::process::id()));
        fs::write(&path, text).expect("test restic env file should be written");
        path
    }

    #[test]
    fn validates_complete_minecraft_config() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            "#,
        );

        validate_config(&config).expect("config should be valid");
    }

    #[test]
    fn validates_local_backup_dir_when_backup_is_enabled() {
        let restic_env = write_test_restic_env(
            "validates-local-backup-dir",
            r#"
            RESTIC_REPOSITORY='s3:s3.example.com/bucket'
            RESTIC_PASSWORD_FILE='/secure/restic.pwd'
            "#,
        );
        let config = parse_config(&format!(
            r#"
        [global]
        serverDir = "/srv/servers"
        logDir = "/var/log/clserver"

        [backup]
        localDir = "/srv/backups"
        resticEnvFile = "{}"

        [java_environments]
        default = "/usr/bin/java"

        [servers.survival]
        name = "survival"
        type = "minecraft"
        jarFile = "server.jar"
        rconPort = 25575
        rconPassword = "secret"
        backup = true
        enabled = true
        "#,
            restic_env.display()
        ));

        validate_config(&config).expect("backup config should be valid");
        assert_eq!(
            config.backup.local_dir.as_deref(),
            Some(Path::new("/srv/backups"))
        );
        assert_eq!(config.servers["survival"].enabled, Some(true));
    }

    #[test]
    fn reports_restic_environment_check_success_when_backup_is_enabled() {
        let restic_env = write_test_restic_env(
            "success-message",
            r#"
            RESTIC_REPOSITORY='s3:s3.example.com/bucket'
            RESTIC_PASSWORD_FILE='/secure/restic.pwd'
            "#,
        );
        let config = parse_config(&format!(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [backup]
            localDir = "/srv/backups"
            resticEnvFile = "{}"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            backup = true
            "#,
            restic_env.display()
        ));

        validate_config(&config).expect("backup config should be valid");

        let message = restic_environment_check_success_message(&config)
            .expect("backup-enabled config should report restic check success");
        assert!(message.contains("Remote backup Restic environment check valid"));
        assert!(message.contains(&restic_env.display().to_string()));
    }

    #[test]
    fn skips_restic_environment_check_success_when_no_backup_is_enabled() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            "#,
        );

        validate_config(&config).expect("config should be valid");

        assert!(restic_environment_check_success_message(&config).is_none());
    }

    #[test]
    fn parses_backup_config() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [backup]
            localDir = "/srv/backups"
            resticEnvFile = "/home/blackshadow/.config/clserver/secrets/restic.env"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            "#,
        );

        validate_config(&config).expect("backup config should be valid");
        assert_eq!(
            config.backup.local_dir.as_deref(),
            Some(Path::new("/srv/backups"))
        );
        assert_eq!(
            config.backup.restic_env_file.as_deref(),
            Some(Path::new(
                "/home/blackshadow/.config/clserver/secrets/restic.env"
            ))
        );
    }

    #[test]
    fn rejects_restic_env_file_missing_repository_when_backup_is_enabled() {
        let restic_env = write_test_restic_env(
            "missing-repository",
            r#"
            RESTIC_PASSWORD_FILE='/secure/restic.pwd'
            "#,
        );
        let config = parse_config(&format!(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [backup]
            localDir = "/srv/backups"
            resticEnvFile = "{}"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            backup = true
            "#,
            restic_env.display()
        ));

        let error = validate_config(&config).expect_err("missing restic repository should fail");
        assert!(error.to_string().contains("RESTIC_REPOSITORY"));
    }

    #[test]
    fn rejects_restic_env_file_missing_password_when_backup_is_enabled() {
        let restic_env = write_test_restic_env(
            "missing-password",
            r#"
            RESTIC_REPOSITORY='s3:s3.example.com/bucket'
            "#,
        );
        let config = parse_config(&format!(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [backup]
            localDir = "/srv/backups"
            resticEnvFile = "{}"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            backup = true
            "#,
            restic_env.display()
        ));

        let error = validate_config(&config).expect_err("missing restic password should fail");
        assert!(error.to_string().contains("RESTIC_PASSWORD"));
    }

    #[test]
    fn parses_restore_mode_and_defaults_missing_restore_to_world() {
        let config = parse_config(
            r#"
        [global]
        serverDir = "/srv/servers"
        logDir = "/var/log/clserver"

        [java_environments]
        default = "/usr/bin/java"

        [servers.survival]
        name = "survival"
        type = "minecraft"
        jarFile = "server.jar"
        rconPort = 25575
        rconPassword = "secret"

        [servers.creative]
        name = "creative"
        type = "minecraft"
        jarFile = "server.jar"
        rconPort = 25576
        rconPassword = "secret"
        restore = "all"
        "#,
        );

        validate_config(&config).expect("restore config should be valid");
        assert_eq!(
            config.servers["survival"].restore.unwrap_or_default(),
            RestoreMode::World
        );
        assert_eq!(config.servers["creative"].restore, Some(RestoreMode::All));
    }

    #[test]
    fn rejects_missing_local_backup_dir_when_backup_is_enabled() {
        let config = parse_config(
            r#"
        [global]
        serverDir = "/srv/servers"
        logDir = "/var/log/clserver"

        [java_environments]
        default = "/usr/bin/java"

        [servers.survival]
        name = "survival"
        type = "minecraft"
        jarFile = "server.jar"
        rconPort = 25575
        rconPassword = "secret"
        backup = true
        "#,
        );

        let error = validate_config(&config).expect_err("missing localDir should be invalid");
        assert!(error.to_string().contains("backup.localDir is required"));
    }

    #[test]
    fn validates_complete_velocity_config() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.velocity]
            name = "velocity"
            type = "velocity"
            jarFile = "velocity.jar"
            stopCommand = "end"
            "#,
        );

        validate_config(&config).expect("config should be valid");
    }

    #[test]
    fn rejects_missing_global_fields_and_servers() {
        let config = parse_config("");

        let error = validate_config(&config).expect_err("config should be invalid");
        let message = error.to_string();

        assert!(message.contains("global.serverDir is required"));
        assert!(message.contains("global.logDir is required"));
        assert!(message.contains("at least one server must be configured"));
    }

    #[test]
    fn rejects_missing_minecraft_rcon_config() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            jarFile = "server.jar"
            "#,
        );

        let error = validate_config(&config).expect_err("config should be invalid");
        let message = error.to_string();

        assert!(message.contains("no rconPort"));
        assert!(message.contains("no rconPassword"));
    }

    #[test]
    fn rejects_unknown_java_version() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.survival]
            name = "survival"
            type = "minecraft"
            javaVersion = "java99"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            "#,
        );

        let error = validate_config(&config).expect_err("config should be invalid");
        assert!(
            error
                .to_string()
                .contains("references unknown javaVersion 'java99'")
        );
    }

    #[test]
    fn parses_rcon_password_from_server_properties() {
        let password = parse_server_properties_rcon_password(
            r#"
            #Minecraft server properties
            enable-rcon=true
            rcon.port=25575
            rcon.password=from-properties
            "#,
        );

        assert_eq!(password.as_deref(), Some("from-properties"));
    }

    #[test]
    fn parses_rcon_password_with_colon_separator() {
        let password = parse_server_properties_rcon_password("rcon.password: from-properties");

        assert_eq!(password.as_deref(), Some("from-properties"));
    }

    #[test]
    fn ignores_comments_when_parsing_rcon_password() {
        let password = parse_server_properties_rcon_password(
            r#"
            # rcon.password=commented-out
            ! rcon.password=also-commented-out
            motd=A Minecraft Server
            "#,
        );

        assert_eq!(password, None);
    }

    #[test]
    fn formats_rcon_password_fix_as_literal_string() -> Result<()> {
        let item = literal_string_item(r#"abc\def$%^"#)?;

        assert_eq!(item.to_string(), r#"'abc\def$%^'"#);
        Ok(())
    }

    #[test]
    fn rejects_literal_string_format_for_password_with_single_quote() {
        let error = literal_string_item("abc'def")
            .expect_err("single quotes cannot be represented in single-line literal strings");

        assert!(error.to_string().contains("single quote"));
    }

    #[test]
    fn allows_server_table_key_as_shortcut_id() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.CLS4]
            name = "CatLordSurvival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"
            "#,
        );

        validate_config(&config).expect("shortcut id should be valid");
    }

    #[test]
    fn rejects_duplicate_server_names() {
        let config = parse_config(
            r#"
            [global]
            serverDir = "/srv/servers"
            logDir = "/var/log/clserver"

            [java_environments]
            default = "/usr/bin/java"

            [servers.CL-S]
            name = "CatLordSurvival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25575
            rconPassword = "secret"

            [servers.survival]
            name = "CatLordSurvival"
            type = "minecraft"
            jarFile = "server.jar"
            rconPort = 25576
            rconPassword = "secret"
            "#,
        );

        let error = validate_config(&config).expect_err("duplicate server names should be invalid");
        assert!(
            error
                .to_string()
                .contains("both use name 'CatLordSurvival'")
        );
    }
}
