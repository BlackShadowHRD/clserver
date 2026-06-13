use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};

use crate::config::Config;

const WEBHOOK_URL_KEY: &str = "CLSERVER_DISCORD_WEBHOOK_URL";
const DISCORD_CONTENT_LIMIT: usize = 2000;
const DISCORD_TRUNCATED_CONTENT_LIMIT: usize = 1900;

pub fn send_maintenance_summary(
    config: &Config,
    result: &Result<()>,
    duration: Duration,
) -> Result<bool> {
    if !notifications_enabled(config) {
        return Ok(false);
    }

    let Some(env_file) = config.notifications.discord.webhook_env_file.as_deref() else {
        bail!("notifications.discord.webhookEnvFile is required when notifications are enabled");
    };

    let webhook_url = discord_webhook_url(env_file)?;
    let content = truncate_discord_content(&maintenance_message(config, result, duration));
    send_discord_webhook(&webhook_url, &content)?;
    Ok(true)
}

fn notifications_enabled(config: &Config) -> bool {
    config.notifications.enabled.unwrap_or(false)
        && config.notifications.maintenance_summary.unwrap_or(true)
}

fn maintenance_message(config: &Config, result: &Result<()>, duration: Duration) -> String {
    let total_servers = config.servers.len();
    let backup_enabled_servers = config
        .servers
        .values()
        .filter(|server| server.backup.unwrap_or(false))
        .count();
    let enabled_servers = config
        .servers
        .values()
        .filter(|server| server.enabled.unwrap_or(false))
        .count();
    let duration = format_duration(duration);

    match result {
        Ok(()) => format!(
            "✅ clserver maintenance completed\n\nServers: {total_servers} configured\nBackups: {backup_enabled_servers} enabled\nEnabled for restart: {enabled_servers}\nDuration: {duration}"
        ),
        Err(err) => format!(
            "❌ clserver maintenance failed\n\nServers: {total_servers} configured\nBackups: {backup_enabled_servers} enabled\nEnabled for restart: {enabled_servers}\nDuration: {duration}\n\nError:\n{err:#}"
        ),
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn discord_webhook_url(env_file: &Path) -> Result<String> {
    let entries = load_env_file(env_file)?;
    entries
        .into_iter()
        .rev()
        .find_map(|(key, value)| (key == WEBHOOK_URL_KEY).then_some(value))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "Discord webhook env file '{}' must define {WEBHOOK_URL_KEY}",
                env_file.display()
            )
        })
}

fn send_discord_webhook(webhook_url: &str, content: &str) -> Result<()> {
    let payload = format!("{{\"content\":\"{}\"}}", escape_json_string(content));
    ureq::post(webhook_url)
        .set("Content-Type", "application/json")
        .send_string(&payload)
        .map(|_| ())
        .map_err(|err| anyhow!("Failed to send Discord webhook: {err}"))
}

fn load_env_file(path: &Path) -> Result<Vec<(String, String)>> {
    let text = fs::read_to_string(path).with_context(|| {
        format!(
            "Failed to read Discord webhook env file '{}'",
            path.display()
        )
    })?;

    text.lines()
        .enumerate()
        .filter_map(|(index, line)| parse_env_line(path, index + 1, line).transpose())
        .collect()
}

fn parse_env_line(path: &Path, line_number: usize, line: &str) -> Result<Option<(String, String)>> {
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

    Ok(Some((key.to_string(), parse_env_value(value.trim()))))
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn parse_env_value(value: &str) -> String {
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

fn truncate_discord_content(content: &str) -> String {
    if content.chars().count() <= DISCORD_CONTENT_LIMIT {
        return content.to_string();
    }

    let mut truncated = content
        .chars()
        .take(DISCORD_TRUNCATED_CONTENT_LIMIT)
        .collect::<String>();
    truncated.push_str("\n\n… truncated; check clserver.log for full details");
    truncated
}

fn escape_json_string(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_maintenance_success_message() {
        let config = Config::default();
        let message = maintenance_message(&config, &Ok(()), Duration::from_secs(75));

        assert!(message.contains("clserver maintenance completed"));
        assert!(message.contains("Duration: 1m 15s"));
    }

    #[test]
    fn reports_summary_skipped_when_notifications_are_disabled() -> Result<()> {
        let config = Config::default();

        let sent = send_maintenance_summary(&config, &Ok(()), Duration::from_secs(1))?;

        assert!(!sent);
        Ok(())
    }

    #[test]
    fn reports_enabled_summary_configuration_errors() {
        let mut config = Config::default();
        config.notifications.enabled = Some(true);

        let error = send_maintenance_summary(&config, &Ok(()), Duration::from_secs(1))
            .expect_err("enabled notifications without a webhook env file should fail");

        assert!(
            error
                .to_string()
                .contains("notifications.discord.webhookEnvFile is required")
        );
    }

    #[test]
    fn parses_discord_env_lines() -> Result<()> {
        let path = Path::new("discord.env");

        assert_eq!(
            parse_env_line(
                path,
                1,
                "CLSERVER_DISCORD_WEBHOOK_URL='https://discord.com/api/webhooks/test'"
            )?,
            Some((
                WEBHOOK_URL_KEY.to_string(),
                "https://discord.com/api/webhooks/test".to_string()
            ))
        );
        assert_eq!(parse_env_line(path, 2, "# comment")?, None);
        Ok(())
    }

    #[test]
    fn truncates_long_discord_content() {
        let content = "x".repeat(2100);
        let truncated = truncate_discord_content(&content);

        assert!(truncated.chars().count() <= DISCORD_CONTENT_LIMIT);
        assert!(truncated.contains("truncated"));
    }

    #[test]
    fn escapes_json_message_content() {
        assert_eq!(
            escape_json_string("hello \"world\"\nnext"),
            "hello \\\"world\\\"\\nnext"
        );
    }
}
