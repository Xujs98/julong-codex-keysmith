# State schemas

`project.json`: `{ "title": string, "language": string, "viewpoint": string, "tone": string, "created_at": string, "updated_at": string }`

Character file: `{ "id": string, "name": string, "aliases": [], "role": string, "traits": [], "goals": [], "constraints": [], "voice": string, "status": "active"|"inactive", "notes": string }`

Worldbook file: `{ "id": string, "title": string, "category": string, "keywords": [], "content": string, "priority": number, "enabled": boolean }`

Scene snapshot: `{ "id": string, "title": string, "location": string, "time": string, "pov": string, "characters": [], "facts": [], "open_threads": [], "last_beat_id": string }`

Beat: `{ "id": string, "title": string, "objective": string, "status": "pending"|"active"|"done", "order": number, "notes": string }`
