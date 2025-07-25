use anyhow::{Context, Result};
use base64::Engine;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json;

use crate::config::Config;

#[derive(Debug, Serialize, Deserialize)]
pub struct JiraIssue {
    pub key: String,
    pub fields: IssueFields,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IssueFields {
    pub summary: String,
    #[serde(default)]
    pub description: Option<serde_json::Value>,
    pub status: Status,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
}

#[derive(Debug, Serialize)]
struct CommentRequest {
    body: CommentBody,
}

#[derive(Debug, Serialize)]
struct CommentBody {
    #[serde(rename = "type")]
    doc_type: String,
    version: i32,
    content: Vec<CommentContent>,
}

#[derive(Debug, Serialize)]
struct CommentContent {
    #[serde(rename = "type")]
    content_type: String,
    content: Vec<CommentText>,
}

#[derive(Debug, Serialize)]
struct CommentText {
    #[serde(rename = "type")]
    text_type: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct TransitionRequest {
    transition: TransitionId,
}

#[derive(Debug, Serialize)]
struct TransitionId {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TransitionsResponse {
    transitions: Vec<Transition>,
}

#[derive(Debug, Deserialize)]
struct Transition {
    id: String,
    name: String,
}

pub struct JiraClient {
    client: Client,
    base_url: String,
    auth_header: String,
}

impl JiraClient {
    pub fn new(config: Config) -> Self {
        let client = Client::new();
        let auth = format!("{}:{}", config.username, config.api_token);
        let auth_header = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(auth)
        );
        
        Self {
            client,
            base_url: config.jira_url.trim_end_matches('/').to_string(),
            auth_header,
        }
    }
    
    pub fn get_issue(&self, issue_key: &str) -> Result<JiraIssue> {
        let url = format!("{}/rest/api/3/issue/{}", self.base_url, issue_key);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .send()
            .context("Failed to send request to JIRA")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("JIRA API error: {}", status);
        }
        
        let issue: JiraIssue = serde_json::from_str(&response_text)
            .context(format!("Failed to parse JIRA response. Response: {}", response_text))?;
        
        Ok(issue)
    }
    
    pub fn add_comment(&self, issue_key: &str, comment: &str) -> Result<()> {
        let url = format!("{}/rest/api/3/issue/{}/comment", self.base_url, issue_key);
        
        let comment_request = CommentRequest {
            body: CommentBody {
                doc_type: "doc".to_string(),
                version: 1,
                content: vec![CommentContent {
                    content_type: "paragraph".to_string(),
                    content: vec![CommentText {
                        text_type: "text".to_string(),
                        text: comment.to_string(),
                    }],
                }],
            },
        };
        
        let response = self.client
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&comment_request)
            .send()
            .context("Failed to send comment to JIRA")?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().unwrap_or_else(|_| "Unable to read response".to_string());
            eprintln!("Failed to add comment. Status: {}, Response: {}", status, error_text);
            anyhow::bail!("Failed to add comment: {}", status);
        }
        
        Ok(())
    }
    
    pub fn close_issue(&self, issue_key: &str) -> Result<()> {
        let transitions = self.get_transitions(issue_key)?;
        
        let done_transition = transitions.iter()
            .find(|t| t.name.to_lowercase().contains("done") || 
                      t.name.to_lowercase().contains("close") ||
                      t.name.to_lowercase().contains("resolved"))
            .context("No 'Done' or 'Close' transition found for this issue")?;
        
        let url = format!("{}/rest/api/3/issue/{}/transitions", self.base_url, issue_key);
        
        let transition_request = TransitionRequest {
            transition: TransitionId {
                id: done_transition.id.clone(),
            },
        };
        
        let response = self.client
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&transition_request)
            .send()
            .context("Failed to transition issue")?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to close issue: {}", response.status());
        }
        
        Ok(())
    }
    
    fn get_transitions(&self, issue_key: &str) -> Result<Vec<Transition>> {
        let url = format!("{}/rest/api/3/issue/{}/transitions", self.base_url, issue_key);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .send()
            .context("Failed to get transitions")?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to get transitions: {}", response.status());
        }
        
        let transitions_response: TransitionsResponse = response.json()
            .context("Failed to parse transitions response")?;
        
        Ok(transitions_response.transitions)
    }
}