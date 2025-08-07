use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod config;
mod jira;
mod ui;
mod google;

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
    
    #[command(about = "Google Calendar meetings commands")]
    Meetings {
        #[command(subcommand)]
        command: MeetingsCommands,
    },
    
    #[command(about = "Configure qq settings")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
enum GetSubcommands {
    #[command(about = "Show ticket details (default)")]
    Info,
    
    #[command(about = "Show parent epic of the ticket")]
    Parent,
}

#[derive(Subcommand)]
enum JiraCommands {
    #[command(about = "Get ticket information from current git branch")]
    Get {
        #[command(subcommand)]
        subcommand: Option<GetSubcommands>,
    },
    
    #[command(about = "Add a comment to the ticket from current git branch")]
    Comment {
        #[arg(help = "Comment text to add")]
        message: String,
    },
    
    #[command(about = "Close the ticket from current git branch")]
    Close,
    
    #[command(about = "Create a feature branch for a JIRA ticket, assign it to yourself, and move to In Progress")]
    Start {
        #[arg(help = "JIRA ticket number (e.g., PROJ-123)")]
        ticket: String,
    },
    
    #[command(about = "List all tickets in an epic with interactive controls")]
    Epic {
        #[arg(help = "Epic ticket number (e.g., EPIC-123) or 'list' to show all epics")]
        ticket: String,
    },
    
    #[command(about = "List all tickets assigned to me")]
    Mine,
}

#[derive(Subcommand)]
enum MeetingsCommands {
    #[command(about = "List today's meetings from Google Calendar")]
    List,
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
    
    #[command(about = "Configure Google Calendar settings")]
    Google {
        #[arg(long, help = "Google OAuth2 Client ID")]
        client_id: String,
        
        #[arg(long, help = "Google OAuth2 Client Secret")]
        client_secret: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Jira { command } => {
            handle_jira_command(command)?;
        }
        
        Commands::Meetings { command } => {
            handle_meetings_command(command)?;
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
        ConfigCommands::Google { client_id, client_secret } => {
            let mut config = Config::load().unwrap_or_else(|_| Config::new(
                String::new(),
                String::new(),
                String::new(),
            ));
            config.set_google_credentials(client_id, client_secret);
            config.save()?;
            println!("Google Calendar configuration saved successfully!");
        }
    }
    
    Ok(())
}

fn handle_meetings_command(command: MeetingsCommands) -> Result<()> {
    use ui::MeetingsListDisplay;
    
    match command {
        MeetingsCommands::List => {
            let config = Config::load()?;
            
            let client_id = config.google_client_id
                .context("Google client ID not configured. Run 'qq config google' first.")?;
            let client_secret = config.google_client_secret
                .context("Google client secret not configured. Run 'qq config google' first.")?;
            
            let token_path = Config::google_token_path()?;
            
            println!("Fetching meetings from Google Calendar...");
            let meetings = google::blocking_list_meetings(client_id, client_secret, token_path)?;
            
            if meetings.is_empty() {
                println!("No meetings scheduled for the next 7 days.");
            } else {
                println!("Found {} meeting(s).", meetings.len());
                MeetingsListDisplay::show(meetings)?;
            }
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
        JiraCommands::Get { subcommand } => {
            let branch = get_current_branch()?;
            let ticket_id = extract_ticket_id(&branch)?;
            
            match subcommand.as_ref().unwrap_or(&GetSubcommands::Info) {
                GetSubcommands::Info => {
                    println!("Fetching details for ticket: {}", ticket_id);
                    let issue = client.get_issue(&ticket_id)?;
                    
                    // Use the new Ratatui UI to display the issue
                    JiraIssueDisplay::show(&issue)?;
                }
                GetSubcommands::Parent => {
                    use ui::EpicListDisplay;
                    
                    println!("Fetching parent epic for ticket: {}", ticket_id);
                    let issue = client.get_issue_with_parent(&ticket_id)?;
                    
                    if let Some(parent) = &issue.fields.parent {
                        println!("Found parent epic: {}", parent.key);
                        
                        println!("Fetching child issues...");
                        let children = client.get_epic_children(&parent.key)?;
                        
                        // Display the epic and its children in interactive UI
                        EpicListDisplay::show(parent, children, &client)?;
                    } else {
                        println!("This issue is not part of an epic.");
                    }
                }
            }
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
        
        JiraCommands::Start { ticket } => {
            // Create the feature branch
            let branch_name = format!("feature/{}", ticket);
            
            // Open the git repository
            let repo = Repository::open(".").context("Failed to open git repository")?;
            
            // Get the current HEAD commit
            let head = repo.head().context("Failed to get HEAD reference")?;
            let target = head.target().context("Failed to get HEAD target")?;
            let commit = repo.find_commit(target).context("Failed to find HEAD commit")?;
            
            // Create the new branch
            repo.branch(&branch_name, &commit, false)
                .context(format!("Failed to create branch '{}'", branch_name))?;
            
            // Checkout the new branch
            let obj = repo.revparse_single(&format!("refs/heads/{}", branch_name))
                .context("Failed to find new branch")?;
            
            repo.checkout_tree(&obj, None)
                .context("Failed to checkout new branch")?;
            
            repo.set_head(&format!("refs/heads/{}", branch_name))
                .context("Failed to set HEAD to new branch")?;
            
            println!("Created and switched to branch: {}", branch_name);
            
            // Now assign the ticket to yourself and move it to In Progress
            println!("Picking up ticket: {}", ticket);
            client.pickup_issue(&ticket)?;
            println!("Ticket assigned to you and moved to In Progress!");
        }
        
        JiraCommands::Epic { ticket } => {
            use ui::{EpicListDisplay, AllEpicsDisplay};
            
            if ticket == "list" {
                // Show all epics
                println!("Fetching all epics...");
                
                let epics = client.get_all_epics()?;
                
                if epics.is_empty() {
                    println!("No epics found.");
                } else {
                    println!("Found {} epic(s).", epics.len());
                    // Display all epics in interactive UI
                    AllEpicsDisplay::show(epics, &client)?;
                }
            } else {
                // Show specific epic
                println!("Fetching epic details for: {}", ticket);
                let epic = client.get_issue(&ticket)?;
                
                println!("Fetching child issues...");
                let children = client.get_epic_children(&ticket)?;
                
                // Display the epic and its children in interactive UI
                EpicListDisplay::show(&epic, children, &client)?;
            }
        }
        
        JiraCommands::Mine => {
            use ui::MyIssuesDisplay;
            
            println!("Fetching issues assigned to you...");
            let issues = client.get_my_issues()?;
            
            if issues.is_empty() {
                println!("No issues currently assigned to you.");
            } else {
                println!("Found {} issue(s) assigned to you.", issues.len());
                // Display the issues in interactive UI
                MyIssuesDisplay::show(issues, &client)?;
            }
        }
    }
    
    Ok(())
}