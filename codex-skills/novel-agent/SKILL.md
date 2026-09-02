---
name: novel-agent
description: Write and manage long-form fiction in a local project using structured characters, worldbooks, scenes, beats, context recall, and chapter files.
metadata:
  short-description: Structured fiction workspace for Codex
---

# Novel Agent

Use this skill when the user wants to plan, draft, revise, or continue a novel, screenplay, interactive story, or serialized fiction project. Keep project state in a `.novel-agent/` directory inside the user's workspace; do not depend on the source project's browser UI.

## Operating model

- Before drafting, inspect the current project state, scene, active characters, worldbooks, and outline beats.
- Use `scripts/novel_agent.py` for deterministic state changes. Execute one mutation per command and verify its JSON result.
- Treat `characters/`, `worldbooks/`, `scenes.jsonl`, `summaries.jsonl`, `outline.json`, and `chapters/` as the source of truth.
- Recall only the character fields, worldbook entries, scene snapshot, and current beat needed for the requested passage.
- After drafting or revising, save the chapter, update the scene snapshot, and append a compact summary so another Codex task can resume from local files.
- Preserve the user's language, tense, viewpoint, tone, rating, and continuity. All operations run through Codex conversation and local files.

## Tool mapping

- `character_select`: `character list|show|upsert|remove`
- `recall_context`: `context recall --query ...`
- `scene_manager`: `scene show|update`
- `beat_manager`: `beat list|create|add|start|done`
- `generate_options`: `options save`
- State persistence: `init`, `state`, `chapter save`, `summary add`, `import`, and `export`

Read [references/workflow.md](references/workflow.md) for the narrative loop and [references/schemas.md](references/schemas.md) for JSON shapes. Run `python3 scripts/novel_agent.py --help` for commands.
