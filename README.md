# qq - Personal CLI Assistant

A personal CLI tool for day-to-day development tasks. Currently includes JIRA integration with git branches, with more features planned.

## Installation

```bash
cargo install --path .
```

## Features

### JIRA Integration

Automatically extracts JIRA ticket IDs from git branch names and provides quick access to ticket operations.

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
qq jira get
```

##### Add a comment
```bash
qq jira comment "Updated the implementation as discussed"
```

##### Close the ticket
```bash
qq jira close
```

##### Assign ticket to yourself
```bash
qq jira assign
```

##### Pick up a ticket (assign to yourself and move to In Progress)
```bash
qq jira pickup
```

##### Start working on a new ticket
Creates a new feature branch, assigns the ticket to yourself, and moves it to In Progress:
```bash
qq jira start PROJ-123
```
This will create and switch to a branch named `feature/PROJ-123`.

## Examples

```bash
# On branch "feature/PROJ-123-add-authentication"
$ qq jira get

Fetching details for ticket: PROJ-123

Ticket: PROJ-123
Summary: Add user authentication
Status: In Progress

Description:
Implement OAuth2 authentication for the application...

$ qq jira comment "Authentication module completed, ready for review"
Adding comment to ticket: PROJ-123
Comment added successfully!

$ qq jira close
Closing ticket: PROJ-123
Ticket closed successfully!
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