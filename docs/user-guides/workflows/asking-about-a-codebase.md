# Asking questions about a codebase

Use an Ask conversation when you want read-only help understanding a codebase, an existing behavior, or a technical decision. You will finish with an answer grounded in the project context without starting implementation.

**Before you start:** [Finding your way around](../02-tour-of-the-app.md)

## Start an Ask conversation

1. Start a conversation in **Ask**.
2. Use **Ask** when you need an explanation, investigation, or orientation rather than a code change.
3. Start without a project when your question does not need project context.
4. Select a project when its codebase is necessary to answer the question.
5. State the question directly and include the outcome you need from the answer.

   Answers usually arrive in under a minute, though a question that sends RalphX reading widely across the project takes longer.

   **Ask** still runs a real agent against your provider account and consumes credits, but it is the cheapest of the workflows here — it reads and explains rather than writing code. Use it freely.

   You can end a long-running answer with **Stop** in the composer, the same as any other run.

## Add useful context

1. Type `@` in the composer to reference relevant project context.

   A menu opens as soon as you type it. Keep typing to filter, then pick the entry you want — the reference is inserted into your message and RalphX loads that context when it answers.

2. Reference the file, artifact, or conversation context that makes the question precise.
3. Ask one focused question at a time when you need a dependable answer.
4. Include an example of the behavior you are trying to understand when it is ambiguous.
5. Ask a follow-up when the answer identifies an unfamiliar term or an important tradeoff.

> **Worked example — an `@` reference in practice.** The other four workflow guides follow one feature through **RalphX Release Companion**: *"Block publishing until the release checklist is complete"*. Here it is approached as a question instead of a change — the step before you decide to build anything. Rather than asking "how does publishing work", which invites a tour of the whole flow:
>
> *"@publish-service.ts What stops a release from being published today? I'm trying to work out where a checklist gate would have to live to also cover API callers, not just the button."*
>
> The `@` reference is what makes this answerable. Without it RalphX has to guess which of several publish paths you mean; with it, the answer is grounded in the file you are actually looking at. Naming the decision you are trying to make — gate placement — is the other half, because it tells RalphX which details matter.

## Keep the conversation read-only

1. Use **Ask** to explore the codebase before deciding whether to make a change.
2. Treat the answer as read-only guidance, not an implementation request.
3. Start a **Plan** conversation when your question becomes a feature you want to build.
4. Keep the original Ask conversation for future reference when it explains a decision or area of the codebase.

## What you have now

You have a read-only answer about a codebase, supported by the context you supplied with `@` references. You can keep asking follow-up questions or use what you learned to start planning a change.

## Next

- [Planning a feature with RalphX](planning-a-feature.md)

If this did not look right — the `@` menu found nothing, or the answer described a different part of the codebase than you meant — see [When something goes wrong](../troubleshooting.md).
