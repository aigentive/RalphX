---
name: "Avoid repeat conversation skill candidate"
description: "Before similar work, check prior conversation skill candidate evidence and avoid repeating the same failure pattern."
---

# Avoid repeat conversation skill candidate

Before similar work, check prior conversation skill candidate evidence and avoid repeating the same failure pattern.

## Guidance

## Learned candidate

Prior outcome `conversation_skill_candidate` from `agent_conversation` was marked eligible for review.

Evidence summary:

```json
{"conversation_id":"a9cc8b49-cbf6-4ae4-9ca3-28c4fb47feed","message_count":5,"recent_messages":[{"content":"I’m resuming from the same Plan-mode session. I already traced the main backend and frontend paths; I’ll refresh the linked plan state, add the draft plan artifact, and keep it as a read-only planning artifact.\n\nNo draft plan exists yet for this planning session, so I’m creating the single linked artifact now. It will capture the architecture evidence, gotchas, options, and proof obligations without changing workspace files.\n\nI created the draft plan artifact: **Agent Conversation Plan Mode Architecture And Gotchas**.\n\nKey extraction:\n- Plan mode is a separate Agent conversation phase, not normal Ideation mode.\n- It coordinates workspace mode, linked planning ideation session, runtime context injection, artifact approval, and UI post-approval actions.\n- Biggest gotchas are state drift between `chat_conversations.agent_mode` and `agent_conversation_workspaces.mode`, duplicate frontend/bac...","message_id":"84b0eecb-7fb2-4575-b329-86292872075e","role":"orchestrator"},{"content":"Continue where you left off.","message_id":"5cc9a60f-d5ef-4ffa-82b4-614886d5fb4f","role":"us...
```

## Predicted Effect

Reduces repeat conversation skill candidate outcomes by surfacing the prior evidence before similar work.
