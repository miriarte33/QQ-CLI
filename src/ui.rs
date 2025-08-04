use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Terminal, Frame,
};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io;
use serde_json::Value;

use crate::jira::{JiraIssue, JiraClient};

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
}

impl EpicListDisplay {
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
                        }
                    }
                    KeyCode::Down => {
                        if app.selected_index < app.children.len().saturating_sub(1) {
                            app.selected_index += 1;
                        }
                    }
                    KeyCode::Char('a') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            message = Some(format!("Assigning {} to yourself...", issue.key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            match client.assign_issue(&issue.key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} assigned to you", issue.key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue.key) {
                                        app.children[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to assign {}: {}", issue.key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            message = Some(format!("Picking up {}...", issue.key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            match client.pickup_issue(&issue.key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} assigned and moved to In Progress", issue.key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue.key) {
                                        app.children[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to pickup {}: {}", issue.key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            message = Some(format!("Starting {}...", issue.key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            // Create feature branch
                            use git2::Repository;
                            match Repository::open(".") {
                                Ok(repo) => {
                                    let branch_name = format!("feature/{}", issue.key);
                                    
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
                                                        match client.pickup_issue(&issue.key) {
                                                            Ok(_) => {
                                                                message = Some(format!("✓ Created branch '{}' and picked up {}", branch_name, issue.key));
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
                            // Temporarily restore terminal for nested UI
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            
                            // Show the issue details
                            println!("Viewing issue: {}", issue.key);
                            let _ = JiraIssueDisplay::show(issue);
                            
                            // Re-setup terminal for epic list
                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;
                            
                            message = Some(format!("Returned from viewing {}", issue.key));
                        }
                    }
                    KeyCode::Char('c') => {
                        if let Some(issue) = app.children.get(app.selected_index) {
                            message = Some(format!("Closing {}...", issue.key));
                            terminal.draw(|f| app.draw(f, epic, &message))?;
                            
                            match client.close_issue(&issue.key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} closed successfully", issue.key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue.key) {
                                        app.children[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to close {}: {}", issue.key, e));
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

    fn draw(&self, f: &mut Frame, epic: &JiraIssue, message: &Option<String>) {
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

    fn render_children_table(&self, f: &mut Frame, area: Rect) {
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
        let header_cells: Vec<Cell> = vec!["Key", "Status", "Summary", "Assignee"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .collect();
        let header = Row::new(header_cells).height(1);

        // Create table rows
        let rows: Vec<Row> = self.children.iter().enumerate().map(|(idx, issue)| {
            let assignee = issue.fields.assignee.as_ref()
                .map(|u| u.display_name.clone())
                .unwrap_or_else(|| "Unassigned".to_string());
            
            let style = if idx == self.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            
            // Color code the status
            let status_style = match issue.fields.status.name.to_lowercase().as_str() {
                s if s.contains("done") || s.contains("closed") => Style::default().fg(Color::Green),
                s if s.contains("progress") => Style::default().fg(Color::Yellow),
                s if s.contains("review") => Style::default().fg(Color::Magenta),
                _ => Style::default().fg(Color::White),
            };
            
            let cells = vec![
                Cell::from(issue.key.clone()).style(style),
                Cell::from(issue.fields.status.name.clone()).style(style.patch(status_style)),
                Cell::from(issue.fields.summary.clone()).style(style),
                Cell::from(assignee).style(style),
            ];
            
            Row::new(cells).height(1)
        }).collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(12),     // Key
                Constraint::Length(15),     // Status
                Constraint::Min(20),        // Summary (takes remaining space)
                Constraint::Length(20),     // Assignee
            ]
        )
        .header(header)
        .block(Block::default());

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
        let help = Paragraph::new("↑/↓: Navigate | v: View | a: Assign | p: Pickup | c: Close | s: Start | q/ESC: Quit")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}

pub struct MyIssuesDisplay {
    selected_index: usize,
    issues: Vec<JiraIssue>,
}

impl MyIssuesDisplay {
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
                        }
                    }
                    KeyCode::Down => {
                        if app.selected_index < app.issues.len().saturating_sub(1) {
                            app.selected_index += 1;
                        }
                    }
                    KeyCode::Char('v') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            // Temporarily restore terminal for nested UI
                            disable_raw_mode()?;
                            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                            terminal.show_cursor()?;
                            
                            // Show the issue details
                            println!("Viewing issue: {}", issue.key);
                            let _ = JiraIssueDisplay::show(issue);
                            
                            // Re-setup terminal
                            enable_raw_mode()?;
                            let mut stdout = io::stdout();
                            execute!(stdout, EnterAlternateScreen)?;
                            let backend = CrosstermBackend::new(stdout);
                            terminal = Terminal::new(backend)?;
                            
                            message = Some(format!("Returned from viewing {}", issue.key));
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            message = Some(format!("Moving {} to In Progress...", issue.key));
                            terminal.draw(|f| app.draw(f, &message))?;
                            
                            match client.transition_to_in_progress(&issue.key) {
                                Ok(_) => {
                                    message = Some(format!("✓ {} moved to In Progress", issue.key));
                                    // Refresh the issue data
                                    if let Ok(updated_issue) = client.get_issue(&issue.key) {
                                        app.issues[app.selected_index] = updated_issue;
                                    }
                                }
                                Err(e) => {
                                    message = Some(format!("✗ Failed to move {}: {}", issue.key, e));
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            message = Some(format!("Starting {}...", issue.key));
                            terminal.draw(|f| app.draw(f, &message))?;
                            
                            // Create feature branch
                            use git2::Repository;
                            match Repository::open(".") {
                                Ok(repo) => {
                                    let branch_name = format!("feature/{}", issue.key);
                                    
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
                                                        match client.pickup_issue(&issue.key) {
                                                            Ok(_) => {
                                                                message = Some(format!("✓ Created branch '{}' and picked up {}", branch_name, issue.key));
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
                    KeyCode::Char('e') => {
                        if let Some(issue) = app.issues.get(app.selected_index) {
                            if let Some(parent) = &issue.fields.parent {
                                // Temporarily restore terminal
                                disable_raw_mode()?;
                                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                                terminal.show_cursor()?;
                                
                                // Show the parent epic
                                println!("Fetching epic details for: {}", parent.key);
                                if let Ok(children) = client.get_epic_children(&parent.key) {
                                    println!("Fetching child issues...");
                                    let _ = EpicListDisplay::show(parent, children, client);
                                }
                                
                                // Re-setup terminal
                                enable_raw_mode()?;
                                let mut stdout = io::stdout();
                                execute!(stdout, EnterAlternateScreen)?;
                                let backend = CrosstermBackend::new(stdout);
                                terminal = Terminal::new(backend)?;
                                
                                message = Some(format!("Returned from viewing epic {}", parent.key));
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

    fn draw(&self, f: &mut Frame, message: &Option<String>) {
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

    fn render_issues_table(&self, f: &mut Frame, area: Rect) {
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
        let header_cells: Vec<Cell> = vec!["Key", "Parent", "Status", "Summary"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
            .collect();
        let header = Row::new(header_cells).height(1);

        // Create table rows
        let rows: Vec<Row> = self.issues.iter().enumerate().map(|(idx, issue)| {
            let parent = issue.fields.parent.as_ref()
                .map(|p| p.key.clone())
                .unwrap_or_else(|| "—".to_string());
            
            let style = if idx == self.selected_index {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            
            // Color code the status
            let status_style = match issue.fields.status.name.to_lowercase().as_str() {
                s if s.contains("done") || s.contains("closed") => Style::default().fg(Color::Green),
                s if s.contains("progress") => Style::default().fg(Color::Yellow),
                s if s.contains("review") => Style::default().fg(Color::Magenta),
                _ => Style::default().fg(Color::White),
            };
            
            let cells = vec![
                Cell::from(issue.key.clone()).style(style),
                Cell::from(parent).style(style),
                Cell::from(issue.fields.status.name.clone()).style(style.patch(status_style)),
                Cell::from(issue.fields.summary.clone()).style(style),
            ];
            
            Row::new(cells).height(1)
        }).collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Length(12),     // Key
                Constraint::Length(12),     // Parent
                Constraint::Length(15),     // Status
                Constraint::Min(20),        // Summary (takes remaining space)
            ]
        )
        .header(header)
        .block(Block::default());

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
        let help = Paragraph::new("↑/↓: Navigate | v: View | e: Epic | p: In Progress | s: Start | q/ESC: Quit")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, area);
    }
}