# ADR-0001: Adopt Persistent Project Memory Structure

## Status

Accepted

## Context

This project needs durable context that survives across CLI, headless, and multi-agent coding sessions. The implementation repo alone is not enough to preserve current state, invariants, active work, and architectural intent.

## Decision

Adopt a version-controlled project memory structure at the project root consisting of:

- `AGENTS.md`, `CODEX.md`, and `CLAUDE.md` for agent instructions
- `.context/` for project brief, current state, and invariants
- `docs/` for architecture notes, workflows, decisions, and tasks
- `scripts/check-context-files.sh` as a lightweight presence check

## Consequences

- Agents now have a stable entry point before editing.
- Project context can evolve independently of the inner implementation repo.
- Architectural intent and active work become easier to preserve across sessions.
- These files must now be maintained as part of normal project hygiene.
