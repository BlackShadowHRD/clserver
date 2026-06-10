use anyhow::Result;
use std::collections::HashMap;

use crate::config::{GlobalConfig, ServerConfig};

use super::manager::ServerManager;

pub struct GenericServer {
    pub manager: ServerManager,
}

impl GenericServer {
    pub fn new(
        config: ServerConfig,
        global: &GlobalConfig,
        java_environments: &HashMap<String, String>,
    ) -> Result<Self> {
        let manager = ServerManager::new(config, global, java_environments)?;
        Ok(Self { manager })
    }
}
