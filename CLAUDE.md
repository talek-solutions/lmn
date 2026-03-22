# Workflow
- Always provide a plan for the changes you want to make and only then you can eddit
- Accept edits is enabled by default
- You are FORBIDDEN from executing any edits, until you have presented the plan to me first and I have explicitly APPROVED it
- This rule applies to EVERY task, including follow-up instructions and continuations — approval from a previous task does NOT carry over

# System
- You SHOULD propose better practices or other improvements, after you reason exactly why they might be needed
- NEVER auto implement such suggestions, without reasonining them and my explicit approval

# Agents
- Agents must never work directly on master, without explicit approval
- Agents should work on the same local feature branch that is the currently checked out branch, unless asked otherwise by the user


# Plan
- Every plan you show, MUST contain very visibly which files in the entire structure you will change
- In the case the changes are too many, you can show how many files you will create, update and delete, with their respective folders
- ALWAYS wait for explicit approval (e.g. "yes", "go ahead") before making any edits — do not assume approval from context or prior messages

# Rust
- Prefer using structs for parameters close by function, so they are more extendable and flexible

# Commands
- You are ALLOWED by default to execute local cargo build, cat and some other commands, which you need to validate the work

# Project
- The project MAY be used in varying ways (CLI, webserver etc.), therefore separation in domains is needed to ensure code resuability

# Git
- Single line commit messages, following pattern like feat: ***, chore: ***, fix: ****
- ALWAYS exclude your contribution in the commit
- ALWAYS request approval for commit
- You are FORBIDDEN to perform git push

# CLI
- EVERY time you make a change to the CLI contract (flags, subcommands, aliases, conflicts), ALWAYS update CLI.md accordingly

# Templates
- Use the TEMPLATES.md file to gain information about the templating functionality
- EVERY time you make a structural change to template placeholder, or a change to the strategy, ALWAYS update TEMPLATES.md and the actual template example accordingly 

# Tests
- ALWAYS write tests for tht functionalities you change or add
- You are FORBIDDEN from deleting tests for making them pass
- The only way you can delete a test is with you asking for my explicit approval on the specific test and file