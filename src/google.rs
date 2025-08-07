use anyhow::{Context, Result};
use google_calendar3::{api, CalendarHub, hyper_rustls};
use yup_oauth2::{InstalledFlowAuthenticator, InstalledFlowReturnMethod};
use chrono::{DateTime, Utc, FixedOffset};
use chrono_tz::America::New_York;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub summary: String,
    pub start_time: DateTime<FixedOffset>,
    pub end_time: DateTime<FixedOffset>,
    pub meeting_url: Option<String>,
    pub location: Option<String>,
    pub description: Option<String>,
    pub status: String,
}

pub struct GoogleCalendarClient {
    hub: CalendarHub<hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>,
}

impl GoogleCalendarClient {
    pub async fn new(client_id: String, client_secret: String, token_path: PathBuf) -> Result<Self> {
        let secret = yup_oauth2::ApplicationSecret {
            client_id,
            client_secret,
            auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
            token_uri: "https://oauth2.googleapis.com/token".to_string(),
            ..Default::default()
        };

        let auth = InstalledFlowAuthenticator::builder(
            secret,
            InstalledFlowReturnMethod::HTTPPortRedirect(8080),
        )
        .persist_tokens_to_disk(token_path)
        .build()
        .await
        .context("Failed to build authenticator")?;

        let client = hyper_util::client::legacy::Client::builder(
            hyper_util::rt::TokioExecutor::new()
        ).build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .context("Failed to load native roots")?
                .https_only()
                .enable_http1()
                .build()
        );

        let hub = CalendarHub::new(client, auth);
        
        Ok(Self { hub })
    }

    pub async fn list_meetings(&self) -> Result<Vec<Meeting>> {
        let now = Utc::now();
        let week_from_now = now + chrono::Duration::days(7);
        
        let result = self.hub
            .events()
            .list("primary")
            .time_min(now)
            .time_max(week_from_now)
            .single_events(true)
            .order_by("startTime")
            .add_scope(api::Scope::Readonly)
            .doit()
            .await
            .context("Failed to fetch calendar events")?;

        let (_, events_list) = result;
        let mut meetings = Vec::new();

        if let Some(items) = events_list.items {
            for event in items {
                let meeting = self.parse_event_to_meeting(event)?;
                meetings.push(meeting);
            }
        }

        Ok(meetings)
    }

    fn parse_event_to_meeting(&self, event: api::Event) -> Result<Meeting> {
        let id = event.id.clone().unwrap_or_default();
        let summary = event.summary.clone().unwrap_or_else(|| "(No title)".to_string());
        let status = event.status.clone().unwrap_or_else(|| "confirmed".to_string());
        
        let (start_time, end_time) = self.extract_times(&event)?;
        
        let meeting_url = self.extract_meeting_url(&event);
        let location = event.location.clone();
        let description = event.description.clone();

        Ok(Meeting {
            id,
            summary,
            start_time,
            end_time,
            meeting_url,
            location,
            description,
            status,
        })
    }

    fn extract_times(&self, event: &api::Event) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
        let start = event.start.as_ref()
            .context("Event has no start time")?;
        let end = event.end.as_ref()
            .context("Event has no end time")?;

        let start_time = if let Some(date_time) = &start.date_time {
            date_time.with_timezone(&New_York)
        } else if let Some(date) = &start.date {
            let date_time_str = format!("{}T09:00:00+00:00", date);
            DateTime::parse_from_rfc3339(&date_time_str)
                .context("Failed to parse all-day start time")?
                .with_timezone(&New_York)
        } else {
            anyhow::bail!("Event has neither date_time nor date for start")
        };

        let end_time = if let Some(date_time) = &end.date_time {
            date_time.with_timezone(&New_York)
        } else if let Some(date) = &end.date {
            let date_time_str = format!("{}T17:00:00+00:00", date);
            DateTime::parse_from_rfc3339(&date_time_str)
                .context("Failed to parse all-day end time")?
                .with_timezone(&New_York)
        } else {
            anyhow::bail!("Event has neither date_time nor date for end")
        };

        Ok((start_time.fixed_offset(), end_time.fixed_offset()))
    }

    fn extract_meeting_url(&self, event: &api::Event) -> Option<String> {
        if let Some(hangout_link) = &event.hangout_link {
            return Some(hangout_link.clone());
        }

        if let Some(conference_data) = &event.conference_data {
            if let Some(entry_points) = &conference_data.entry_points {
                for entry_point in entry_points {
                    if entry_point.entry_point_type == Some("video".to_string()) {
                        if let Some(uri) = &entry_point.uri {
                            return Some(uri.clone());
                        }
                    }
                }
            }
        }

        if let Some(location) = &event.location {
            if location.starts_with("http://") || location.starts_with("https://") {
                return Some(location.clone());
            }
        }

        if let Some(description) = &event.description {
            let url_regex = regex::Regex::new(r"https?://[^\s<>]+(?:zoom\.us|meet\.google\.com|teams\.microsoft\.com)[^\s<>]*").ok()?;
            if let Some(mat) = url_regex.find(description) {
                return Some(mat.as_str().to_string());
            }
        }

        None
    }
}

pub fn blocking_list_meetings(client_id: String, client_secret: String, token_path: PathBuf) -> Result<Vec<Meeting>> {
    // Initialize the crypto provider for rustls
    let _ = rustls::crypto::ring::default_provider().install_default();
    
    let runtime = tokio::runtime::Runtime::new()
        .context("Failed to create Tokio runtime")?;
    
    runtime.block_on(async {
        let client = GoogleCalendarClient::new(client_id, client_secret, token_path).await?;
        client.list_meetings().await
    })
}