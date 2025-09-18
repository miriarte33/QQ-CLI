use anyhow::Result;
use chrono::Timelike;
use chrono_tz::America::New_York;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Wrap},
    Terminal, Frame,
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use serde_json::Value;

use crate::jira::{JiraIssue, JiraClient, User};

pub struct JiraIssueDisplay {
    scroll_offset: u16,
}

impl JiraIssueDisplay {
    pub fn show(issue: &JiraIssue) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut app = Self { scroll_offset: 0 };
        let mut should_quit = false;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f, issue))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                    KeyCode::Up => app.scroll_offset = app.scroll_offset.saturating_sub(1),
                    KeyCode::Down => app.scroll_offset = app.scroll_offset.saturating_add(1),
                    _ => {}
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn draw(&self, f: &mut Frame, issue: &JiraIssue) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(9),  // Header info (increased for assignee)
                Constraint::Min(0),     // Description
                Constraint::Length(2),  // Help text
            ])
            .split(f.area());

        self.render_header(f, chunks[0], issue);
        self.render_description(f, chunks[1], &issue.fields.description);
        self.render_help(f, chunks[2]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect, issue: &JiraIssue) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" JIRA Issue Details ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let assignee_text = match &issue.fields.assignee {
            Some(user) => user.display_name.clone(),
            None => "Unassigned".to_string(),
        };

        let header_text = vec![
            Line::from(vec![
                Span::styled("Ticket: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&issue.key),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&issue.fields.status.name),
            ]),
            Line::from(vec![
                Span::styled("Assignee: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&assignee_text),
            ]),
            Line::from(vec![
                Span::styled("Summary: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&issue.fields.summary),
            ]),
        ];

        let paragraph = Paragraph::new(header_text).wrap(Wrap { trim: true });
        f.render_widget(paragraph, inner);
    }

    fn render_description(&self, f: &mut Frame, area: Rect, description: &Option<Value>) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Description ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(desc) = description {
            self.render_jira_description(f, inner, desc);
        } else {
            let text = Paragraph::new("(No description provided)")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(text, inner);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help = Paragraph::new("Press 'q' or ESC to quit, ↑/↓ to scroll")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }

    fn render_jira_description(&self, f: &mut Frame, area: Rect, value: &Value) {
        let mut remaining_area = area;

        if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if remaining_area.height == 0 {
                    break;
                }

                let block_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                
                match block_type {
                    "paragraph" => {
                        if let Some(text) = self.extract_text(item) {
                            let paragraph = Paragraph::new(text)
                                .wrap(Wrap { trim: true });
                            let height = 2.min(remaining_area.height);
                            f.render_widget(paragraph, Rect { height, ..remaining_area });
                            
                            remaining_area.y += height;
                            remaining_area.height = remaining_area.height.saturating_sub(height);
                        }
                    }
                    "heading" => {
                        if let Some(text) = self.extract_text(item) {
                            let level = item.get("attrs")
                                .and_then(|a| a.get("level"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(1);
                            
                            let heading = format!("{} {}", "#".repeat(level as usize), text);
                            let paragraph = Paragraph::new(heading)
                                .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                            
                            let height = 2.min(remaining_area.height);
                            f.render_widget(paragraph, Rect { height, ..remaining_area });
                            
                            remaining_area.y += height;
                            remaining_area.height = remaining_area.height.saturating_sub(height);
                        }
                    }
                    "table" => {
                        if let Some((table_widget, height)) = self.create_table_widget(item, remaining_area.width) {
                            let table_height = height.min(remaining_area.height);
                            f.render_widget(table_widget, Rect { height: table_height, ..remaining_area });
                            
                            remaining_area.y += table_height + 1;
                            remaining_area.height = remaining_area.height.saturating_sub(table_height + 1);
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(text) = value.as_str() {
            let paragraph = Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .scroll((self.scroll_offset, 0));
            f.render_widget(paragraph, area);
        }
    }

    fn create_table_widget(&self, item: &Value, available_width: u16) -> Option<(Table, u16)> {
        if let Some(rows) = item.get("content").and_then(|c| c.as_array()) {
            let mut table_rows = Vec::new();
            let mut header_row = None;
            let mut max_cols = 0;
            
            // First pass: collect all rows and determine column count
            let mut all_cells: Vec<Vec<String>> = Vec::new();
            for row in rows {
                if row.get("type").and_then(|t| t.as_str()) == Some("tableRow") {
                    if let Some(cells) = row.get("content").and_then(|c| c.as_array()) {
                        let row_content: Vec<String> = cells.iter()
                            .map(|cell| self.extract_cell_text(cell).unwrap_or_default())
                            .collect();
                        max_cols = max_cols.max(row_content.len());
                        all_cells.push(row_content);
                    }
                }
            }

            if all_cells.is_empty() || max_cols == 0 {
                return None;
            }

            // Create constraints for equal-width columns
            let constraints: Vec<Constraint> = (0..max_cols)
                .map(|_| Constraint::Percentage(100 / max_cols as u16))
                .collect();

            // Calculate actual column width based on available width
            // Account for borders: 1 char per column separator + 2 for outer borders
            // Account for padding: 1 space on each side per column
            let borders_width = (max_cols - 1) + 2; // column separators + outer borders
            let padding_width = max_cols * 2; // 2 spaces per column
            let content_width = available_width.saturating_sub((borders_width + padding_width) as u16);
            let approx_col_width = (content_width / max_cols as u16).max(10) as usize;
            
            let mut total_height = 0u16;
            
            // Create table rows with text wrapping
            for (idx, row_content) in all_cells.iter().enumerate() {
                let mut max_lines_in_row = 1;
                let mut wrapped_cells: Vec<Vec<String>> = Vec::new();
                
                // Wrap text in each cell
                for cell_text in row_content {
                    let wrapped = textwrap::wrap(cell_text, approx_col_width);
                    let wrapped_lines: Vec<String> = wrapped.iter().map(|cow| cow.to_string()).collect();
                    max_lines_in_row = max_lines_in_row.max(wrapped_lines.len());
                    wrapped_cells.push(wrapped_lines);
                }
                
                // Pad cells to have the same number of lines
                for cell_lines in &mut wrapped_cells {
                    while cell_lines.len() < max_lines_in_row {
                        cell_lines.push(String::new());
                    }
                }
                
                // Create cells with multi-line content
                let cells: Vec<Cell> = wrapped_cells.into_iter()
                    .map(|lines| {
                        let text = lines.join("\n");
                        if idx == 0 {
                            Cell::from(text).style(
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                            )
                        } else {
                            Cell::from(text)
                        }
                    })
                    .collect();

                let row = Row::new(cells).height(max_lines_in_row as u16);
                total_height += max_lines_in_row as u16;
                
                if idx == 0 {
                    header_row = Some(row);
                } else {
                    table_rows.push(row);
                }
            }

            let mut table = Table::new(table_rows, constraints)
                .block(Block::default().borders(Borders::ALL));

            if let Some(header) = header_row {
                table = table.header(header);
            }

            // Total height includes borders and spacing
            let height = (total_height + 4).min(20);
            Some((table, height))
        } else {
            None
        }
    }

    fn extract_text(&self, item: &Value) -> Option<String> {
        if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<String> = content.iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .map(|s| s.to_string())
                .collect();
            
            if !texts.is_empty() {
                Some(texts.join(""))
            } else {
                None
            }
        } else {
            None
        }
    }

    fn extract_cell_text(&self, cell: &Value) -> Option<String> {
        if let Some(content) = cell.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<String> = content.iter()
                .filter_map(|item| self.extract_text(item))
                .collect();
            
            if !texts.is_empty() {
                Some(texts.join(" "))
            } else {
                None
            }
        } else {
            None
        }
    }
}

pub struct EpicListDisplay {
    selected_index: usize,
    children: Vec<JiraIssue>,
    scroll_offset: usize,
    viewport_height: usize,
}

impl EpicListDisplay {
    fn update_scroll_offset(&mut self, viewport_height: usize) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index.saturating_sub(viewport_height - 1);
        }
    }
    
    pub fn show(epic: &JiraIssue, children: Vec<JiraIssue>, client: &JiraClient) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut app = Self {
            selected_index: 0,
            children,
            scroll_offset: 0,
            viewport_height: 20, // Will be updated during first render
        };
        
        let mut should_quit = false;
        let mut message: Option<String> = None;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f, epic, &message))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                    KeyCode::Up => {
                        if app.selected_index > 0 {
                            app.selected_index -= 1;
                            // Estimate viewport height - can be refined based on terminal size
                            app.update_scroll_offset(app.viewport_height);
                        }
                    }
                    KeyCode::Down => {
                        if app.selected_index < app.children.len().saturating_sub(1) {
                            app.selected_index += 1;
                            // Estimate viewport height - can be refined based on terminal size
                            app.update_scroll_offset(app.viewport_height);
                        }
                    }
                    KeyCode::Char('a') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            // Temporarily restore terminal for assignee selection
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            
                            println!("Fetching assignable users for {}...", issue_key);
                            
                            // Get current user and assignable users
                            let (current_user_id, selected_account_id) = match (
                                client.get_current_user(),
                                client.get_assignable_users(&issue_key)
                            ) {
                                (Ok(current_user), Ok(users)) => {
                                    let current_id = current_user.account_id.clone();
                                    let selected = AssigneeSelector::show(users, current_user.account_id)?;
                                    (current_id, selected)
                                }
                                (Err(e), _) | (_, Err(e)) => {
                                    eprintln!("Failed to fetch users: {}", e);
                                    (String::new(), None)
                                }
                            };
                            
                            // Re-setup terminal
                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;
                            
                            if let Some(account_id) = selected_account_id {
                                if account_id == "UNASSIGN" {
                                    // Handle unassignment
                                    message = Some(format!("Unassigning {} ...", issue_key));
                                    terminal.draw(|f| app.draw(f, epic, &message))?;
                                    
                                    match client.assign_issue(&issue_key, None) {
                                        Ok(_) => {
                                            message = Some(format!("✓ {} unassigned", issue_key));
                                            // Refresh the issue data
                                            if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                                app.children[app.selected_index] = updated_issue;
                                            }
                                        }
                                        Err(e) => {
                                            message = Some(format!("✗ Failed to unassign {}: {}", issue_key, e));
                                        }
                                    }
                                } else {
                                    // Handle normal assignment
                                    message = Some(format!("Assigning {} ...", issue_key));
                                    terminal.draw(|f| app.draw(f, epic, &message))?;
                                    
                                    match client.assign_issue(&issue_key, Some(&account_id)) {
                                        Ok(_) => {
                                            let assignee_text = if account_id == current_user_id {
                                                "you".to_string()
                                            } else {
                                                "selected user".to_string()
                                            };
                                            message = Some(format!("✓ {} assigned to {}", issue_key, assignee_text));
                                            // Refresh the issue data
                                            if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                                app.children[app.selected_index] = updated_issue;
                                            }
                                        }
                                        Err(e) => {
                                            message = Some(format!("✗ Failed to assign {}: {}", issue_key, e));
                                        }
                                    }
                                }
                            } else {
                                message = Some("Assignment cancelled".to_string());
                            }
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Moving {} to In Progress...", issue_key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            match client.transition_to_in_progress(&issue_key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} moved to In Progress", issue_key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                        app.children[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to move {} to In Progress: {}", issue_key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Starting {}...", issue_key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            // Create feature branch
                            use git2::Repository;
                            match Repository::open(".") {
                                Ok(repo) => {
                                    let branch_name = format!("feature/{}", issue_key);
                                    
                                    // Get the current HEAD commit
                                    match repo.head()
                                        .and_then(|head| head.target().ok_or_else(|| git2::Error::from_str("No HEAD target")))
                                        .and_then(|target| repo.find_commit(target))
                                    {
                                        Ok(commit) => {
                                            // Create and checkout the new branch
                                            match repo.branch(&branch_name, &commit, false) {
                                                Ok(_) => {
                                                    if let Ok(obj) = repo.revparse_single(&format!("refs/heads/{}", branch_name)) {
                                                        let _ = repo.checkout_tree(&obj, None);
                                                        let _ = repo.set_head(&format!("refs/heads/{}", branch_name));
                                                        
                                                        // Now pickup the issue
                                                        match client.pickup_issue(&issue_key) {
                                                            Ok(_) => {
                                                                message = Some(format!("✓ Created branch '{}' and picked up {}", branch_name, issue_key));
                                                                should_quit = true; // Exit after successful start
                                                            }
                                                            Err(e) => {
                                                                message = Some(format!("✗ Branch created but failed to pickup: {}", e));
                                                            }
                                                        }
                                                    } else {
                                                        message = Some(format!("✗ Failed to checkout branch '{}'", branch_name));
                                                    }
                                                }
                                                Err(e) => {
                                                    message = Some(format!("✗ Failed to create branch: {}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            message = Some(format!("✗ Failed to get HEAD commit: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to open git repository: {}", e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('v') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            // Temporarily restore terminal for nested UI
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            
                            // Show the issue details
                            println!("Viewing issue: {}", issue_key);
                            let _ = JiraIssueDisplay::show(issue);
                            
                            // Re-setup terminal for epic list
                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;
                            
                            message = Some(format!("Returned from viewing {}", issue_key));
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Closing {}...", issue_key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            match client.close_issue(&issue_key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} closed successfully", issue_key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                        app.children[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to close {}: {}", issue_key, e));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn draw(&mut self, f: &mut Frame, epic: &JiraIssue, message: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(5),    // Epic header
                Constraint::Min(0),       // Children table
                Constraint::Length(2),    // Message area
                Constraint::Length(2),    // Help text
            ])
            .split(f.area());

        self.render_epic_header(f, chunks[0], epic);
        self.render_children_table(f, chunks[1]);
        self.render_message(f, chunks[2], message);
        self.render_help(f, chunks[3]);
    }

    fn render_epic_header(&self, f: &mut Frame, area: Rect, epic: &JiraIssue) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Epic Details ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header_text = vec![
            Line::from(vec![
                Span::styled("Epic: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&epic.key),
                Span::raw(" - "),
                Span::raw(&epic.fields.summary),
            ]),
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(&epic.fields.status.name),
            ]),
        ];

        let paragraph = Paragraph::new(header_text);
        f.render_widget(paragraph, inner);
    }

    fn render_children_table(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Child Issues ({}) ", self.children.len()))
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.children.is_empty() {
            let text = Paragraph::new("(No child issues in this epic)")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(text, inner);
            return;
        }

        // Create table headers
        let header_cells: Vec<Cell> = vec!["", "Key", "Status", "Summary", "Assignee"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .collect();
        let header = Row::new(header_cells).height(1);

        // Calculate viewport dimensions for table (accounting for header)
        let viewport_height = inner.height.saturating_sub(2) as usize; // -2 for header and border
        self.viewport_height = viewport_height; // Store for use in key handlers
        
        // Use the persisted scroll_offset
        let visible_start = self.scroll_offset;
        let visible_end = (self.scroll_offset + viewport_height).min(self.children.len());
        
        let rows: Vec<Row> = self.children[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(visible_idx, issue)| {
                let actual_idx = visible_start + visible_idx;
                let assignee = issue.fields.assignee.as_ref()
                    .map(|u| u.display_name.clone())
                    .unwrap_or_else(|| "Unassigned".to_string());
                
                // Color code the status
                let status_style = match issue.fields.status.name.to_lowercase().as_str() {
                    s if s.contains("done") || s.contains("closed") => Style::default().fg(Color::Green),
                    s if s.contains("progress") => Style::default().fg(Color::Yellow),
                    s if s.contains("review") => Style::default().fg(Color::Magenta),
                    _ => Style::default().fg(Color::White),
                };
                
                // Selection indicator
                let indicator = if actual_idx == self.selected_index { "➤" } else { "" };
                
                let cells = vec![
                    Cell::from(indicator).style(Style::default().fg(Color::Green)),
                    Cell::from(issue.key.clone()),
                    Cell::from(issue.fields.status.name.clone()).style(status_style),
                    Cell::from(issue.fields.summary.clone()),
                    Cell::from(assignee),
                ];
                
                Row::new(cells).height(1)
            })
            .collect();

        // Add scroll indicators in the title
        let title = if self.children.len() > viewport_height {
            format!(" Child Issues ({}) [{}-{} of {}] ", 
                self.children.len(),
                visible_start + 1,
                visible_end,
                self.children.len()
            )
        } else {
            format!(" Child Issues ({}) ", self.children.len())
        };

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),      // Arrow indicator
                Constraint::Length(12),     // Key
                Constraint::Length(15),     // Status
                Constraint::Min(20),        // Summary (takes remaining space)
                Constraint::Length(20),     // Assignee
            ]
        )
        .header(header)
        .block(Block::default().title(title));

        f.render_widget(table, inner);
    }

    fn render_message(&self, f: &mut Frame, area: Rect, message: &Option<String>) {
        if let Some(msg) = message {
            let style = if msg.starts_with('✓') {
                Style::default().fg(Color::Green)
            } else if msg.starts_with('✗') {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Yellow)
            };
            
            let text = Paragraph::new(msg.as_str())
                .style(style)
                .alignment(Alignment::Center);
            f.render_widget(text, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help = Paragraph::new("↑/↓: Navigate | v: View | a: Assign to... | p: In Progress | c: Close | s: Start | q/ESC: Quit")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

pub struct MyIssuesDisplay {
    selected_index: usize,
    issues: Vec<JiraIssue>,
    scroll_offset: usize,
    viewport_height: usize,
}

impl MyIssuesDisplay {
    fn update_scroll_offset(&mut self, viewport_height: usize) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index.saturating_sub(viewport_height - 1);
        }
    }
    
    pub fn show(issues: Vec<JiraIssue>, client: &JiraClient) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut app = Self {
            selected_index: 0,
            issues,
            scroll_offset: 0,
            viewport_height: 20, // Will be updated during first render
        };
        
        let mut should_quit = false;
        let mut message: Option<String> = None;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f, &message))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                    KeyCode::Up => {
                        if app.selected_index > 0 {
                            app.selected_index -= 1;
                            app.update_scroll_offset(app.viewport_height); // Typical terminal height
                        }
                    }
                    KeyCode::Down => {
                        if app.selected_index < app.issues.len().saturating_sub(1) {
                            app.selected_index += 1;
                            app.update_scroll_offset(app.viewport_height); // Typical terminal height
                        }
                    }
                    KeyCode::Char('v') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            // Temporarily restore terminal for nested UI
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            
                            // Show the issue details
                            println!("Viewing issue: {}", issue_key);
                            let _ = JiraIssueDisplay::show(issue);
                            
                            // Re-setup terminal
                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;
                            
                            message = Some(format!("Returned from viewing {}", issue_key));
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Moving {} to In Progress...", issue_key));
                            terminal.draw(|f| app.draw(f, &message))?;
                            
                            match client.transition_to_in_progress(&issue_key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} moved to In Progress", issue_key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                        app.issues[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to move {}: {}", issue_key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Starting {}...", issue_key));
                            terminal.draw(|f| app.draw(f, &message))?;
                            
                            // Create feature branch
                            use git2::Repository;
                            match Repository::open(".") {
                                Ok(repo) => {
                                    let branch_name = format!("feature/{}", issue_key);
                                    
                                    // Get the current HEAD commit
                                    match repo.head()
                                        .and_then(|head| head.target().ok_or_else(|| git2::Error::from_str("No HEAD target")))
                                        .and_then(|target| repo.find_commit(target))
                                    {
                                        Ok(commit) => {
                                            // Create and checkout the new branch
                                            match repo.branch(&branch_name, &commit, false) {
                                                Ok(_) => {
                                                    if let Ok(obj) = repo.revparse_single(&format!("refs/heads/{}", branch_name)) {
                                                        let _ = repo.checkout_tree(&obj, None);
                                                        let _ = repo.set_head(&format!("refs/heads/{}", branch_name));
                                                        
                                                        // Now pickup the issue
                                                        match client.pickup_issue(&issue_key) {
                                                            Ok(_) => {
                                                                message = Some(format!("✓ Created branch '{}' and picked up {}", branch_name, issue_key));
                                                                should_quit = true; // Exit after successful start
                                                            }
                                                            Err(e) => {
                                                                message = Some(format!("✗ Branch created but failed to pickup: {}", e));
                                                            }
                                                        }
                                                    } else {
                                                        message = Some(format!("✗ Failed to checkout branch '{}'", branch_name));
                                                    }
                                                }
                                                Err(e) => {
                                                    message = Some(format!("✗ Failed to create branch: {}", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            message = Some(format!("✗ Failed to get HEAD commit: {}", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to open git repository: {}", e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            let issue_key = issue.key.clone();
                            message = Some(format!("Closing {}...", issue_key));
                            terminal.draw(|f| app.draw(f, &message))?;
                            
                            match client.close_issue(&issue_key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} closed successfully", issue_key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue_key) {
                                        app.issues[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to close {}: {}", issue_key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            if let Some(parent) = &issue.fields.parent {
                                let parent_key = parent.key.clone();
                                // Temporarily restore terminal
                                disable_raw_mode()?;
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                terminal.show_cursor()?;
                                
                                // Show the parent epic
                                println!("Fetching epic details for: {}", parent_key);
                                if let Ok(children) = client.get_epic_children(&parent_key) {
                                    println!("Fetching child issues...");
                                    let _ = EpicListDisplay::show(parent, children, client);
                                }
                                
                                // Re-setup terminal
                                enable_raw_mode()?;
                                let mut stdout = io::stdout();
                                execute!(stdout, EnterAlternateScreen)?;
                                let backend = CrosstermBackend::new(stdout);
                                terminal = Terminal::new(backend)?;
                                
                                message = Some(format!("Returned from viewing epic {}", parent_key));
                            } else {
                                message = Some("This issue is not part of an epic".to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn draw(&mut self, f: &mut Frame, message: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),    // Header
                Constraint::Min(0),       // Issues table
                Constraint::Length(2),    // Message area
                Constraint::Length(2),    // Help text
            ])
            .split(f.area());

        self.render_header(f, chunks[0]);
        self.render_issues_table(f, chunks[1]);
        self.render_message(f, chunks[2], message);
        self.render_help(f, chunks[3]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" My Issues ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header_text = vec![
            Line::from(vec![
                Span::styled("Total Issues: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(self.issues.len().to_string()),
            ]),
        ];

        let paragraph = Paragraph::new(header_text);
        f.render_widget(paragraph, inner);
    }

    fn render_issues_table(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Issues ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.issues.is_empty() {
            let text = Paragraph::new("(No issues assigned to you)")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(text, inner);
            return;
        }

        // Create table headers
        let header_cells: Vec<Cell> = vec!["", "Key", "Parent", "Status", "Summary"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .collect();
        let header = Row::new(header_cells).height(1);

        // Calculate viewport dimensions for table (accounting for header)
        let viewport_height = inner.height.saturating_sub(2) as usize; // -2 for header and border
        self.viewport_height = viewport_height; // Store for use in key handlers
        
        // Use the persisted scroll_offset
        let visible_start = self.scroll_offset;
        let visible_end = (self.scroll_offset + viewport_height).min(self.issues.len());
        
        let rows: Vec<Row> = self.issues[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(visible_idx, issue)| {
                let actual_idx = visible_start + visible_idx;
                let parent = issue.fields.parent.as_ref()
                    .map(|p| p.key.clone())
                    .unwrap_or_else(|| "—".to_string());
                
                // Color code the status
                let status_style = match issue.fields.status.name.to_lowercase().as_str() {
                    s if s.contains("done") || s.contains("closed") => Style::default().fg(Color::Green),
                    s if s.contains("progress") => Style::default().fg(Color::Yellow),
                    s if s.contains("review") => Style::default().fg(Color::Magenta),
                    _ => Style::default().fg(Color::White),
                };
                
                // Selection indicator
                let indicator = if actual_idx == self.selected_index { "➤" } else { "" };
                
                let cells = vec![
                    Cell::from(indicator).style(Style::default().fg(Color::Green)),
                    Cell::from(issue.key.clone()),
                    Cell::from(parent),
                    Cell::from(issue.fields.status.name.clone()).style(status_style),
                    Cell::from(issue.fields.summary.clone()),
                ];
                
                Row::new(cells).height(1)
            })
            .collect();

        // Update title with scroll indicators
        let title = if self.issues.len() > viewport_height {
            format!(" Issues [{}-{} of {}] ", 
                visible_start + 1,
                visible_end,
                self.issues.len()
            )
        } else {
            " Issues ".to_string()
        };

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),      // Arrow indicator
                Constraint::Length(12),     // Key
                Constraint::Length(12),     // Parent
                Constraint::Length(15),     // Status
                Constraint::Min(20),        // Summary (takes remaining space)
            ]
        )
        .header(header)
        .block(Block::default().title(title));

        f.render_widget(table, inner);
    }

    fn render_message(&self, f: &mut Frame, area: Rect, message: &Option<String>) {
        if let Some(msg) = message {
            let style = if msg.starts_with('✓') {
                Style::default().fg(Color::Green)
            } else if msg.starts_with('✗') {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Yellow)
            };
            
            let text = Paragraph::new(msg.as_str())
                .style(style)
                .alignment(Alignment::Center);
            f.render_widget(text, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help = Paragraph::new("↑/↓: Navigate | v: View | c: Close | e: Epic | p: In Progress | s: Start | q/ESC: Quit")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

pub struct AssigneeSelector {
    selected_index: usize,
    users: Vec<User>,
    filtered_indices: Vec<usize>,
    current_user_id: String,
    search_query: String,
    search_mode: bool,
    scroll_offset: usize,
    viewport_height: usize,
}

impl AssigneeSelector {
    pub fn show(users: Vec<User>, current_user_id: String) -> Result<Option<String>> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let filtered_indices: Vec<usize> = (0..users.len()).collect();
        let mut app = Self {
            selected_index: 0,
            users,
            filtered_indices,
            current_user_id,
            search_query: String::new(),
            search_mode: false,
            scroll_offset: 0,
            viewport_height: 20, // Will be updated during first render
        };
        
        let mut selected_account_id: Option<String> = None;
        let mut should_quit = false;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f))?;

            if let Event::Key(key) = event::read()? {
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            app.search_mode = false;
                        }
                        KeyCode::Enter => {
                            app.search_mode = false;
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.update_filter();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.update_filter();
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                        KeyCode::Char('/') => {
                            app.search_mode = true;
                        }
                        KeyCode::Up => {
                            if app.selected_index > 0 {
                                app.selected_index -= 1;
                                app.update_scroll_offset(app.viewport_height); // Typical terminal height
                            }
                        }
                        KeyCode::Down => {
                            let total_items = app.filtered_indices.len() + 2; // +2 for "Myself" and "None"
                            if app.selected_index < total_items.saturating_sub(1) {
                                app.selected_index += 1;
                                app.update_scroll_offset(app.viewport_height);
                            }
                        }
                        KeyCode::Enter => {
                            if app.selected_index == 0 {
                                // "Myself" selected
                                selected_account_id = Some(app.current_user_id.clone());
                            } else if app.selected_index == 1 {
                                // "None" selected - return a special marker
                                selected_account_id = Some("UNASSIGN".to_string());
                            } else if let Some(&user_idx) = app.filtered_indices.get(app.selected_index - 2) {
                                if let Some(user) = app.users.get(user_idx) {
                                    selected_account_id = Some(user.account_id.clone());
                                }
                            }
                            should_quit = true;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(selected_account_id)
    }
    
    fn update_filter(&mut self) {
        self.filtered_indices.clear();
        let query_lower = self.search_query.to_lowercase();
        
        for (idx, user) in self.users.iter().enumerate() {
            if user.display_name.to_lowercase().contains(&query_lower) {
                self.filtered_indices.push(idx);
            }
        }
        
        // Reset selection if current selection is out of bounds
        let total_items = self.filtered_indices.len() + 1; // +1 for "Myself"
        if self.selected_index >= total_items {
            self.selected_index = 0;
        }
        
        // Reset scroll to show selected item
        self.scroll_offset = 0;
        self.update_scroll_offset(self.viewport_height);
    }
    
    fn update_scroll_offset(&mut self, viewport_height: usize) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index.saturating_sub(viewport_height - 1);
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(if self.search_mode { 5 } else { 3 }),    // Header (bigger in search mode)
                Constraint::Min(0),       // User list
                Constraint::Length(2),    // Help text
            ])
            .split(f.area());

        self.render_header(f, chunks[0]);
        self.render_user_list(f, chunks[1]);
        self.render_help(f, chunks[2]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Select Assignee ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.search_mode {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);
                
            let header_text = vec![
                Line::from(vec![
                    Span::styled("Search for users", Style::default().fg(Color::Cyan)),
                ]),
            ];
            let paragraph = Paragraph::new(header_text);
            f.render_widget(paragraph, chunks[0]);
            
            let search_text = vec![
                Line::from(vec![
                    Span::styled("Filter: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&self.search_query),
                    Span::styled("_", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
                ]),
            ];
            let search_para = Paragraph::new(search_text);
            f.render_widget(search_para, chunks[1]);
            
            let found_count = self.filtered_indices.len();
            let count_text = vec![
                Line::from(vec![
                    Span::styled(format!("{} user(s) found", found_count), Style::default().fg(Color::DarkGray)),
                ]),
            ];
            let count_para = Paragraph::new(count_text);
            f.render_widget(count_para, chunks[2]);
        } else {
            let header_text = vec![
                Line::from(vec![
                    Span::styled("Select a user to assign the issue to", Style::default().fg(Color::Cyan)),
                ]),
            ];
            let paragraph = Paragraph::new(header_text);
            f.render_widget(paragraph, inner);
        }
    }

    fn render_user_list(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Users ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let viewport_height = inner.height as usize;
        self.viewport_height = viewport_height; // Store for use in key handlers
        let mut items: Vec<ListItem> = Vec::new();
        
        // Calculate total items and adjust scroll
        let total_items = self.filtered_indices.len() + 2; // +2 for "Myself" and "None"
        
        // Use the persisted scroll_offset
        let scroll_offset = self.scroll_offset;
        
        // Show scroll indicators
        if scroll_offset > 0 {
            items.push(ListItem::new("↑ more above ↑").style(Style::default().fg(Color::DarkGray)));
        }
        
        let start_idx = if scroll_offset > 0 { scroll_offset + 1 } else { scroll_offset };
        let end_idx = (start_idx + viewport_height.saturating_sub(if scroll_offset > 0 { 2 } else { 1 }))
            .min(total_items);
        
        // Add visible items
        for visible_idx in start_idx..end_idx {
            if visible_idx == 0 {
                // "Myself" option
                let indicator = if self.selected_index == 0 { "➤ " } else { "  " };
                let text = format!("{} Myself", indicator);
                items.push(ListItem::new(text).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
            } else if visible_idx == 1 {
                // "None" option (unassign)
                let indicator = if self.selected_index == 1 { "➤ " } else { "  " };
                let text = format!("{} None (unassign)", indicator);
                items.push(ListItem::new(text).style(Style::default().fg(Color::Red)));
            } else if let Some(&user_idx) = self.filtered_indices.get(visible_idx - 2) {
                if let Some(user) = self.users.get(user_idx) {
                    let indicator = if visible_idx == self.selected_index { "➤ " } else { "  " };
                    
                    let display_text = if user.account_id == self.current_user_id {
                        format!("{}{} (you)", indicator, user.display_name)
                    } else {
                        format!("{}{}", indicator, user.display_name)
                    };
                    
                    items.push(ListItem::new(display_text));
                }
            }
        }
        
        // Show scroll indicator at bottom
        if end_idx < total_items {
            items.push(ListItem::new("↓ more below ↓").style(Style::default().fg(Color::DarkGray)));
        }

        let list = List::new(items)
            .block(Block::default());

        f.render_widget(list, inner);
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = if self.search_mode {
            "Type to filter | Enter: Confirm | ESC: Cancel search"
        } else {
            "↑/↓: Navigate | /: Search | Enter: Select | q/ESC: Cancel"
        };
        
        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

pub struct AllEpicsDisplay {
    selected_index: usize,
    epics: Vec<JiraIssue>,
    filtered_indices: Vec<usize>,
    search_query: String,
    search_mode: bool,
    scroll_offset: usize,
    viewport_height: usize,
}

impl AllEpicsDisplay {
    pub fn show(epics: Vec<JiraIssue>, client: &JiraClient) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let epic_count = epics.len();
        let filtered_indices: Vec<usize> = (0..epic_count).collect();
        
        let mut app = Self {
            selected_index: 0,
            epics,
            filtered_indices,
            search_query: String::new(),
            search_mode: false,
            scroll_offset: 0,
            viewport_height: 20, // Will be updated during first render
        };
        
        let mut should_quit = false;
        let mut message: Option<String> = None;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f, &message))?;

            if let Event::Key(key) = event::read()? {
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            app.search_mode = false;
                        }
                        KeyCode::Enter => {
                            app.search_mode = false;
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.update_filter();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.update_filter();
                        }
                        _ => {}
                    }
                } else {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                        KeyCode::Char('/') => {
                            app.search_mode = true;
                        }
                        KeyCode::Up => {
                            if app.selected_index > 0 {
                                app.selected_index -= 1;
                                app.update_scroll_offset(app.viewport_height);
                            }
                        }
                        KeyCode::Down => {
                            if app.selected_index < app.filtered_indices.len().saturating_sub(1) {
                                app.selected_index += 1;
                                app.update_scroll_offset(app.viewport_height);
                            }
                        }
                        KeyCode::Char('v') => {
                            if let Some(&epic_idx) = app.filtered_indices.get(app.selected_index) {
                                if let Some(epic) = app.epics.get(epic_idx) {
                                    let epic_key = epic.key.clone();
                                    // Temporarily restore terminal for nested UI
                                    disable_raw_mode()?;
                                    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                    terminal.show_cursor()?;
                                    
                                    // Fetch and show the epic with its children
                                    println!("Fetching child issues for {}...", epic_key);
                                    if let Ok(children) = client.get_epic_children(&epic_key) {
                                        let _ = EpicListDisplay::show(epic, children, client);
                                    }
                                    
                                    // Re-setup terminal
                                    enable_raw_mode()?;
                                    let mut stdout = io::stdout();
                                    execute!(stdout, EnterAlternateScreen)?;
                                    let backend = CrosstermBackend::new(stdout);
                                    terminal = Terminal::new(backend)?;
                                    
                                    message = Some(format!("Returned from viewing {}", epic_key));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }
    
    fn update_scroll_offset(&mut self, viewport_height: usize) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index.saturating_sub(viewport_height - 1);
        }
    }
    
    fn update_filter(&mut self) {
        self.filtered_indices.clear();
        let query_lower = self.search_query.to_lowercase();
        
        for (idx, epic) in self.epics.iter().enumerate() {
            let key_matches = epic.key.to_lowercase().contains(&query_lower);
            let summary_matches = epic.fields.summary.to_lowercase().contains(&query_lower);
            
            if key_matches || summary_matches {
                self.filtered_indices.push(idx);
            }
        }
        
        // Reset selection if current selection is out of bounds
        if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = 0;
        }
        
        // Reset scroll to show selected item
        self.scroll_offset = 0;
        self.update_scroll_offset(self.viewport_height);
    }

    fn draw(&mut self, f: &mut Frame, message: &Option<String>) {
        let constraints = if self.search_mode {
            vec![
                Constraint::Length(3),     // Header
                Constraint::Length(3),     // Search bar
                Constraint::Min(0),        // Epic list
                Constraint::Length(1),     // Message line
                Constraint::Length(2),     // Help text
            ]
        } else {
            vec![
                Constraint::Length(3),     // Header
                Constraint::Min(0),        // Epic list
                Constraint::Length(1),     // Message line
                Constraint::Length(2),     // Help text
            ]
        };
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(f.area());

        if self.search_mode {
            self.render_header(f, chunks[0]);
            self.render_search_bar(f, chunks[1]);
            self.render_epics_table(f, chunks[2]);
            self.render_message(f, chunks[3], message);
            self.render_help(f, chunks[4]);
        } else {
            self.render_header(f, chunks[0]);
            self.render_epics_table(f, chunks[1]);
            self.render_message(f, chunks[2], message);
            self.render_help(f, chunks[3]);
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" All Epics ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header_text = if !self.search_query.is_empty() {
            vec![
                Line::from(vec![
                    Span::styled("Filtered Epics: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::raw(format!("{} / {}", self.filtered_indices.len(), self.epics.len())),
                ]),
            ]
        } else {
            vec![
                Line::from(vec![
                    Span::styled("Total Epics: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(self.epics.len().to_string()),
                ]),
            ]
        };

        let paragraph = Paragraph::new(header_text);
        f.render_widget(paragraph, inner);
    }
    
    fn render_search_bar(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Search ")
            .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);
        
        let search_text = vec![
            Line::from(vec![
                Span::styled("Filter: ", Style::default().fg(Color::Cyan)),
                Span::raw(&self.search_query),
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            ]),
        ];
        
        let paragraph = Paragraph::new(search_text);
        f.render_widget(paragraph, inner);
    }

    fn render_epics_table(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Epics ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if self.filtered_indices.is_empty() {
            let text = if self.search_query.is_empty() {
                "(No epics found)"
            } else {
                "(No epics match your search)"
            };
            let paragraph = Paragraph::new(text)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center);
            f.render_widget(paragraph, inner);
            return;
        }

        // Create table headers
        let header_cells: Vec<Cell> = vec!["", "Key", "Status", "Summary"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .collect();
        let header = Row::new(header_cells).height(1);

        // Calculate viewport dimensions for table (accounting for header)
        let viewport_height = inner.height.saturating_sub(2) as usize; // -2 for header and border
        self.viewport_height = viewport_height; // Store for use in key handlers
        
        // Use the persisted scroll_offset with filtered indices
        let visible_start = self.scroll_offset;
        let visible_end = (self.scroll_offset + viewport_height).min(self.filtered_indices.len());
        
        let rows: Vec<Row> = self.filtered_indices[visible_start..visible_end]
            .iter()
            .enumerate()
            .map(|(visible_idx, &epic_idx)| {
                let epic = &self.epics[epic_idx];
                let actual_idx = visible_start + visible_idx;
                
                // Color code the status
                let status_style = match epic.fields.status.name.to_lowercase().as_str() {
                    s if s.contains("done") || s.contains("closed") => Style::default().fg(Color::Green),
                    s if s.contains("progress") => Style::default().fg(Color::Yellow),
                    s if s.contains("review") => Style::default().fg(Color::Magenta),
                    _ => Style::default().fg(Color::White),
                };
                
                // Selection indicator
                let indicator = if actual_idx == self.selected_index { "➤" } else { "" };
                
                let cells = vec![
                    Cell::from(indicator).style(Style::default().fg(Color::Green)),
                    Cell::from(epic.key.clone()),
                    Cell::from(epic.fields.status.name.clone()).style(status_style),
                    Cell::from(epic.fields.summary.clone()),
                ];
                
                Row::new(cells).height(1)
            })
            .collect();

        // Update title with scroll indicators
        let title = if self.epics.len() > viewport_height {
            format!(" Epics [{}-{} of {}] ", 
                visible_start + 1,
                visible_end,
                self.epics.len()
            )
        } else {
            " Epics ".to_string()
        };

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(3),      // Arrow indicator
                Constraint::Length(12),     // Key
                Constraint::Length(15),     // Status
                Constraint::Min(20),        // Summary (takes remaining space)
            ]
        )
        .header(header)
        .block(Block::default().title(title));

        f.render_widget(table, inner);
    }

    fn render_message(&self, f: &mut Frame, area: Rect, message: &Option<String>) {
        if let Some(msg) = message {
            let style = if msg.starts_with('✓') {
                Style::default().fg(Color::Green)
            } else if msg.starts_with('✗') {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Yellow)
            };
            
            let text = Paragraph::new(msg.as_str())
                .style(style)
                .alignment(Alignment::Center);
            f.render_widget(text, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = if self.search_mode {
            "Type to search | Enter/ESC: Exit search | Backspace: Delete"
        } else {
            "↑/↓: Navigate | v: View Epic | /: Search | q/ESC: Quit"
        };
        
        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

pub struct MeetingsListDisplay {
    selected_index: usize,
    meetings: Vec<crate::google::Meeting>,
    scroll_offset: usize,
    viewport_height: usize,
}

impl MeetingsListDisplay {
    pub fn show(meetings: Vec<crate::google::Meeting>) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut app = Self {
            selected_index: 0,
            meetings,
            scroll_offset: 0,
            viewport_height: 20, // Will be updated during first render
        };
        
        let mut should_quit = false;
        let mut message: Option<String> = None;

        // Main loop
        while !should_quit {
            terminal.draw(|f| app.draw(f, &message))?;

            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => should_quit = true,
                    KeyCode::Up => {
                        if app.selected_index > 0 {
                            app.selected_index -= 1;
                            app.update_scroll();
                        }
                    }
                    KeyCode::Down => {
                        if app.selected_index < app.meetings.len().saturating_sub(1) {
                            app.selected_index += 1;
                            app.update_scroll();
                        }
                    }
                    KeyCode::Char('j') => {
                        if let Some(meeting) = app.meetings.get(app.selected_index) {
                            let meeting_summary = meeting.summary.clone();
                            let meeting_url = meeting.meeting_url.clone();
                            
                            if let Some(url) = meeting_url {
                                message = Some(format!("Opening meeting: {}", meeting_summary));
                                terminal.draw(|f| app.draw(f, &message))?;
                                
                                if let Err(e) = webbrowser::open(&url) {
                                    message = Some(format!("Failed to open browser: {}", e));
                                } else {
                                    message = Some(format!("✓ Opened meeting in browser"));
                                }
                            } else {
                                message = Some(format!("No meeting URL available for: {}", meeting_summary));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn update_scroll(&mut self) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + self.viewport_height {
            self.scroll_offset = self.selected_index - self.viewport_height + 1;
        }
    }

    fn draw(&mut self, f: &mut Frame, message: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),     // Header
                Constraint::Min(0),        // Meetings table
                Constraint::Length(1),     // Message line
                Constraint::Length(2),     // Help text
            ])
            .split(f.area());

        self.render_header(f, chunks[0]);
        self.render_meetings_table(f, chunks[1]);
        self.render_message(f, chunks[2], message);
        self.render_help(f, chunks[3]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Today's Meetings ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header_text = vec![
            Line::from(vec![
                Span::styled("Total Meetings: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::raw(self.meetings.len().to_string()),
            ]),
        ];

        let paragraph = Paragraph::new(header_text);
        f.render_widget(paragraph, inner);
    }

    fn render_meetings_table(&mut self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL);
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        // Update viewport height
        self.viewport_height = inner.height.saturating_sub(2) as usize;

        // Create table headers
        let header_cells = ["", "Day", "Time", "Meeting Name", "Status", "URL"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
        let header = Row::new(header_cells).height(1);

        // Create table rows
        let now = chrono::Utc::now().with_timezone(&New_York);
        let rows: Vec<Row> = self.meetings
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(self.viewport_height)
            .map(|(idx, meeting)| {
                let day_str = if meeting.start_time.date_naive() == now.date_naive() {
                    "Today".to_string()
                } else if meeting.start_time.date_naive() == (now + chrono::Duration::days(1)).date_naive() {
                    "Tomorrow".to_string()
                } else {
                    meeting.start_time.format("%a %b %d").to_string()
                };
                
                let time_str = format!(
                    "{}:{:02} {} - {}:{:02} {}",
                    meeting.start_time.format("%l").to_string().trim(),
                    meeting.start_time.minute(),
                    meeting.start_time.format("%p"),
                    meeting.end_time.format("%l").to_string().trim(),
                    meeting.end_time.minute(),
                    meeting.end_time.format("%p")
                );
                
                let status = if now >= meeting.start_time && now <= meeting.end_time {
                    "In Progress"
                } else if now < meeting.start_time {
                    "Upcoming"
                } else {
                    "Ended"
                };
                
                let status_color = match status {
                    "In Progress" => Color::Green,
                    "Upcoming" => Color::Cyan,
                    _ => Color::DarkGray,
                };
                
                let url_status = if meeting.meeting_url.is_some() {
                    "Available"
                } else {
                    "Not Available"
                };
                
                // Selection indicator
                let indicator = if idx + self.scroll_offset == self.selected_index { "➤" } else { "" };
                
                let cells = vec![
                    Cell::from(indicator).style(Style::default().fg(Color::Green)),
                    Cell::from(day_str),
                    Cell::from(time_str),
                    Cell::from(meeting.summary.clone()),
                    Cell::from(status).style(Style::default().fg(status_color)),
                    Cell::from(url_status),
                ];
                
                Row::new(cells)
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(3),   // Arrow indicator
                Constraint::Length(12),  // Day
                Constraint::Length(15),  // Time
                Constraint::Min(30),     // Meeting Name
                Constraint::Length(12),  // Status
                Constraint::Length(15),  // URL
            ],
        )
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD));

        f.render_widget(table, inner);
    }

    fn render_message(&self, f: &mut Frame, area: Rect, message: &Option<String>) {
        if let Some(msg) = message {
            let paragraph = Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center);
            f.render_widget(paragraph, area);
        }
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        let help_text = "↑/↓: Navigate | j: Join Meeting | q/ESC: Quit";
        
        let help = Paragraph::new(help_text)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}