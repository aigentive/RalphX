---
name: qa-evaluation
description: Guide for analyzing git diff and refining QA test plans
---

# QA Evaluation

Guidelines for refining test plans based on actual implementation and evaluating results.

## Phase 2A: Refinement Process

### 1. Analyze Git Diff
```bash
git diff HEAD~1 --stat          # Overview of changed files
git diff HEAD~1 -- <file>       # Specific file changes
git log -1 --oneline            # Commit message context
```

### 2. Compare Intent vs Reality
- Original acceptance criteria: What was planned
- Git diff: What was actually implemented
- Identify:
  - Selectors that changed (testids, classes)
  - Features added/removed
  - UI structure differences

### 3. Refine Test Steps
Update test commands to match actual implementation:
- Correct element selectors
- Adjust wait times
- Update expected outcomes
- Add tests for new features
- Remove tests for dropped features

## Refinement Output Format

```json
{
  "actual_implementation": "Brief summary of what git diff shows was implemented",
  "refined_test_steps": [
    {
      "id": "QA1",
      "criteria_id": "AC1",
      "description": "Updated description",
      "commands": [
        "agent-browser open http://localhost:1420",
        "agent-browser wait --load",
        "agent-browser is visible [data-testid='actual-element']",
        "agent-browser screenshot \"$RALPHX_AGENT_SCREENSHOT_DIR/qa1.png\""
      ],
      "expected": "Updated expectation"
    }
  ]
}
```

## Phase 2B: Test Execution

### Execution Flow
1. Execute each test step in order
2. Record pass/fail for each command
3. Capture screenshot regardless of outcome
4. Continue testing even if step fails
5. Calculate overall status

### Result Recording

For each step:
- **status**: "passed" | "failed" | "skipped"
- **screenshot**: Path to captured image
- **actual**: What actually happened (if failure)
- **expected**: What should have happened
- **error**: Error message (if any)

## Test Results Output Format

```json
{
  "qa_results": {
    "task_id": "task-123",
    "overall_status": "passed",
    "total_steps": 5,
    "passed_steps": 5,
    "failed_steps": 0,
    "steps": [
      {
        "step_id": "QA1",
        "status": "passed",
        "screenshot": "$RALPHX_AGENT_SCREENSHOT_DIR/qa1-result.png",
        "actual": null,
        "expected": null,
        "error": null
      }
    ]
  }
}
```

## Failure Analysis

### Common Failure Types

1. **Element Not Found**
   - Selector mismatch
   - Element not rendered
   - Timing issue

2. **Visibility Failed**
   - Element hidden
   - Wrong element found
   - CSS display:none

3. **Interaction Failed**
   - Element disabled
   - Overlapping element
   - Event not triggered

4. **Value Mismatch**
   - Text differs from expected
   - Attribute missing
   - State not updated

### Recording Failures

```json
{
  "step_id": "QA2",
  "status": "failed",
  "screenshot": "$RALPHX_AGENT_SCREENSHOT_DIR/qa2-failure.png",
  "actual": "Button text is 'Submit' instead of 'Save'",
  "expected": "Button should display 'Save'",
  "error": "Text content mismatch"
}
```

## Evaluation Guidelines

### Determine Pass/Fail
- **Pass**: All verification commands succeed
- **Fail**: Any verification command fails
- **Skip**: Prerequisites not met (dependency failed)

### Overall Status
- **passed**: All steps passed
- **failed**: Any step failed
- **pending**: Not yet executed

### Flaky Test Detection
If same test passes/fails inconsistently:
- Add longer waits
- Check for animations
- Verify element stability
- Consider network latency

## Best Practices

### Be Thorough
- Test all acceptance criteria
- Don't skip edge cases
- Verify both positive and negative scenarios

### Be Accurate
- Don't guess results
- Actually execute commands
- Capture real screenshots

### Be Helpful
- Clear error descriptions
- Actionable feedback
- Specific fix suggestions

### Be Efficient
- Don't repeat unnecessary setup
- Group related verifications
- Use appropriate waits

## Example Workflow

```
1. Read acceptance criteria from prep phase
2. Run: git diff HEAD~1
3. Update test steps based on actual selectors
4. Execute each step:
   a. Run commands
   b. Check outcome
   c. Record result
   d. Capture screenshot
5. Calculate totals
6. Return QA results JSON
```
