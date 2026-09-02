#!/usr/bin/env python3
"""Local, dependency-free state manager for the Novel Agent Codex skill."""
from __future__ import annotations

import argparse
import json
import os
import shutil
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1


def timestamp() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def project_root(value: str | None = None) -> Path:
    return Path(value or os.environ.get("NOVEL_PROJECT") or os.getcwd()).expanduser().resolve()


def data_dir(root: Path) -> Path:
    return root / ".novel-agent"


def fail(message: str, code: int = 2) -> None:
    print(json.dumps({"ok": False, "error": message}, ensure_ascii=False))
    raise SystemExit(code)


def emit(**payload: Any) -> None:
    print(json.dumps({"ok": True, **payload}, ensure_ascii=False, indent=2))


def read_json(path: Path, default: Any) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return default
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON at {path}: {exc}")


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False, indent=2)
            stream.write("\n")
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def append_jsonl(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(value, ensure_ascii=False) + "\n")


def ensure_project(root: Path) -> Path:
    directory = data_dir(root)
    if not (directory / "project.json").exists():
        fail("project is not initialized; run init first")
    return directory


def ensure_layout(root: Path) -> Path:
    directory = data_dir(root)
    directory.mkdir(parents=True, exist_ok=True)
    for name in ("characters", "worldbooks", "chapters"):
        (directory / name).mkdir(exist_ok=True)
    return directory


def slug(value: str) -> str:
    cleaned = "".join(char.lower() if char.isalnum() or char in "-_" else "-" for char in value.strip())
    cleaned = "-".join(part for part in cleaned.split("-") if part)
    return cleaned or "record"


def require_id(value: str) -> str:
    normalized = slug(value)
    if normalized == "record" and not value.strip():
        fail("--id is required for this action")
    return normalized


def record_path(directory: Path, collection: str, identifier: str) -> Path:
    return directory / collection / f"{require_id(identifier)}.json"


def list_records(directory: Path, collection: str) -> list[dict[str, Any]]:
    return [read_json(path, {}) for path in sorted((directory / collection).glob("*.json"))]


def current_scene(directory: Path) -> dict[str, Any]:
    path = directory / "scenes.jsonl"
    try:
        lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    except FileNotFoundError:
        return {}
    return json.loads(lines[-1]) if lines else {}


def project_state(directory: Path) -> dict[str, Any]:
    return read_json(directory / "state.json", {"schema": SCHEMA_VERSION})


def write_state(directory: Path, state: dict[str, Any]) -> None:
    state["updated_at"] = timestamp()
    write_json(directory / "state.json", state)


def parse_object(value: str, label: str) -> dict[str, Any]:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as exc:
        fail(f"{label} must be valid JSON: {exc}")
    if not isinstance(parsed, dict):
        fail(f"{label} must be a JSON object")
    return parsed


def parse_array(value: str, label: str) -> list[Any]:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as exc:
        fail(f"{label} must be valid JSON: {exc}")
    if not isinstance(parsed, list):
        fail(f"{label} must be a JSON array")
    return parsed


def command_init(args: argparse.Namespace) -> None:
    root = project_root(args.project)
    directory = ensure_layout(root)
    project = read_json(directory / "project.json", {})
    project.update(
        {
            "schema": SCHEMA_VERSION,
            "title": args.title,
            "language": args.language,
            "viewpoint": args.viewpoint,
            "tone": args.tone,
            "created_at": project.get("created_at", timestamp()),
            "updated_at": timestamp(),
        }
    )
    write_json(directory / "project.json", project)
    if not (directory / "state.json").exists():
        write_state(directory, {"schema": SCHEMA_VERSION, "current_scene": "scene-1", "current_beat": None})
    if not (directory / "outline.json").exists():
        write_json(directory / "outline.json", {"title": args.title, "beats": []})
    if not (directory / "scenes.jsonl").exists():
        append_jsonl(
            directory / "scenes.jsonl",
            {
                "id": "scene-1",
                "title": "开场",
                "location": "",
                "time": "",
                "pov": args.viewpoint,
                "characters": [],
                "facts": [],
                "open_threads": [],
                "last_beat_id": "",
                "updated_at": timestamp(),
            },
        )
    emit(action="init", path=str(directory), project=project)


