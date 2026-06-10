use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "clServer",
    about = "Manage CatLord Minecraft and Velocity servers"
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

    /// Restart the named server
    Restart { server: String },

    /// Attach to the named server's screen session
    Attach { server: String },

    /// Show status information for the named server
    Status { server: String },
}

#[derive(Debug)]
pub enum Action {
    Start,
    Stop { stop_type: StopType },
    Backup,
    Restart,
    Attach,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StopType {
    Immediate,
    Friendly,
}

#[derive(Debug)]
pub struct Request {
    pub action: Action,
    pub server: String,
    pub verbose: bool,
}

pub fn parse_request() -> Result<Request> {
    parse_request_from(std::env::args_os())
}

fn parse_request_from<I, T>(args: I) -> Result<Request>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;

    let (action, server) = match cli.command {
        Commands::Start { server } => (Action::Start, server),
        Commands::Stop { server, stop_type } => (Action::Stop { stop_type }, server),
        Commands::Backup { server } => (Action::Backup, server),
        Commands::Restart { server } => (Action::Restart, server),
        Commands::Attach { server } => (Action::Attach, server),
        Commands::Status { server } => (Action::Status, server),
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

    #[test]
    fn parses_start_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "start", "survival"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server, "survival");
        assert!(!request.verbose);
        Ok(())
    }

    #[test]
    fn parses_verbose_flag_before_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "--verbose", "start", "survival"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server, "survival");
        assert!(request.verbose);
        Ok(())
    }

    #[test]
    fn parses_verbose_flag_after_subcommand() -> Result<()> {
        let request = parse_request_from(["clserver", "start", "survival", "--verbose"])?;

        assert!(matches!(request.action, Action::Start));
        assert_eq!(request.server, "survival");
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
        assert_eq!(request.server, "survival");
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
        assert_eq!(request.server, "survival");
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
    fn rejects_unknown_stop_type() {
        let error = parse_request_from(["clserver", "stop", "survival", "invalid"])
            .expect_err("invalid stop type should fail");

        assert!(error.to_string().contains("invalid"));
    }
}
