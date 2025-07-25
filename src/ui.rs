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

use crate::jira::JiraIssue;

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
        // Calculate the needed height for the header based on summary length
        // Estimate wrapped lines: summary length / (terminal width - margins - label width)
        let available_width = f.area().width.saturating_sub(20) as usize; // 20 for margins and "Summary: "
        let summary_lines = (issue.fields.summary.len() / available_width.max(1)) + 1;
        let header_height = 6 + summary_lines as u16; // 6 = borders(2) + ticket(1) + status(1) + summary label(1) + padding(1)
        
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(header_height.min(20)), // Header info (exact height needed, max 20)
                Constraint::Min(0),                        // Description (gets all remaining space)
                Constraint::Length(2),                     // Help text at bottom
            ])
            .split(f.area());

        // Header block with ticket info
        self.render_header(f, chunks[0], issue);

        // Description block
        self.render_description(f, chunks[1], &issue.fields.description);

        // Help text
        let help = Paragraph::new("Press 'q' or ESC to quit, ↑/↓ to scroll")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        f.render_widget(help, chunks[2]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect, issue: &JiraIssue) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" JIRA Issue Details ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        // Create layout with separate lines for each field
        let content_area = Layout::default()
            .direction(Direction::Vertical)
            .margin(0)
            .constraints([
                Constraint::Length(1),  // Ticket
                Constraint::Length(1),  // Status
                Constraint::Min(1),     // Summary (can wrap)
            ])
            .split(inner);

        // Ticket line
        let ticket_line = vec![
            Span::styled("Ticket: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(&issue.key),
        ];
        f.render_widget(Paragraph::new(Line::from(ticket_line)), content_area[0]);

        // Status line
        let status_line = vec![
            Span::styled("Status: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(&issue.fields.status.name),
        ];
        f.render_widget(Paragraph::new(Line::from(status_line)), content_area[1]);

        // Summary with wrapping
        let summary = vec![
            Span::styled("Summary: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(&issue.fields.summary),
        ];
        let summary_paragraph = Paragraph::new(Line::from(summary))
            .wrap(Wrap { trim: true });
        f.render_widget(summary_paragraph, content_area[2]);
    }

    fn render_description(&self, f: &mut Frame, area: Rect, description: &Option<Value>) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Description ")
            .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD));
        
        let inner = block.inner(area);
        f.render_widget(block, area);

        if let Some(desc) = description {
            self.render_description_content(f, inner, desc);
        } else {
            let text = Paragraph::new("(No description provided)")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(text, inner);
        }
    }

    fn render_description_content(&self, f: &mut Frame, area: Rect, value: &Value) {
        let mut current_y = 0;
        let mut remaining_area = area;

        if let Some(content) = value.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if remaining_area.height == 0 {
                    break;
                }

                let block_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                
                match block_type {
                    "paragraph" => {
                        if let Some(text) = Self::extract_text_content(item) {
                            let paragraph = Paragraph::new(text)
                                .wrap(Wrap { trim: true });
                            let height = 3.min(remaining_area.height); // Estimate height
                            f.render_widget(paragraph, Rect { height, ..remaining_area });
                            
                            current_y += height + 1;
                            if current_y < area.height {
                                remaining_area.y += height + 1;
                                remaining_area.height = remaining_area.height.saturating_sub(height + 1);
                            }
                        }
                    }
                    "heading" => {
                        if let Some(text) = Self::extract_text_content(item) {
                            let level = item.get("attrs")
                                .and_then(|a| a.get("level"))
                                .and_then(|l| l.as_u64())
                                .unwrap_or(1);
                            
                            let style = Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD);
                            
                            let heading = Paragraph::new(format!("{} {}", "#".repeat(level as usize), text))
                                .style(style);
                            
                            let height = 2.min(remaining_area.height);
                            f.render_widget(heading, Rect { height, ..remaining_area });
                            
                            current_y += height;
                            if current_y < area.height {
                                remaining_area.y += height;
                                remaining_area.height = remaining_area.height.saturating_sub(height);
                            }
                        }
                    }
                    "table" => {
                        // Pass the available width to create_table_widget
                        if let Some((table_widget, height)) = Self::create_table_widget(item, remaining_area.width) {
                            let table_height = height.min(remaining_area.height);
                            f.render_widget(table_widget, Rect { height: table_height, ..remaining_area });
                            
                            current_y += table_height + 1;
                            if current_y < area.height {
                                remaining_area.y += table_height + 1;
                                remaining_area.height = remaining_area.height.saturating_sub(table_height + 1);
                            }
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


    fn create_table_widget(item: &Value, available_width: u16) -> Option<(Table, u16)> {
        if let Some(rows) = item.get("content").and_then(|c| c.as_array()) {
            let mut all_cells: Vec<Vec<String>> = Vec::new();
            let mut is_header_row = true;
            
            // First pass: collect all cell content
            for row in rows {
                if row.get("type").and_then(|t| t.as_str()) == Some("tableRow") {
                    if let Some(cells) = row.get("content").and_then(|c| c.as_array()) {
                        let mut row_content = Vec::new();
                        for cell in cells {
                            let cell_text = Self::extract_cell_text(cell).unwrap_or_default();
                            row_content.push(cell_text);
                        }
                        all_cells.push(row_content);
                    }
                }
            }

            if all_cells.is_empty() {
                return None;
            }

            let num_cols = all_cells[0].len();
            if num_cols == 0 {
                return None;
            }

            // Calculate column widths based on available terminal width
            // Account for borders: 1 char per column separator + 2 for outer borders
            // Also account for padding: 1 space on each side of content per column
            let separators = if num_cols > 0 { num_cols - 1 } else { 0 };
            let border_width = (separators + 2) as u16; // separators + left/right borders
            let padding_width = (num_cols * 2) as u16; // 2 spaces padding per column
            let total_overhead = border_width + padding_width;
            let usable_width = available_width.saturating_sub(total_overhead) as usize;
            let min_col_width = 5;
            
            // Calculate the actual content length and ideal width for each column
            let mut content_lengths: Vec<usize> = vec![0; num_cols];
            let mut ideal_widths: Vec<usize> = vec![0; num_cols];
            let mut min_widths: Vec<usize> = vec![min_col_width; num_cols];
            
            for row in &all_cells {
                for (i, cell) in row.iter().enumerate() {
                    if i < content_lengths.len() {
                        // Track the maximum content length for this column
                        content_lengths[i] = content_lengths[i].max(cell.len());
                        
                        // Calculate minimum width needed (based on longest word)
                        let longest_word = cell.split_whitespace()
                            .map(|w| w.len())
                            .max()
                            .unwrap_or(0);
                        min_widths[i] = min_widths[i].max(longest_word);
                        
                        // Ideal width for readability (capped at reasonable length)
                        ideal_widths[i] = ideal_widths[i].max(cell.len().min(50));
                    }
                }
            }

            // Calculate total ideal width
            let total_ideal_width: usize = ideal_widths.iter().sum();
            
            let col_widths: Vec<usize> = if total_ideal_width <= usable_width {
                // If ideal widths fit, use them
                ideal_widths.iter()
                    .map(|&w| w.max(min_col_width))
                    .collect()
            } else {
                // Need to distribute space - prioritize columns with more content
                let total_min_width: usize = min_widths.iter().sum();
                
                if total_min_width > usable_width {
                    // Even minimum widths don't fit - scale down proportionally
                    let scale_factor = usable_width as f64 / total_min_width as f64;
                    min_widths.iter()
                        .map(|&w| ((w as f64 * scale_factor) as usize).max(min_col_width))
                        .collect()
                } else {
                    // We have extra space to distribute after meeting minimums
                    let extra_space = usable_width - total_min_width;
                    
                    // Calculate weights based on content length
                    let total_content_length: usize = content_lengths.iter().sum::<usize>().max(1);
                    
                    min_widths.iter().enumerate()
                        .map(|(i, &min_w)| {
                            // Give extra space proportional to content length
                            let content_ratio = content_lengths[i] as f64 / total_content_length as f64;
                            let extra_for_column = (extra_space as f64 * content_ratio) as usize;
                            min_w + extra_for_column
                        })
                        .collect()
                }
            };

            // Create table rows with wrapped text
            let mut table_rows = Vec::new();
            let mut header_row = None;
            let mut total_height = 0u16;

            for (row_idx, row_content) in all_cells.iter().enumerate() {
                // Wrap text in each cell
                let mut wrapped_cells: Vec<Vec<String>> = Vec::new();
                let mut max_lines = 1;

                for (col_idx, cell_text) in row_content.iter().enumerate() {
                    let width = col_widths.get(col_idx).copied().unwrap_or(min_col_width);
                    let wrapped = textwrap::wrap(cell_text, width);
                    let wrapped_lines: Vec<String> = wrapped.iter().map(|cow| cow.to_string()).collect();
                    max_lines = max_lines.max(wrapped_lines.len());
                    wrapped_cells.push(wrapped_lines);
                }

                // Pad cells to have the same number of lines
                for cell_lines in &mut wrapped_cells {
                    while cell_lines.len() < max_lines {
                        cell_lines.push(String::new());
                    }
                }

                // Create table row with proper height
                let mut row_cells = Vec::new();
                for (_col_idx, cell_lines) in wrapped_cells.iter().enumerate() {
                    let cell_text = cell_lines.join("\n");
                    let cell = if row_idx == 0 && is_header_row {
                        Cell::from(cell_text)
                            .style(Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD))
                    } else {
                        Cell::from(cell_text)
                    };
                    row_cells.push(cell);
                }

                let row = Row::new(row_cells).height(max_lines as u16);
                total_height += max_lines as u16;

                if row_idx == 0 && is_header_row {
                    header_row = Some(row);
                    is_header_row = false;
                } else {
                    table_rows.push(row);
                }
            }

            // Ensure we're using all available width
            let total_calculated_width: usize = col_widths.iter().sum();
            
            // If we have extra space, distribute it to columns proportionally to their content
            let final_widths = if total_calculated_width < usable_width {
                let extra_space = usable_width - total_calculated_width;
                let mut adjusted_widths = col_widths.clone();
                
                // Distribute extra space proportionally to content length
                let total_content_length: usize = content_lengths.iter().sum::<usize>().max(1);
                let mut distributed = 0;
                
                for (i, width) in adjusted_widths.iter_mut().enumerate() {
                    if i < content_lengths.len() {
                        let content_ratio = content_lengths[i] as f64 / total_content_length as f64;
                        let extra_for_column = (extra_space as f64 * content_ratio) as usize;
                        *width += extra_for_column;
                        distributed += extra_for_column;
                    }
                }
                
                // Add any remaining space due to rounding to the longest column
                if distributed < extra_space {
                    if let Some((max_col_idx, _)) = content_lengths.iter().enumerate().max_by_key(|&(_, len)| *len) {
                        adjusted_widths[max_col_idx] += extra_space - distributed;
                    }
                }
                
                adjusted_widths
            } else {
                col_widths
            };
            
            // Create constraints based on final column widths
            let constraints: Vec<Constraint> = final_widths.iter()
                .map(|&w| Constraint::Length(w as u16))
                .collect();

            let mut table = Table::new(table_rows, &constraints)
                .style(Style::default())
                .block(Block::default().borders(Borders::ALL));

            if let Some(header) = header_row {
                table = table.header(header);
            }

            // Total height includes borders and spacing
            let height = total_height + 4; // +4 for borders and spacing
            Some((table, height))
        } else {
            None
        }
    }

    fn extract_text_content(item: &Value) -> Option<String> {
        if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
            let mut texts = Vec::new();
            
            for content_item in content {
                if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                    texts.push(text.to_string());
                }
            }
            
            if !texts.is_empty() {
                Some(texts.join(""))
            } else {
                None
            }
        } else {
            None
        }
    }

    fn extract_cell_text(cell: &Value) -> Option<String> {
        if let Some(content) = cell.get("content").and_then(|c| c.as_array()) {
            let mut texts = Vec::new();
            
            for item in content {
                if let Some(text) = Self::extract_text_content(item) {
                    texts.push(text);
                }
            }
            
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