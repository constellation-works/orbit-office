#!/usr/bin/env python3
"""Synthetic callback fixture, NOT an Orbit authority or installed CLI.

Only the demo's exec children use ORBIT_BIN to select this file. It records
synthetic proposals and artifacts in their fresh root. It refuses dispatch,
network calls and any command outside the tiny callback surface below.
"""
import json
import os
from pathlib import Path
import sys

from common import child_env, run, write_json


def view(task):
    return dict(task, artifacts=[{'path': path} for path in sorted(task['artifacts'])],
                description=task.get('description', ''), execution_summary=task.get('execution_summary'))


def callback(tool, data, root):
    state = root / "demo-orbit.json"
    db = json.loads(state.read_text()) if state.exists() else {"tasks": {}}
    tasks = db["tasks"]
    if tool == "orbit.task.add":
        identity = f"DEMO-{len(tasks) + 1:04d}"
        task = dict(data, id=identity, status="proposed", artifacts={},
                    created_at=data.get('created_at', '2026-10-25T09:00:00Z'),
                    history=[])
        task.setdefault('type', 'chore')
        task.setdefault('tags', [])
        tasks[identity] = task
        result = dict(task, task=task)
    elif tool == "orbit.task.update":
        task = tasks[data["id"]]
        if 'status' in data:
            task['history'].append({'at': data.get('at', '2026-10-25T10:00:00Z'),
                                    'from_status': task['status'], 'to_status': data['status']})
        task.update({k: v for k, v in data.items() if k != "id"})
        result = dict(task, task=task)
    elif tool == "orbit.task.show":
        result = view(tasks[data["id"]])
    elif tool == "orbit.task.list":
        selected = list(tasks.values())
        for key in ["status", "tag"]:
            if key in data:
                accepted = data[key] if isinstance(data[key], list) else [data[key]]
                selected = [t for t in selected if (t.get(key) in accepted if key == "status"
                            else data[key] in t.get("tags", []))]
        result = {"tasks": [view(task) for task in selected], "total": len(selected), "truncated": False}
    elif tool == "orbit.task.artifact.put":
        source = Path(data["source_path"]).resolve()
        if not source.is_relative_to(root):
            raise ValueError("demo artifact must be inside its fresh root")
        tasks[data["id"]]["artifacts"][data["path"]] = source.read_text()
        result = {"path": data["path"], "stored": True}
    elif tool == "orbit.task.artifact.get":
        content = tasks[data["id"]]["artifacts"][data["path"]]
        result = {"content": content, "encoding": "utf8", "path": data["path"]}
    elif tool in ["orbit.search", "orbit.auto_task.list"]:
        result = {"tasks": [], "results": [], "auto_tasks": [], "truncated": False}
    else:
        raise ValueError("unsupported synthetic callback")
    if tool in {'orbit.task.add', 'orbit.task.update', 'orbit.task.artifact.put'}:
        write_json(state, db)
    return result


def main():
    if sys.argv[1:3] != ["tool", "run"]:
        raise ValueError("synthetic fixture accepts tool run only")
    root = Path(os.environ["ORBIT_WORKSPACE"]).resolve()
    if not (root / ".synthetic-demo").is_file():
        raise ValueError("missing fresh synthetic root marker")
    payload = json.loads(sys.argv[sys.argv.index("--input") + 1])
    tool = sys.argv[3]
    if tool.startswith('office.'):
        allowed = {'office.status', 'office.source-show', 'office.knowledge-list', 'office.knowledge-show',
                   'office.artifact-list', 'office.artifact-show', 'office.search', 'office.metrics',
                   'office.bootstrap-status', 'office.selection-get'}
        if tool not in allowed:
            raise ValueError('demo preview is read only')
        runtime = json.loads((root / 'runtime.json').read_text())
        result = json.loads(run([runtime['office'], 'orbit-tool'], env=child_env(root), cwd=root,
                  input=json.dumps({'schema_version': 1, 'tool': tool, 'input': payload,
                                    'context': {'workspace_root': str(root), 'plugin_state': str(root / 'office-state')}})))
        if not result['ok']:
            raise ValueError(result['error']['code'])
        result = result['output']
    else:
        result = callback(tool, payload, root)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
