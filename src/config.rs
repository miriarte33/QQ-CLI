use anyhow::{Context, Result};
use dirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub jira_url: String,
    pub username: String,
    pub api_token: String,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
}

impl Config {
    pub fn new(jira_url: String, username: String, api_token: String) -> Self {
        Self {
            jira_url,
            username,
            api_token,
            google_client_id: None,
            google_client_secret: None,
        }
    }
    
    pub fn set_google_credentials(&mut self, client_id: String, client_secret: String) {
        self.google_client_id = Some(client_id);
        self.google_client_secret = Some(client_secret);
    }
    
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }
        
        let toml_string = toml::to_string_pretty(&self)?;
        fs::write(&config_path, toml_string).context("Failed to write config file")?;
        
        Ok(())
    }
    
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        let contents = fs::read_to_string(&config_path)
            .context("Failed to read config file. Please run 'jira-git-cli config' first.")?;
        
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
    
    fn config_path() -> Result<PathBuf> {
        let home_dir = dirs::config_dir()
            .context("Failed to determine config directory")?;
        Ok(home_dir.join("qq").join("config.toml"))
    }
    
    pub fn google_token_path() -> Result<PathBuf> {
        let home_dir = dirs::config_dir()
            .context("Failed to determine config directory")?;
        Ok(home_dir.join("qq").join("google_tokens.json"))
    }
}