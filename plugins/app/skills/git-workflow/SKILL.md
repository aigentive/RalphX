---
name: git-workflow
description: Git commit conventions and workflow
disable-model-invocation: true
user-invocable: false
---

# Git Workflow

## Commit Message Format

```
<type>: <description>

[optional body]

[optional footer]
```

### Types
- `feat`: New feature
- `fix`: Bug fix
- `refactor`: Code change that neither fixes a bug nor adds a feature
- `test`: Adding or updating tests
- `docs`: Documentation only changes
- `chore`: Changes to build process or auxiliary tools
- `style`: Formatting, missing semi-colons, etc.

### Examples
```
feat: add user authentication
fix: resolve race condition in task loading
refactor: extract validation logic to separate module
test: add integration tests for API endpoints
docs: update README with setup instructions
chore: update dependencies
```

## Atomic Commits

### Principles
- One logical change per commit
- Tests should pass after each commit
- Can be reverted independently
- Easy to understand in isolation

### Signs of Good Atomicity
- Commit message fits in one line
- All files relate to the same change
- No "and" needed to describe the change
- Tests still pass if commit is reverted

### What to Avoid
- "Fix several bugs"
- Mixing refactoring with features
- Incomplete changes
- Unrelated file changes

## Workflow

### Before Committing
1. Review changes: `git diff`
2. Follow the target project's local instructions and run only the focused validation required for the change
3. Run scoped linting when the target project requires it
4. Stage specific files: `git add <files>`

### Commit Checklist
- [ ] Changes are tested
- [ ] Tests pass
- [ ] Lint passes
- [ ] Message follows convention
- [ ] No sensitive data included

## Commands

```bash
# View changes
git status
git diff
git diff --staged

# Stage changes
git add <file>           # Specific file
git add -p              # Interactive staging (avoid -A)

# Commit
git commit -m "type: description"

# View history
git log --oneline -10
git log --stat -3
```

## Safety Rules

### Never
- Force push to main/master
- Commit secrets or credentials
- Commit without the target project's required focused validation
- Use `git add -A` blindly

### Always
- Review staged changes before commit
- Write descriptive messages
- Keep commits atomic
- Run project-required focused checks before committing