def command_state(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    emit(
        action="state",
        project=read_json(directory / "project.json", {}),
        state=project_state(directory),
        scene=current_scene(directory),
        outline=read_json(directory / "outline.json", {}),
    )


def command_record(args: argparse.Namespace, collection: str) -> None:
    directory = ensure_project(project_root(args.project))
    if args.action == "list":
        emit(action=f"{collection}_list", records=list_records(directory, collection))
        return

    path = record_path(directory, collection, args.id)
    if args.action == "show":
        if not path.exists():
            fail(f"{collection} record not found: {args.id}", 1)
        emit(action=f"{collection}_show", record=read_json(path, {}))
        return
    if args.action == "remove":
        if path.exists():
            path.unlink()
        emit(action=f"{collection}_remove", id=require_id(args.id))
        return

    record = parse_object(args.json, "--json")
    record["id"] = require_id(args.id)
    record.setdefault("name" if collection == "characters" else "title", args.id)
    record.setdefault("status", "active") if collection == "characters" else record.setdefault("enabled", True)
    record["updated_at"] = timestamp()
    write_json(path, record)
    emit(action=f"{collection}_upsert", record=record)


def command_scene(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    previous = current_scene(directory)
    if args.action == "show":
        emit(action="scene_show", scene=previous)
        return

    scene = parse_object(args.json, "--json")
    scene.setdefault("id", previous.get("id", "scene-1"))
    scene.setdefault("title", previous.get("title", "未命名场景"))
    scene.setdefault("characters", previous.get("characters", []))
    scene.setdefault("facts", previous.get("facts", []))
    scene.setdefault("open_threads", previous.get("open_threads", []))
    scene["updated_at"] = timestamp()
    append_jsonl(directory / "scenes.jsonl", scene)
    state = project_state(directory)
    state["current_scene"] = scene["id"]
    write_state(directory, state)
    emit(action="scene_update", scene=scene)


def command_beat(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    outline = read_json(directory / "outline.json", {"title": "", "beats": []})
    beats = outline.setdefault("beats", [])
    if args.action == "list":
        emit(action="beat_list", beats=beats, current_beat=project_state(directory).get("current_beat"))
        return

    if args.action in ("create", "add"):
        incoming = parse_array(args.json, "--json")
        if args.action == "create":
            beats.clear()
        for item in incoming:
            if not isinstance(item, dict):
                fail("each beat must be a JSON object")
            item = dict(item)
            item.setdefault("id", f"beat-{len(beats) + 1}")
            item["id"] = slug(str(item["id"]))
            item.setdefault("title", item["id"])
            item.setdefault("objective", "")
            item.setdefault("status", "pending")
            item.setdefault("order", len(beats) + 1)
            item.setdefault("notes", "")
            beats.append(item)
    else:
        identifier = require_id(args.id)
        match = next((item for item in beats if item.get("id") == identifier), None)
        if match is None:
            fail(f"beat not found: {identifier}", 1)
        if args.action == "start":
            for item in beats:
                if item.get("status") == "active":
                    item["status"] = "pending"
            match["status"] = "active"
        elif args.action == "done":
            match["status"] = "done"

    write_json(directory / "outline.json", outline)
    state = project_state(directory)
    if args.action == "start":
        state["current_beat"] = require_id(args.id)
    elif args.action == "done":
        state["current_beat"] = None
    write_state(directory, state)
    emit(action="beat_update", outline=outline, current_beat=state.get("current_beat"))


def command_context(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    query = args.query.strip().lower()

    def matches(item: dict[str, Any]) -> bool:
        return not query or query in json.dumps(item, ensure_ascii=False).lower()

    characters = [record for record in list_records(directory, "characters") if matches(record)]
    worldbooks = [
        record
        for record in list_records(directory, "worldbooks")
        if record.get("enabled", True) and matches(record)
    ]
    emit(
        action="context_recall",
        query=args.query,
        characters=characters,
        worldbooks=worldbooks,
        scene=current_scene(directory),
        outline=read_json(directory / "outline.json", {}),
        state=project_state(directory),
    )


def command_chapter(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    path = (directory / "chapters" / args.file).resolve()
    if directory.resolve() not in path.parents:
        fail("chapter path must remain inside .novel-agent/chapters")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(args.text, encoding="utf-8")
    emit(action="chapter_save", path=str(path), bytes=len(args.text.encode("utf-8")))


def command_summary(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    entry = {
        "id": f"summary-{int(datetime.now().timestamp() * 1000)}",
        "text": args.text,
        "scene_id": args.scene or project_state(directory).get("current_scene", ""),
        "created_at": timestamp(),
    }
    append_jsonl(directory / "summaries.jsonl", entry)
    emit(action="summary_add", entry=entry)


def command_options(args: argparse.Namespace) -> None:
    directory = ensure_project(project_root(args.project))
    options = parse_array(args.json, "--json")
    if not 1 <= len(options) <= 8:
        fail("options must contain 1 to 8 items")
    state = project_state(directory)
    state["current_options"] = options
    write_state(directory, state)
    emit(action="options_save", options=options)


def command_export(args: argparse.Namespace) -> None:
    root = project_root(args.project)
    directory = ensure_project(root)
    output = Path(args.output).expanduser().resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    archive = shutil.make_archive(str(output.with_suffix("")), "zip", directory)
    emit(action="export", path=archive, project=str(root))


def command_import(args: argparse.Namespace) -> None:
    root = project_root(args.project)
    archive = Path(args.archive).expanduser().resolve()
    if not archive.is_file():
        fail(f"archive not found: {archive}", 1)
    target = data_dir(root)
    if target.exists() and any(target.iterdir()) and not args.overwrite:
        fail("target project already has .novel-agent data; pass --overwrite to replace it")
    staging = Path(tempfile.mkdtemp(prefix="novel-agent-import-"))
    try:
        shutil.unpack_archive(str(archive), staging)
        source = staging
        if not (source / "project.json").exists():
            candidates = [path for path in staging.iterdir() if path.is_dir() and (path / "project.json").exists()]
            if len(candidates) != 1:
                fail("archive does not contain a valid Novel Agent state directory")
            source = candidates[0]
        if target.exists():
            shutil.rmtree(target)
        shutil.copytree(source, target)
    finally:
        shutil.rmtree(staging, ignore_errors=True)
    emit(action="import", path=str(target), project=str(root))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", help="workspace containing .novel-agent (defaults to CWD/NOVEL_PROJECT)")
    commands = parser.add_subparsers(dest="command", required=True)

    item = commands.add_parser("init")
    item.add_argument("--title", required=True)
    item.add_argument("--language", default="中文")
    item.add_argument("--viewpoint", default="第三人称限知")
    item.add_argument("--tone", default="")
    item.set_defaults(handler=command_init)

    item = commands.add_parser("state")
    item.set_defaults(handler=command_state)

    for command, collection in (("character", "characters"), ("worldbook", "worldbooks")):
        item = commands.add_parser(command)
        item.add_argument("action", choices=("list", "show", "upsert", "remove"))
        item.add_argument("--id", default="")
        item.add_argument("--json", default="{}")
        item.set_defaults(handler=lambda args, collection=collection: command_record(args, collection))

    item = commands.add_parser("scene")
    item.add_argument("action", choices=("show", "update"))
    item.add_argument("--json", default="{}")
    item.set_defaults(handler=command_scene)

    item = commands.add_parser("beat")
    item.add_argument("action", choices=("list", "create", "add", "start", "done"))
    item.add_argument("--id", default="")
    item.add_argument("--json", default="[]")
    item.set_defaults(handler=command_beat)

    item = commands.add_parser("context")
    item.add_argument("action", choices=("recall",))
    item.add_argument("--query", default="")
    item.set_defaults(handler=command_context)

    item = commands.add_parser("chapter")
    item.add_argument("action", choices=("save",))
    item.add_argument("--file", required=True)
    item.add_argument("--text", required=True)
    item.set_defaults(handler=command_chapter)

    item = commands.add_parser("summary")
    item.add_argument("action", choices=("add",))
    item.add_argument("--text", required=True)
    item.add_argument("--scene", default="")
    item.set_defaults(handler=command_summary)

    item = commands.add_parser("options")
    item.add_argument("action", choices=("save",))
    item.add_argument("--json", required=True)
    item.set_defaults(handler=command_options)

    item = commands.add_parser("export")
    item.add_argument("--output", default="novel-agent-export.zip")
    item.set_defaults(handler=command_export)

    item = commands.add_parser("import")
    item.add_argument("--archive", required=True)
    item.add_argument("--overwrite", action="store_true")
    item.set_defaults(handler=command_import)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    args.handler(args)


if __name__ == "__main__":
    main()
