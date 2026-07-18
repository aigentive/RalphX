---
name: task-decomposition
description: Guide for breaking features into atomic, implementable tasks
---

# Task Decomposition Skill

This skill helps break down complex features into atomic, independently implementable tasks.

## Core Principles

### 1. Right-Sized Tasks
Each task should be:
- Completable in 1-4 hours of focused work
- Independently testable
- Shippable on its own (even if feature is incomplete)
- Clear enough that someone else could pick it up

### 2. Vertical Slices
Prefer vertical slices over horizontal layers:

**Good (Vertical)**:
- "Add login button that shows login modal"
- "Implement password validation on login form"
- "Add remember me checkbox to login"

**Avoid (Horizontal)**:
- "Create all auth UI components"
- "Write all auth business logic"
- "Add all auth tests"

### 3. Dependency Minimization
Order tasks to minimize blocking:
1. Foundation tasks (types, interfaces, infrastructure)
2. Core functionality (main features)
3. Enhancements (polish, edge cases)
4. Integration (connecting pieces)

## Decomposition Patterns

### Pattern: Feature -> Tasks

**Feature**: "Dark mode support"
**Decomposition**:
1. Define color tokens (CSS variables)
2. Create theme context
3. Add theme toggle component
4. Update navigation to use theme colors
5. Update cards to use theme colors
6. Update modals to use theme colors
7. Persist theme preference
8. Detect system preference

### Pattern: CRUD -> Tasks

**Feature**: "Task comments"
**Decomposition**:
1. Add comments field to task entity
2. Create comment repository
3. Add create comment command
4. Add list comments command
5. Create comment list component
6. Create comment input component
7. Integrate comments into task detail view

### Pattern: Integration -> Tasks

**Feature**: "Connect to external API"
**Decomposition**:
1. Define API response types
2. Create API client module
3. Implement authentication
4. Add fetch method for resource
5. Create loading states
6. Add error handling
7. Implement retry logic
8. Add caching layer

### Pattern: Targeted Testing

Every plan decomposition MUST include a testing strategy that scales with the implementation — not blanket full-suite runs at every gate.

**Rules:**

- Each implementation task's steps MUST include a test identification step — the worker identifies which test files are affected by the changes in that task before running tests.
- The identification method is tech-stack dependent; workers choose the appropriate approach:
  - JS/TS/Python: grep import statements and module references to find test files that import changed modules
  - Rust: check `mod tests` blocks within changed files and the `tests/` directory for integration tests matching changed module names; examine test file naming conventions
  - Other stacks: examine test file naming conventions and directory structure
- If targeted identification yields no results, fall back to a path-scoped suite (e.g., tests for the directory or crate containing the changed files) rather than the full suite.
- Do not manufacture a standalone broad regression task; each implementation task follows the target project's local validation policy, while any broader gate stays with the target project's existing automation unless the user explicitly requests otherwise.
- Non-code tasks (docs updates, config-only changes) are exempt from the test identification step.

**Example decomposition with targeted testing:**

| Task | Type | Test Step |
|------|------|-----------|
| Add comment repository | feature | Identify test files importing `comment_repository` → run those tests |
| Add comment API endpoint | feature | Identify test files importing comment handler → run those tests |
| Add comment UI component | feature | Identify test files importing `CommentInput` → run those tests |

## Decomposition Questions

Ask these to guide decomposition:

1. **Data first**: What data structures are needed?
2. **Backend/Frontend split**: What needs API vs UI?
3. **Dependencies**: What must exist before this works?
4. **Testing**: How will this be verified?
5. **Incremental value**: Can users benefit before it's complete?

## Task Title Conventions

Good titles are action-oriented and specific:

| Good | Bad |
|------|-----|
| "Add save button to form" | "Save functionality" |
| "Validate email format on blur" | "Form validation" |
| "Show loading spinner during fetch" | "Loading states" |
| "Redirect to dashboard after login" | "Auth flow" |

## Acceptance Criteria

Each task should have 2-5 acceptance criteria:

```
Task: Add save button to form

Acceptance Criteria:
- [ ] Save button visible at bottom of form
- [ ] Button disabled when form is invalid
- [ ] Button shows loading state during save
- [ ] Success message appears after save
- [ ] Error message shown if save fails
```

## Red Flags

Signs a task needs more decomposition:
- Description uses "and" multiple times
- Estimated at more than 4 hours
- Requires changes across many files
- Depends on multiple incomplete tasks
- Acceptance criteria exceed 5 items
- Multiple developers would step on each other

## Example Decomposition Session

**User**: "I need user authentication"

**Thinking**:
1. This is too big - auth has many parts
2. What are the core pieces?
   - User entity
   - Login UI
   - Registration UI
   - Token management
   - Protected routes
   - Session persistence
3. What's the minimum viable auth?
   - Login + basic session
4. What can wait?
   - Registration, OAuth, password reset

**Proposed Tasks**:
1. Create User entity with email/password
2. Add login API endpoint
3. Create login form component
4. Implement JWT token storage
5. Add auth context for session state
6. Create protected route wrapper
7. Add logout functionality
8. Persist session across page refresh

Each task is ~2-3 hours, independently testable, and builds toward complete auth.
