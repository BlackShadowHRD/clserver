use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::thread::sleep;
use std::time::Duration;

use crate::cli::StopType;
use crate::config::{GlobalConfig, ServerConfig};
use crate::rcon::RconClient;
use tracing::{info, warn};

use super::manager::ServerManager;

pub struct MinecraftServer {
    pub manager: ServerManager,
    rcon_port: u16,
    rcon_password: String,
}

impl MinecraftServer {
    pub fn new(
        config: ServerConfig,
        global: &GlobalConfig,
        java_environments: &HashMap<String, String>,
    ) -> Result<Self> {
        let rcon_port = config
            .rcon_port
            .ok_or_else(|| anyhow!("Minecraft server '{}' has no rconPort.", config.name))?;
        let rcon_password = config
            .rcon_password
            .clone()
            .ok_or_else(|| anyhow!("Minecraft server '{}' has no rconPassword.", config.name))?;
        let manager = ServerManager::new(config, global, java_environments)?;

        Ok(Self {
            manager,
            rcon_port,
            rcon_password,
        })
    }

    pub fn start_server(&self) -> Result<()> {
        self.manager.start_server()
    }

    pub fn stop_server(&self, stop_type: StopType) -> Result<()> {
        match stop_type {
            StopType::Friendly => self.friendly_stop(),
            StopType::Immediate => self.immediate_stop(),
        }
    }

    fn friendly_stop(&self) -> Result<()> {
        if !self.is_up_server() {
            warn!(server = %self.manager.config.name, "server appears not to be running");
            return Ok(());
        }

        let response = self.rcon_command("list")?;
        let player_count = parse_player_count(&response).unwrap_or(0);

        info!(
            server = %self.manager.config.name,
            player_count,
            "minecraft friendly shutdown started"
        );

        if player_count > 0 {
            self.rcon_command("msg @a Server is shutting down in 5 minutes")?;
            sleep(Duration::from_secs(240));
            self.rcon_command("msg @a Server is shutting down in 1 minute")?;
            sleep(Duration::from_secs(60));
        } else {
            info!(
                server = %self.manager.config.name,
                "no players online; performing immediate shutdown instead"
            );
        }

        self.immediate_stop()
    }

    fn immediate_stop(&self) -> Result<()> {
        if self.is_up_server() {
            self.rcon_command("stop")?;
            info!(server = %self.manager.config.name, "minecraft server stopping immediately");
        } else {
            warn!(server = %self.manager.config.name, "server appears not to be running");
        }

        Ok(())
    }

    pub fn restart_server(&self) -> Result<()> {
        self.stop_server(StopType::Immediate)?;
        sleep(Duration::from_secs(10));
        self.start_server()
    }

    fn is_up_server(&self) -> bool {
        self.rcon_command("list").is_ok()
    }

    fn rcon_command(&self, command: &str) -> Result<String> {
        let mut client = RconClient::connect("127.0.0.1", self.rcon_port)?;
        client.login(&self.rcon_password)?;
        client.command(command)
    }
}

fn parse_player_count(response: &str) -> Option<u32> {
    response
        .split_whitespace()
        .find_map(|part| part.parse::<u32>().ok())
}
