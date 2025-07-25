use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod config;
mod jira;
mod ui;

use config::Config;

#[derive(Parser)]
#[command(name = "qq")]
#[command(author, version, about = "Personal CLI for day-to-day tasks", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "JIRA integration commands")]
    Jira {
        #[command(subcommand)]
        command: JiraCommands,
    },
    
    #[command(about = "Configure qq settings")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum JiraCommands {
    #[command(about = "Get ticket description from current git branch")]
    Get,
    
    #[command(about = "Add a comment to the ticket from current git branch")]
    Comment {
        #[arg(help = "Comment text to add")]
        message: String,
    },
    
    #[command(about = "Close the ticket from current git branch")]
    Close,
}

#[derive(Subcommand)]
enum ConfigCommands {
    #[command(about = "Configure JIRA settings")]
    Jira {
        #[arg(long, help = "JIRA instance URL (e.g., https://company.atlassian.net)")]
        url: String,
        
        #[arg(long, help = "JIRA username/email")]
        username: String,
        
        #[arg(long, help = "JIRA API token")]
        token: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Jira { command } => {
            handle_jira_command(command)?;
        }
        
        Commands::Config { command } => {
            handle_config_command(command)?;
        }
    }
    
    Ok(())
}

fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Jira { url, username, token } => {
            let config = Config::new(url, username, token);
            config.save()?;
            println!("JIRA configuration saved successfully!");
        }
    }
    
    Ok(())
}

fn handle_jira_command(command: JiraCommands) -> Result<()> {
    use git2::Repository;
    use regex::Regex;
    use jira::JiraClient;
    use ui::JiraIssueDisplay;
    
    // Helper functions for JIRA commands
    fn get_current_branch() -> Result<String> {
        let repo = Repository::open(".").context("Failed to open git repository")?;
        let head = repo.head().context("Failed to get HEAD reference")?;
        let branch = head.shorthand().unwrap_or("HEAD");
        Ok(branch.to_string())
    }
    
    fn extract_ticket_id(branch_name: &str) -> Result<String> {
        let patterns = vec![
            r"^([A-Z]+-\d+)",
            r"([A-Z]+-\d+)",
            r"^feature/([A-Z]+-\d+)",
            r"^bugfix/([A-Z]+-\d+)",
            r"^hotfix/([A-Z]+-\d+)",
        ];
        
        for pattern in patterns {
            let re = Regex::new(pattern)?;
            if let Some(captures) = re.captures(branch_name) {
                if let Some(ticket_id) = captures.get(1) {
                    return Ok(ticket_id.as_str().to_string());
                }
            }
        }
        
        anyhow::bail!("No JIRA ticket ID found in branch name: {}", branch_name)
    }
    
    let config = Config::load()?;
    let client = JiraClient::new(config);
    
    match command {
        JiraCommands::Get => {
            let branch = get_current_branch()?;
            let ticket_id = extract_ticket_id(&branch)?;
            
            println!("Fetching details for ticket: {}", ticket_id);
            let issue = client.get_issue(&ticket_id)?;
            
            // Use the new Ratatui UI to display the issue
            JiraIssueDisplay::show(&issue)?;
        }
        
        JiraCommands::Comment { message } => {
            let branch = get_current_branch()?;
            let ticket_id = extract_ticket_id(&branch)?;
            
            println!("Adding comment to ticket: {}", ticket_id);
            client.add_comment(&ticket_id, &message)?;
            println!("Comment added successfully!");
        }
        
        JiraCommands::Close => {
            let branch = get_current_branch()?;
            let ticket_id = extract_ticket_id(&branch)?;
            
            println!("Closing ticket: {}", ticket_id);
            client.close_issue(&ticket_id)?;
            println!("Ticket closed successfully!");
        }
    }
    
    Ok(())
}