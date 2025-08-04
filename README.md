# qq - Personal CLI Assistant

A personal CLI tool for day-to-day development tasks. Currently includes JIRA integration with git branches, with more features planned.

## Installation

```bash
cargo install --path .
```

## Features

### JIRA Integration

Automatically extracts JIRA ticket IDs from git branch names and provides quick access to ticket operations. Includes powerful interactive views for managing epics and your assigned tickets.

#### Configuration

First, configure your JIRA credentials:

```bash
qq config jira --url https://yourcompany.atlassian.net --username your-email@company.com --token your-api-token
```

To get a JIRA API token:
1. Go to https://id.atlassian.com/manage-profile/security/api-tokens
2. Create a new API token
3. Use this token in the configuration

#### Commands

The JIRA commands automatically detect the ticket ID from your current git branch. Supported branch formats:
- `PROJ-123`
- `feature/PROJ-123-some-description`
- `bugfix/PROJ-123-fix-issue`
- `hotfix/PROJ-123`

##### Get ticket details
```bash
qq jira get         # Shows ticket details in a rich UI
qq jira get parent  # Shows the parent epic with all its children
```

##### View and manage epics
Interactive view for managing all tickets in an epic:
```bash
qq jira epic EPIC-123
```

##### View your assigned tickets
Interactive view showing all tickets assigned to you (excluding Done):
```bash
qq jira mine
```

##### Add a comment
```bash
qq jira comment "Updated the implementation as discussed"
```

##### Close the ticket
```bash
qq jira close
```

##### Start working on a new ticket
Creates a new feature branch, assigns the ticket to yourself, and moves it to In Progress:
```bash
qq jira start PROJ-123
```
This will create and switch to a branch named `feature/PROJ-123`.

#### Interactive Views

The `epic` and `mine` commands provide interactive terminal UIs with the following keyboard shortcuts:

##### Common Controls
- `↑/↓` - Navigate through the list
- `v` - View the selected ticket details
- `p` - Move ticket to In Progress
- `c` - Close the selected ticket
- `s` - Start working on ticket (creates branch, assigns to you, moves to In Progress)
- `q` or `ESC` - Quit the view

##### Epic View Controls
- `a` - Assign ticket (opens user selection)
  - In user selection:
    - `↑/↓` - Navigate users
    - `/` - Search for users
    - `Enter` - Select user
    - `ESC` - Cancel

##### Mine View Controls
- `e` - View the parent epic (if ticket has one)

#### Features

- **Rich Terminal UI**: All interactive views use a modern terminal UI with colors and proper formatting
- **Real-time Updates**: After actions, the ticket data refreshes automatically
- **Scrollable Lists**: Long lists of tickets or users scroll smoothly
- **Search Functionality**: The assignee selector includes search to filter users
- **Unassign Option**: When assigning, you can select "None" to unassign a ticket

## Examples

### Basic Commands
```bash
# On branch "feature/PROJ-123-add-authentication"
$ qq jira get

# Opens a rich UI showing ticket details including description, status, and assignee

$ qq jira get parent

# If PROJ-123 is part of an epic, shows the epic with all its child tickets
# Same interactive view as 'qq jira epic EPIC-ID'

$ qq jira comment "Authentication module completed, ready for review"
Adding comment to ticket: PROJ-123
Comment added successfully!

$ qq jira close
Closing ticket: PROJ-123
Ticket closed successfully!
```

### Interactive Epic Management
```bash
$ qq jira epic EPIC-100

# Opens interactive view showing all tickets in the epic
# Use arrow keys to navigate, 'a' to assign tickets, 'p' to move to progress, etc.

Epic: EPIC-100 - Q4 Authentication Features
Child Issues (5):
┌─────────────┬────────────────┬─────────────────────────────────┬──────────────┐
│ Key         │ Status         │ Summary                         │ Assignee     │
├─────────────┼────────────────┼─────────────────────────────────┼──────────────┤
│ PROJ-123 ▶  │ In Progress    │ Add OAuth2 authentication       │ John Doe     │
│ PROJ-124    │ To Do          │ Implement password reset        │ Unassigned   │
│ PROJ-125    │ In Review      │ Add two-factor authentication   │ Jane Smith   │
└─────────────┴────────────────┴─────────────────────────────────┴──────────────┘
```

### Managing Your Tickets
```bash
$ qq jira mine

# Shows all tickets assigned to you in an interactive view
# Press 'e' to view the parent epic, 'v' to view details, 's' to start working

My Issues:
┌─────────────┬─────────────┬────────────────┬─────────────────────────────────┐
│ Key         │ Parent      │ Status         │ Summary                         │
├─────────────┼─────────────┼────────────────┼─────────────────────────────────┤
│ PROJ-123 ▶  │ EPIC-100    │ In Progress    │ Add OAuth2 authentication       │
│ PROJ-127    │ EPIC-101    │ To Do          │ Update API documentation        │
│ PROJ-130    │ —           │ In Review      │ Fix login redirect issue        │
└─────────────┴─────────────┴────────────────┴─────────────────────────────────┘
```

## Future Features

The `qq` CLI is designed to be extensible. Future additions may include:
- GitHub/GitLab integration
- Time tracking
- Note-taking and knowledge management
- Quick calculations and conversions
- And more personal productivity tools

## Contributing

This is a personal tool, but suggestions and contributions are welcome!