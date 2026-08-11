# Right-side chat panel audit — retired

The 2026-04-19 audit in this file covered standalone Kanban and Graph task-chat
docks that are no longer part of the product. Planning and task work now lives
inside **Agents**:

- The main Agents conversation owns chat.
- The Tasks artifact owns embedded Kanban and Graph modes.
- Selecting a card or node opens the Agents-owned task detail overlay.
- Graph keeps the execution timeline as its only right-side panel.

Use the current Agents and task-detail specifications for implementation
guidance. The retired split-chat findings have no remaining production target.
