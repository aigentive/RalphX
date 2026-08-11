---
paths:
  - "frontend/src/components/Chat/**"
  - "frontend/src/hooks/useChat*"
---

# Chat Handler Patterns

> **Maintainer note:** This file optimizes for LLM context efficiency. Rules: (1) Tables > prose (2) One example max per concept (3) No redundant explanations (4) Use symbols: → = leads to, | = or, ❌/✅ = wrong/right (5) Before adding content, ask: "Can this be a single line?" If yes, make it one line.

## Summary

- Special states (questionMode) must bypass message queue and send immediately
- Conditional order critical: special states → transient states → normal flow
- Incorrect order causes agent deadlock waiting for queued answer

## Conditional Priority in Input Handlers

**Rule:** In input handlers where multiple paths exist (agent running, special modes, normal flow), check **special states first**, then **transient states**, then **normal flow**.

**Why:** Special states (like `questionMode`) represent request-response pairs that bypass normal message flow. If queued, they deadlock the agent.

### ❌ Wrong Order (Lost Input)
```typescript
clearInput();                 // optimistic clear BEFORE the special-state send
await onSend(trimmedValue);   // questionMode answer lost if backend send fails
```

### ✅ Correct Order (current two-branch handleSend)
```typescript
if (questionMode) {
  // Special state: send immediately; do NOT clear input until success
  await onSend(trimmedValue);
} else {
  // Normal flow: optimistic clear, then send (queueing happens behind onSend)
  clearInput();
  await onSend(trimmedValue);
}
```

**Impacted:** `frontend/src/components/Chat/ChatInput.tsx:handleSend`
