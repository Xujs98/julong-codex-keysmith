#!/usr/bin/env python3
"""Black-box smoke test for novel_agent.py."""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).with_name("novel_agent.py")


def run(workspace, *arguments):
    result = subprocess.run(
        [sys.executable, str(SCRIPT), "--project", str(workspace), *arguments],
        capture_output=True, text=True, check=True
    )
    return json.loads(result.stdout)


with tempfile.TemporaryDirectory(prefix="novel-agent-test-") as folder:
    root = Path(folder)
    assert run(root, "init", "--title", "验证小说")["project"]["title"] == "验证小说"
    run(root, "character", "upsert", "--id", "hero", "--json", '{"name": "主角", "traits": ["冷静"]}')
    run(root, "worldbook", "upsert", "--id", "city", "--json", '{"title": "城市", "content": "雨夜"}')
    beats = run(root, "beat", "create", "--json", '[{"title": "开场", "objective": "相遇"}]')
    beat_id = beats["outline"]["beats"][0]["id"]
    run(root, "beat", "start", "--id", beat_id)
    context = run(root, "context", "recall", "--query", "主角")
    assert context["characters"][0]["id"] == "hero"
    run(root, "scene", "update", "--json", '{"title": "雨夜相遇", "location": "街角"}')
    chapter = run(root, "chapter", "save", "--file", "01.md", "--text", "第一章")
    assert Path(chapter["path"]).exists()
    run(root, "summary", "add", "--text", "主角抵达街角。")
    archive = run(root, "export", "--output", str(root / "state.zip"))["path"]
    assert Path(archive).exists()

print("novel_agent smoke test passed")
