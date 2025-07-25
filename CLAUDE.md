# qq CLI - Project Patterns and Conventions

This document captures the patterns and conventions used in the qq CLI project to help maintain consistency in future development sessions.

## Project Overview

`qq` is a personal CLI assistant tool written in Rust, currently focused on JIRA integration with git branches. The project is designed to be extensible for future personal productivity features.

## Project Structure

```
qq/
├── Cargo.toml          # Project manifest (Rust edition 2024)
├── README.md           # User documentation
├── src/
│   ├── main.rs         # CLI entry point and command handling
│   ├── config.rs       # Configuration management
│   ├── jira.rs         # JIRA API client implementation
│   └── ui.rs           # Terminal UI components using ratatui
└── target/             # Build artifacts (gitignored)
```

## Key Dependencies and Their Usage

- **clap** (4.5): Command-line argument parsing with derive macros
- **reqwest** (0.12): HTTP client for JIRA API calls (blocking feature)
- **serde** (1.0): JSON serialization/deserialization with derive
- **serde_json** (1.0): JSON handling
- **git2** (0.19): Git repository operations
- **regex** (1.11): Pattern matching for JIRA ticket extraction
- **base64** (0.22): Basic auth encoding
- **dirs** (5.0): Platform-specific directory paths
- **toml** (0.8): Configuration file format
- **anyhow** (1.0): Error handling with context
- **ratatui** (0.29): Terminal UI framework for rich display
- **crossterm** (0.28): Cross-platform terminal manipulation

## Code Patterns and Conventions

### Error Handling
- Uses `anyhow::Result` throughout for error propagation
- Errors include context using `.context()` method
- Functions return `Result<T>` or `Result<()>`

### Module Organization
- Each major feature gets its own module (config, jira)
- Public API exposed through `pub` structs and methods
- Internal helpers remain private

### CLI Structure (using clap)
- Main `Cli` struct with subcommands
- Nested subcommands for feature organization (e.g., `qq jira get`)
- Derive macros for automatic parsing
- Descriptive help text using `#[command(about = "...")]`

### Configuration Management
- Config stored in platform-specific directory: `~/.config/qq/config.toml`
- TOML format for human readability
- Serialize/deserialize using serde

### JIRA Integration Patterns
- Branch name patterns supported:
  - `PROJ-123`
  - `feature/PROJ-123-description`
  - `bugfix/PROJ-123-description`
  - `hotfix/PROJ-123`
- Regex-based ticket extraction with fallback patterns
- JIRA API v3 with JSON responses
- Basic authentication with API tokens

### API Client Design
- Struct-based client with configuration
- Blocking HTTP requests (not async)
- Structured request/response types with serde
- Error responses logged before failing

### Code Style
- Standard Rust naming conventions (snake_case functions, PascalCase types)
- Explicit imports at function level for clarity
- Early returns with `?` operator
- Match expressions for command handling

### UI Design (ratatui)
- Full-screen terminal UI for JIRA ticket display
- Scrollable content with keyboard navigation (↑/↓ keys)
- Proper table widgets for structured data
- Text wrapping for long content
- Color-coded elements (headers, status, etc.)
- Press 'q' or ESC to exit

## Testing and Building

- Standard `cargo` commands work as expected
- `cargo check` - Type checking
- `cargo build` - Debug build
- `cargo install --path .` - Install locally

## Future Extension Points

The codebase is structured to easily add new features:
1. Add new top-level commands in `Commands` enum
2. Create new modules for feature implementations
3. Extend config structure for new service credentials
4. Follow existing patterns for consistency

## Common Tasks

When adding new features:
1. Define command structure in main.rs
2. Create module file for implementation
3. Add configuration fields if needed
4. Update README with usage examples
5. Use existing error handling patterns
6. Follow JIRA module as reference implementation

## UI Development Guidelines (ratatui)

### Key Principles
- **Simplicity over complexity**: Avoid over-engineering UI components. The original table rendering was 468 lines; the refactored version is ~300 lines with better maintainability
- **Use framework features properly**: Always prefer ratatui's built-in widgets (Table, Cell, etc.) over manual rendering
- **Window-aware rendering**: Use the actual terminal dimensions from the Frame, not hardcoded values

### Table Rendering Best Practices
1. **Use Table widget for tabular data**: Don't try to manually render tables with text
2. **Dynamic width calculation**: Calculate column widths based on `available_width` from the rendering area
3. **Account for margins properly**:
   - Column separators: 1 char between columns
   - Outer borders: 2 chars total
   - Cell padding: 2 spaces per column
4. **Text wrapping**: Use textwrap crate with calculated column widths: `(available_width - margins) / num_columns`
5. **Equal-width columns**: Use `Constraint::Percentage(100 / num_cols)` for simple equal distribution

### Example Pattern for Mixed Content Rendering
```rust
// Render different content types in a scrollable area
fn render_jira_description(&self, f: &mut Frame, area: Rect, value: &Value) {
    let mut remaining_area = area;
    
    for item in content {
        match block_type {
            "paragraph" => { /* render with Paragraph widget */ }
            "heading" => { /* render with styled Paragraph */ }
            "table" => { /* render with Table widget */ }
        }
        // Update remaining_area after each element
    }
}
```

### Refactoring Lessons
- **Balance abstraction levels**: Use high-level widgets while keeping implementation straightforward
- **Iterative refinement**: Start simple, then add complexity only where needed
- **Don't reinvent the wheel**: ratatui and textwrap already solve text layout problems well
- **Responsive design**: Tables and text should adapt to terminal width automatically