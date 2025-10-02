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
    pub assignee: Option<User>,
    #[serde(default)]
    pub parent: Option<Box<JiraIssue>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "emailAddress")]
    pub email_address: Option<String>,
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
    
    pub fn get_issue_with_parent(&self, issue_key: &str) -> Result<JiraIssue> {
        // Request the issue with parent field expanded
        let url = format!("{}/rest/api/3/issue/{}", self.base_url, issue_key);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .query(&[("expand", "parent")])
            .send()
            .context("Failed to send request to JIRA")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("JIRA API error: {}", status);
        }
        
        let mut issue: JiraIssue = serde_json::from_str(&response_text)
            .context(format!("Failed to parse JIRA response. Response: {}", response_text))?;
        
        // Early return if parent already exists
        if issue.fields.parent.is_some() {
            return Ok(issue);
        }
        
        // Try to get parent from Epic Link custom field
        let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&response_text) else {
            return Ok(issue);
        };
        
        let Some(fields) = json_value.get("fields").and_then(|f| f.as_object()) else {
            return Ok(issue);
        };
        
        for (key, value) in fields.iter() {
            if !key.starts_with("customfield_") {
                continue;
            }
            
            let Some(epic_key) = value.as_str() else {
                continue;
            };
            
            if let Ok(epic) = self.get_issue(epic_key) {
                issue.fields.parent = Some(Box::new(epic));
                break;
            }
        }
        
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
    
    pub fn assign_issue(&self, issue_key: &str, account_id: Option<&str>) -> Result<()> {
        let url = format!("{}/rest/api/3/issue/{}/assignee", self.base_url, issue_key);
        
        #[derive(Debug, Serialize)]
        struct AssignRequest {
            #[serde(rename = "accountId")]
            account_id: Option<String>,
        }
        
        let assign_request = AssignRequest {
            account_id: account_id.map(|id| id.to_string()),
        };
        
        let response = self.client
            .put(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&assign_request)
            .send()
            .context("Failed to assign issue")?;
        
        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().unwrap_or_else(|_| "Unable to read response".to_string());
            eprintln!("Failed to assign issue. Status: {}, Response: {}", status, error_text);
            anyhow::bail!("Failed to assign issue: {}", status);
        }
        
        Ok(())
    }
    
    pub fn transition_to_in_progress(&self, issue_key: &str) -> Result<()> {
        let transitions = self.get_transitions(issue_key)?;
        
        let in_progress_transition = transitions.iter()
            .find(|t| t.name.to_lowercase().contains("in progress") ||
                      t.name.to_lowercase().contains("start") ||
                      t.name.to_lowercase().contains("begin"))
            .context("No 'In Progress' transition found for this issue")?;
        
        let url = format!("{}/rest/api/3/issue/{}/transitions", self.base_url, issue_key);
        
        let transition_request = TransitionRequest {
            transition: TransitionId {
                id: in_progress_transition.id.clone(),
            },
        };
        
        let response = self.client
            .post(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .json(&transition_request)
            .send()
            .context("Failed to transition issue to In Progress")?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to transition issue to In Progress: {}", response.status());
        }
        
        Ok(())
    }
    
    pub fn pickup_issue(&self, issue_key: &str) -> Result<()> {
        // Get current user's account ID
        let current_user = self.get_current_user()?;
        
        // First assign the issue to yourself
        self.assign_issue(issue_key, Some(&current_user.account_id))?;
        
        // Then transition it to In Progress
        self.transition_to_in_progress(issue_key)?;
        
        Ok(())
    }
    
    pub fn get_epic_children(&self, epic_key: &str) -> Result<Vec<JiraIssue>> {
        // Try modern approach first (parent field)
        let modern_jql = format!("parent={}", epic_key);
        let url = format!("{}/rest/api/3/search/jql", self.base_url);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .query(&[("jql", &modern_jql)])
            .send()
            .context("Failed to send search request to JIRA")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if status.is_success() {
            #[derive(Debug, Deserialize)]
            struct SearchResponse {
                issues: Vec<JiraIssue>,
            }
            
            let search_response: SearchResponse = serde_json::from_str(&response_text)
                .context("Failed to parse JIRA search response")?;
            
            // If we got results with modern approach, return them
            if !search_response.issues.is_empty() {
                return Ok(search_response.issues);
            }
        }
        
        // Fallback to legacy Epic Link approach
        let legacy_jql = format!("\"Epic Link\"={}", epic_key);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .query(&[("jql", &legacy_jql)])
            .send()
            .context("Failed to send search request to JIRA")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("JIRA API error: {}", status);
        }
        
        #[derive(Debug, Deserialize)]
        struct SearchResponse {
            issues: Vec<JiraIssue>,
        }
        
        let search_response: SearchResponse = serde_json::from_str(&response_text)
            .context("Failed to parse JIRA search response")?;
        
        Ok(search_response.issues)
    }
    
    pub fn get_my_issues(&self) -> Result<Vec<JiraIssue>> {
        // Use JQL to find all issues assigned to current user, excluding Done status
        let jql = "assignee = currentUser() AND status != Done ORDER BY updated DESC";
        let url = format!("{}/rest/api/3/search/jql", self.base_url);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .query(&[
                ("jql", jql),
                ("expand", "parent"),
                ("fields", "key,summary,status,assignee,description,parent")
            ])
            .send()
            .context("Failed to send search request to JIRA")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("JIRA API error: {}", status);
        }
        
        #[derive(Debug, Deserialize)]
        struct SearchResponse {
            issues: Vec<JiraIssue>,
        }
        
        let search_response: SearchResponse = serde_json::from_str(&response_text)
            .context("Failed to parse JIRA search response")?;
        
        Ok(search_response.issues)
    }
    
    pub fn get_assignable_users(&self, issue_key: &str) -> Result<Vec<User>> {
        let url = format!("{}/rest/api/3/user/assignable/search", self.base_url);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .query(&[
                ("issueKey", issue_key),
                ("maxResults", "50") // Reasonable limit for UI display
            ])
            .send()
            .context("Failed to get assignable users")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("Failed to get assignable users: {}", status);
        }
        
        let users: Vec<User> = serde_json::from_str(&response_text)
            .context("Failed to parse assignable users response")?;
        
        Ok(users)
    }
    
    pub fn get_current_user(&self) -> Result<User> {
        let url = format!("{}/rest/api/3/myself", self.base_url);
        
        let response = self.client
            .get(&url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(ACCEPT, "application/json")
            .send()
            .context("Failed to get current user info")?;
        
        let status = response.status();
        let response_text = response.text()?;
        
        if !status.is_success() {
            eprintln!("JIRA API error response: {}", response_text);
            anyhow::bail!("Failed to get current user: {}", status);
        }
        
        let user: User = serde_json::from_str(&response_text)
            .context("Failed to parse current user response")?;
        
        Ok(user)
    }
    
    pub fn get_all_epics(&self) -> Result<Vec<JiraIssue>> {
        // Try different epic type names
        let epic_types = vec!["Epic", "epic", "Epic Story", "Epic Feature"];
        let mut all_epics = Vec::new();
        let mut last_error = None;
        
        for epic_type in epic_types {
            let jql = format!("issuetype = \"{}\" AND status != Done ORDER BY updated DESC", epic_type);

            let url = format!("{}/rest/api/3/search/jql", self.base_url);
            
            let response = self.client
                .get(&url)
                .header(AUTHORIZATION, &self.auth_header)
                .header(ACCEPT, "application/json")
                .query(&[
                    ("jql", &jql),
                    ("fields", &"key,summary,status,assignee,updated".to_string()),
                    ("maxResults", &"100".to_string())
                ])
                .send()
                .context("Failed to send search request to JIRA")?;
            
            let status = response.status();
            let response_text = response.text()?;
            
            if status.is_success() {
                #[derive(Debug, Deserialize)]
                struct SearchResponse {
                    issues: Vec<JiraIssue>,
                }
                
                if let Ok(search_response) = serde_json::from_str::<SearchResponse>(&response_text) {
                    all_epics.extend(search_response.issues);
                }
            } else {
                // Check if it's a permission error
                if response_text.contains("does not exist or you do not have permission") {
                    last_error = Some(format!("Permission denied or '{}' issue type doesn't exist", epic_type));
                } else if response_text.contains("does not exist for the field 'issuetype'") {
                    // This epic type doesn't exist, try the next one
                    continue;
                } else {
                    eprintln!("JIRA API error for epic type '{}': {}", epic_type, response_text);
                    last_error = Some(format!("Failed to get epics: {}", status));
                }
            }
        }
        
        if all_epics.is_empty() {
            if let Some(error) = last_error {
                anyhow::bail!("Could not find any active epics. {}", error);
            } else {
                anyhow::bail!("No active epics found. Your JIRA instance might use a different issue type name for epics or all epics are Done.");
            }
        }
        
        // Remove duplicates (in case multiple epic types returned the same issues)
        let mut unique_epics = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();
        for epic in all_epics {
            if seen_keys.insert(epic.key.clone()) {
                unique_epics.push(epic);
            }
        }
        
        Ok(unique_epics)
    }
}