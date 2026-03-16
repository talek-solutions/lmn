# Workflow
- Always provide a plan for the changes you want to make and only then you can eddit
- Accept edits is enabled by default
- You are FORBIDDEN from executing any edits, until you have presented the plan to me first and I have explicitly APPROVED it
- This rule applies to EVERY task, including follow-up instructions and continuations — approval from a previous task does NOT carry over

# System
- You CAN/SHOULD propose better practices or other improvements, after you reason exactly why they might be needed
- You should NEVER auto implement such suggestions, without reasonining them and my explicit approval

# Agents
- The senior-rust-dev agent is to implement code changes as per the request of the user or the tech-lead or senior-product-manager (only after tech lead or user approval)
- The tech lead owns implementing and keeping up-to-date documentation of the features he owns in .docs directory, creating and maintaining the TECH.md file
- The senior product manage is to create BRD and PRD documents in .docs/initiative-name/ directory and keep them up-to-date. The PdM works with the technical lead to produce and maintain .docs/BACKLOG.md, as the list and reasoning (short) of the next 5 features that need to be implemented
- The tech lead signs off on all initiative maintaing balance between velocity, preventing tech debt and informing the user in a detailed way of the reasoning and technical decisions
- The tech lead reviews all code of the senior-rust-dev once the agent is done
- The security reviewer reviews the feature code in parallel with the tech-lead review
- A second senior-rust-dev can be spawned to work in parallel with the first one to acoomplish the requirements by the user or the tech-lead


# Plan
- Every plan you show to me, just contain very visibly which files in the entire structure you must change
- In the case the changes are too many, you can show how many files you will create, update and delete, with their respective folders
- ALWAYS wait for explicit approval (e.g. "yes", "go ahead") before making any edits — do not assume approval from context or prior messages

# Rust
- Prefer using structs for parameters close by function, so they are more extendable and flexible

# Commands
- You are ALLOWED by default to execute local cargo build, cat and some other commands, which you need to validate the work

# Project
- The project MAY be used in varying ways (CLI, webserver etc.), therefore separation in domains is needed to ensure code resuability
- Examples of templates can be found in .templates.example, where will be templates for all supported data format


# Git
- If I ask you to commit changes, opt-in for single line commit messages, which ALWAYS exclude your contribution
- Use prefixes like: refactor:, feat:, fix:, chore: in the commit messages
- ALWAYS request approval for commit

# CLI
- EVERY time you make a change to the CLI contract (flags, subcommands, aliases, conflicts), ALWAYS update CLI.md accordingly

# Templates
- You are FREE to read the contents of the .templates.example folder, ALWAYS
- Use the TEMPLATES.md file to gain information about the templating functionality
- EVERY time you make a structural change to template placeholder, or a change to the strategy, ALWAYS update TEMPLATES.md and the actual template example accordingly 

# Tests
- You are to ALWAYS write tests for tht functionalities you change or add
- You are FORBIDDEN from deleting tests for making them pass
- The only way you can delete a test is with you asking for my explicit approval on the specific test and file