---
name: qa-step-generation
description: Guide for generating QA test steps with agent-browser commands
---

# QA Step Generation

Guidelines for creating executable QA test steps using agent-browser.

## Test Step Structure

Each test step maps to one or more acceptance criteria and contains:
1. **id**: Unique identifier (QA1, QA2...)
2. **criteria_id**: Links to acceptance criterion
3. **description**: What the step verifies
4. **commands**: Array of agent-browser commands
5. **expected**: What success looks like

## Output Format

```json
{
  "qa_steps": [
    {
      "id": "QA1",
      "criteria_id": "AC1",
      "description": "Verify task board renders with all columns",
      "commands": [
        "agent-browser open http://localhost:1420",
        "agent-browser wait --load",
        "agent-browser snapshot -i -c",
        "agent-browser is visible [data-testid='task-board']",
        "agent-browser screenshot \"$RALPHX_AGENT_SCREENSHOT_DIR/qa1-taskboard.png\""
      ],
      "expected": "Task board visible with 7 columns"
    }
  ]
}
```

## Command Patterns

### Standard Test Template
```
agent-browser open <url>
agent-browser wait --load
agent-browser snapshot -i -c
# verification commands
agent-browser screenshot "$RALPHX_AGENT_SCREENSHOT_DIR/<task>-<step>.png"
agent-browser close
```

### Visibility Verification
```
agent-browser is visible [data-testid='element']
agent-browser is visible .class-name
agent-browser is visible #element-id
```

### Interaction Testing
```
agent-browser click [data-testid='button']
agent-browser wait 500
agent-browser is visible [data-testid='result']
```

### Form Testing
```
agent-browser fill [data-testid='input'] "test value"
agent-browser click [data-testid='submit']
agent-browser wait 1000
agent-browser is visible [data-testid='success']
```

### Drag-Drop Testing
```
agent-browser snapshot -i -c
agent-browser drag @e5 @e8
agent-browser wait 500
agent-browser screenshot "$RALPHX_AGENT_SCREENSHOT_DIR/after-drag.png"
```

### Text Verification
```
agent-browser get text [data-testid='title']
# Compare output with expected value
```

## Best Practices

### Always Do
- Start with `agent-browser open` and `wait --load`
- Use `snapshot -i -c` to see available elements
- Take screenshots as evidence
- End with `agent-browser close`
- Use data-testid selectors when available

### Use Element References
After `snapshot`, elements are labeled @e1, @e2, etc.
Use these for reliable interactions:
```
agent-browser snapshot -i -c
agent-browser click @e3
```

### Wait Appropriately
- `wait --load` for initial page load
- `wait 500` for quick animations
- `wait 1000` for API responses
- `wait @e1` for element to appear

### Screenshot Naming
Format: `$RALPHX_AGENT_SCREENSHOT_DIR/<task-name>-<step>-<description>.png`
Examples:
- `$RALPHX_AGENT_SCREENSHOT_DIR/kanban-qa1-board-visible.png`
- `$RALPHX_AGENT_SCREENSHOT_DIR/task-create-qa2-form-submitted.png`

## Common Scenarios

### Component Visibility Test
```json
{
  "id": "QA1",
  "criteria_id": "AC1",
  "description": "Verify component renders",
  "commands": [
    "agent-browser open http://localhost:1420",
    "agent-browser wait --load",
    "agent-browser is visible [data-testid='component']",
    "agent-browser screenshot \"$RALPHX_AGENT_SCREENSHOT_DIR/component-visible.png\"",
    "agent-browser close"
  ],
  "expected": "Component is visible on page"
}
```

### Click Action Test
```json
{
  "id": "QA2",
  "criteria_id": "AC2",
  "description": "Verify button click opens panel",
  "commands": [
    "agent-browser open http://localhost:1420",
    "agent-browser wait --load",
    "agent-browser click [data-testid='open-button']",
    "agent-browser wait 500",
    "agent-browser is visible [data-testid='panel']",
    "agent-browser screenshot \"$RALPHX_AGENT_SCREENSHOT_DIR/panel-opened.png\"",
    "agent-browser close"
  ],
  "expected": "Panel opens after button click"
}
```

### Multiple Element Test
```json
{
  "id": "QA3",
  "criteria_id": "AC3",
  "description": "Verify all columns present",
  "commands": [
    "agent-browser open http://localhost:1420",
    "agent-browser wait --load",
    "agent-browser is visible [data-testid='col-draft']",
    "agent-browser is visible [data-testid='col-planned']",
    "agent-browser is visible [data-testid='col-executing']",
    "agent-browser screenshot \"$RALPHX_AGENT_SCREENSHOT_DIR/all-columns.png\"",
    "agent-browser close"
  ],
  "expected": "All required columns visible"
}
```

## Anti-Patterns

- Skipping screenshot capture
- Not waiting for page/element
- Using brittle selectors (nth-child)
- Long complex command sequences
- Modifying application state in tests
