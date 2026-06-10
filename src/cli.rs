use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "clserver",
    version,
    about = "Manage CatLord Minecraft, Hytale and Velocity servers"
)]
struct Cli {
    /// Enable verbose logging for debugging details such as generated start commands
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the named server
    Start { server: String },

    /// Stop the named server
    Stop {
        server: String,

        /// Stop behavior to use
        #[arg(value_enum, ignore_case = true, default_value_t = StopType::Immediate)]
        stop_type: StopType,
    },

    /// Backup the named server
    Backup { server: String },

    /// Restore the named server from its configured backup
    Restore { server: String },

    /// Restart the named server
    Restart { server: String },

    /// Run daily maintenance across configured servers
    Maintenance,

    /// Attach to the named server's screen session
    Attach { server: String },

    /// Show status information for the named server
    Status { server: String },

    /// List all configured servers and whether their screen sessions are running
    List,

    /// Validate the configuration file and exit
    ValidateConfig {
        /// Offer to update mismatched Minecraft RCON passwords in clserver.toml
        #[arg(long)]
        fix: bool,
    },
}

#[derive(Debug)]
pub enum Action {
    Start,
    Stop { stop_type: StopType },
    Backup,
    Restore,
    Restart,
    Maintenance,
    Attach,
    Status,
    List,
    ValidateConfig { fix: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StopType {
    Immediate,
    Friendly,
}

#[derive(Debug)]
pub struct Request {
    pub action: Action,
    pub server: Option<String>,
    pub verbose: bool,
}

pub fn parse_request() -> Result<Request> {
    match parse_request_from(std::env::args_os()) {
        Ok(request) => Ok(request),
        Err(error) => error.exit(),
    }
}

fn parse_request_from<I, T>(args: I) -> std::result::Result<Request, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;

    let (action, server) = match cli.command {
        Commands::Start { server } => (Action::Start, Some(server)),
        Commands::Stop { server, stop_type } => (Action::Stop { stop_type }, Some(server)),
        Commands::Backup { server } => (Action::Backup, Some(server)),
        Commands::Restore { server } => (Action::Restore, Some(server)),
        Commands::Restart { server } => (Action::Restart, Some(server)),
        Commands::Maintenance => (Action::Maintenance, None),
        Commands::Attach { server } => (Action::Attach, Some(server)),
        Commands::Status { server } => (Action::Status, Some(server)),
        Commands::List => (Action::List, None),
        Commands::ValidateConfig { fix } => (Action::ValidateConfig { fix }, None),
    };

    let request = Request {
        action,
        server,
        verbose: cli.verbose,
    };

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn parses_version_flag() {
        let error = Cli::try_parse_from(["clserver", "--version"])
            .expect_err("version flag should short-circuit parsing");

        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn parses_short_version_flag() {
        let error = Cli::try_parse_from(["clserver", "-V"])
            .expect_err("short version flag should short-circuit parsing");

        assert_eq!(error.kind(), ErrorKind::DisplayVersion);
        assert!(error.to_string().contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn parses_start_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "start", "survival"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server.as_deref(), Some("survival"));
        assert!(!request.verbose);
        Ok(())
    }

    #[test]
    fn parses_verbose_flag_before_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "--verbose", "start", "survival"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server.as_deref(), Some("survival"));
        assert!(request.verbose);
        Ok(())
    }

    #[test]
    fn parses_verbose_flag_after_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "start", "survival", "--verbose"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server.as_deref(), Some("survival"));
        assert!(request.verbose);
        Ok(())
    }

    #[test]
    fn parses_stop_subcommand_with_default_stop_type() -> Result<()> {
        let request = parse_request_from(["clserver", "stop", "survival"])?;

        assert!(matches!(
            request.action,
            Action::Stop {
                stop_type: StopType::Immediate
            }
        ));
        assert_eq!(request.server.as_deref(), Some("survival"));
        Ok(())
    }

    #[test]
    fn parses_stop_subcommand_with_friendly_stop_type() -> Result<()> {
        let request = parse_request_from(["clserver", "stop", "survival", "friendly"])?;

        assert!(matches!(
            request.action,
            Action::Stop {
                stop_type: StopType::Friendly
            }
        ));
        assert_eq!(request.server.as_deref(), Some("survival"));
        Ok(())
    }

    #[test]
    fn parses_stop_subcommand_case_insensitively() -> Result<()> {
        let request = parse_request_from(["clserver", "stop", "survival", "FRIENDLY"])?;

        assert!(matches!(
            request.action,
            Action::Stop {
                stop_type: StopType::Friendly
            }
        ));

        let request = parse_request_from(["clserver", "stop", "survival", "ImMeDiAtE"])?;

        assert!(matches!(
            request.action,
            Action::Stop {
                stop_type: StopType::Immediate
            }
        ));
        Ok(())
    }

    #[test]
    fn parses_list_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "list"])?;

        assert!(matches!(request.action, Action::List));
        assert_eq!(request.server, None);
        Ok(())
    }

    #[test]
    fn parses_maintenance_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "maintenance"])?;

        assert!(matches!(request.action, Action::Maintenance));
        assert_eq!(request.server, None);
        Ok(())
    }

    #[test]
    fn parses_restore_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "restore", "survival"])?;

        assert!(matches!(request.action, Action::Restore));
        assert_eq!(request.server.as_deref(), Some("survival"));
        Ok(())
    }

    #[test]
    fn parses_validate_config_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "validate-config"])?;

        assert!(matches!(
            request.action,
            Action::ValidateConfig { fix: false }
        ));
        assert_eq!(request.server, None);
        Ok(())
    }

    #[test]
    fn parses_validate_config_fix_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "validate-config", "--fix"])?;

        assert!(matches!(
            request.action,
            Action::ValidateConfig { fix: true }
        ));
        assert_eq!(request.server, None);
        Ok(())
    }

    #[test]
    fn rejects_unknown_stop_type() {
        let error = parse_request_from(["clserver", "stop", "survival", "invalid"])
            .expect_err("invalid stop type should fail");

        assert!(error.to_string().contains("invalid"));
    }
}
