# Workflow
- Accept edits is enabled by default
- You are FORBIDDEN from executing any edits, until you have presented the plan to me first and I have explicitly APPROVED it

# Plan
- Every plan you show to me, just contain very visibly which files in the entire structure you must change
- In the case the changes are too many, you can show how many files you will create, update and delete, with their respective folders

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

# Templates
- The templates are a tool to add variety and randomness to testing, where the request body for POST, PUT, PATCH requests can vary
- If "_loadtest_metadata_templates" property is available at root level in the user template, the final template should render without it, as it contains only informational metadata for the loadtest tool generator
- "_loadtest_metadata_templates" is the property the user will need to introduce in their json at the root level in order to use the final JSON rendering functionality
- Use the .templates.example/**/placeholder.** file to extract examples of data shape and functionality
- In case the appointed "_loadtest_metadata_templates" key is missing, the template is to be taken at face value and re-used on every request
- The usual format of a placeholder value is "{{name_string}}", but it is possible to affect the render behaviour, if the user defines "once" behaviour, where the value is to be generated once, and re-used for all requests with this temaplate
