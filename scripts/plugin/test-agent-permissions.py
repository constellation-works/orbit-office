#!/usr/bin/env python3
"""Real public Orbit/Office authorization regression (no host state or services).

Build office first. Run with --orbit /path/to/orbit (>=0.39.0) and
--office /path/to/office. The live fixture is created under /tmp, outside this
checkout. A confined callback stats every ancestor `.orbit-product` and turns
a sandbox EACCES on that read into a fatal io_error, so a home nested under
this repo cannot create a proposal. The command transcript is copied back
under .orbit/tmp/. Missing prerequisites fail; this check never silently skips.
"""

import argparse
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import tempfile
import time


REPO = Path(__file__).resolve().parents[2]
SELECTED = {"capture", "proposal-create", "attach", "selection-get"}
GRANTS = ["fs={{workspace}},{{plugin_state}}", "network", "orbit_tools"]


class Fixture:
    def __init__(self, orbit, office, root):
        self.root = root
        self.home = self.root / "home"
        self.work = self.home / "work"
        for path in (self.root, self.home, self.work):
            for ancestor in [path, *path.parents]:
                marker = ancestor / ".orbit-product"
                if marker.is_file():
                    raise SystemExit(
                        f"fixture {path} is inside an Orbit product tree ({marker})"
                    )
        self.work.mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        shutil.copy2(orbit, self.bin / "orbit")
        self.orbit = str(self.bin / "orbit")
        self.office = office
        # Unused media dependencies are fail-fast fixtures, never successful
        # converters. This test exercises text capture and makes no model calls.
        for name in ["ffmpeg", "ffprobe", "pdftotext"]:
            program = self.bin / name
            program.write_text("#!/bin/sh\nexit 97\n")
            program.chmod(0o755)
        self.env = {
            "HOME": str(self.home), "PATH": f"{self.bin}:/usr/bin:/bin",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_AUTHOR_NAME": "Fixture", "GIT_COMMITTER_NAME": "Fixture",
            "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
            "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
        }
        self.records = []
        print(f"fixture: {self.root}", flush=True)
        self.command(["git", "init", "-b", "main"])
        self.command(["git", "commit", "--allow-empty", "-m", "Fixture"])
        self.run("workspace", "init", "--name", "office-auth", operator=True)

    def command(self, args, operator=False, success=True):
        env = dict(self.env)
        if operator:
            env["ORBIT_OPERATOR"] = "1"  # setup only; never an acting agent
        else:
            env["ORBIT_AGENT_NAME"] = "codex"
        out = subprocess.run(args, cwd=self.work, env=env, text=True,
                             input="", capture_output=True, timeout=120)
        record = dict(args=args, operator=operator, exit_code=out.returncode,
                      stdout=out.stdout, stderr=out.stderr)
        self.records.append(record)
        (self.root / "commands.json").write_text(json.dumps(self.records, indent=2))
        if success:
            assert out.returncode == 0, record
        return out

    def run(self, *args, operator=False, success=True):
        return self.command([self.orbit, *map(str, args)], operator, success)

    def data(self, *args, operator=False):
        return json.loads(self.run(*args, "--json", operator=operator).stdout)

    def call(self, verb, payload, operator=False):
        return self.data("tool", "run", "office." + verb, "--input",
                         json.dumps(payload), operator=operator)

    def denied(self, verb):
        out = self.run("tool", "run", "office." + verb, "--input", "{}",
                       "--json", success=False)
        assert out.returncode != 0 and "capability_denied" in out.stderr, out

    def export(self, name, manifest):
        root = self.root / name
        shutil.copytree(REPO / "plugin", root)
        inner = root / ".orbit-plugin"
        (inner / "plugin.yaml").write_text(manifest)
        shutil.copy2(self.office, inner / "bin/office-bin")
        return root

    def enable(self, agent=False):
        args = ["plugin", "enable", "office"]
        for grant in GRANTS + (["agent_tools"] if agent else []):
            args += ["--grant", grant]
        return self.data(*args, operator=True)

    def show(self):
        return self.data("plugin", "show", "office", operator=True)

    def mcp_capture(self):
        """An ordinary agent MCP session, with explicit shutdown and reaping."""
        env = dict(self.env, ORBIT_AGENT_NAME="codex")
        with (self.root / "mcp.stderr").open("wb") as diagnostics:
            child = subprocess.Popen([self.orbit, "mcp", "serve"], cwd=self.work,
                                     env=env, stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=diagnostics)
            pending = b""

            def rpc(identifier, method, params):
                nonlocal pending
                request = dict(jsonrpc="2.0", id=identifier, method=method, params=params)
                child.stdin.write((json.dumps(request) + "\n").encode())
                child.stdin.flush()
                deadline = time.monotonic() + 120
                while True:
                    while b"\n" in pending:
                        line, pending = pending.split(b"\n", 1)
                        response = json.loads(line)
                        if response.get("id") == identifier:
                            assert "error" not in response, response
                            return response["result"]
                    assert select.select([child.stdout], [], [], max(0, deadline - time.monotonic()))[0], "MCP response timeout"
                    block = os.read(child.stdout.fileno(), 65536)
                    assert block, "MCP server exited before response"
                    pending += block

            try:
                rpc(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                                      "_meta": {"orbit": {"workspace": "ws_office-auth"}},
                                      "clientInfo": {"name": "office-auth-fixture", "version": "1"}})
                child.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                child.stdin.flush()
                listed = rpc(2, "tools/list", {})
                names = {tool["name"] for tool in listed["tools"]}
                assert {"office_capture", "office_proposal-create", "office_attach", "office_selection-get"} <= names
                assert "office_proposal-accept" not in names and "office_knowledge-write" not in names
                called = rpc(3, "tools/call", {"name": "office_capture", "arguments": {
                    "kind": "note", "title": "MCP capture", "text": "ordinary MCP capture"}})
                assert not called.get("isError"), called
                output = json.loads(next(item["text"] for item in called["content"] if item["type"] == "text"))
                assert "ordinary MCP capture" in json.dumps(self.call("source-show", {"id": output["id"]}))
                refused = rpc(4, "tools/call", {"name": "office_library-sync", "arguments": {}})
                assert refused.get("isError") and "capability_denied" in json.dumps(refused), refused
                (self.root / "mcp-results.json").write_text(json.dumps({"capture": called, "refused": refused}, indent=2))
            finally:
                child.stdin.close()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait()
                child.stdout.close()


def check(f):
    candidate = (REPO / "plugin/.orbit-plugin/plugin.yaml").read_text()
    legacy = re.sub(r"^      agent_callable: true\n", "", candidate, flags=re.M)
    old = f.export("legacy", legacy)
    f.run("plugin", "add", old, operator=True)
    f.enable()
    f.call("init", {}, operator=True)
    prior_tools = f.show()["tools"]
    capture = {"kind": "conversation", "title": "Permission fixture",
               "entries": [{"kind": "fact", "text": "bounded capture persists"}]}
    before = f.run("tool", "run", "office.capture", "--input", json.dumps(capture),
                   "--json", success=False)
    assert before.returncode != 0 and "capability_denied" in before.stderr, before
    assert "plugin.tool.mutating" in before.stderr, before
    print("passed: legacy ordinary-agent capture denial", flush=True)
    current = f.export("candidate", candidate)
    f.run("plugin", "upgrade", "office", current, operator=True)
    inactive = f.enable()
    assert inactive["status"] == "inactive", inactive
    refused = f.run("tool", "run", "office.capture", "--input", json.dumps(capture),
                    "--json", success=False)
    assert refused.returncode != 0, refused
    f.enable(agent=True)
    shown = f.show()
    assert [(t["name"], t["execution_kind"], t["mcp_scope"]) for t in shown["tools"]] == [
        (t["name"], t["execution_kind"], t["mcp_scope"]) for t in prior_tools
    ], "agent consent must preserve the complete tool surface and MCP placement"
    selected = {t["name"].removeprefix("office.") for t in shown["tools"] if t["agent_callable"]}
    assert selected == SELECTED, selected
    assert all(t["mcp_scope"] == "workspace" for t in shown["tools"] if t["agent_callable"])
    permission = next(p for p in shown["permissions"] if p["grant"] == "agent_tools")
    assert permission["granted"] is True, permission
    captured = f.call("capture", capture)
    source = f.call("source-show", {"id": captured["id"]})
    assert "bounded capture persists" in json.dumps(source), source
    f.call("capture", {"append_to": captured["id"], "entries": ["handoff: still pending"]})
    source = f.call("source-show", {"id": captured["id"]})
    assert "still pending" in json.dumps(source), source
    pasted = f.call("capture", {"kind": "doc", "title": "Pasted source", "text": "source fixture"})
    assert "source fixture" in json.dumps(f.call("source-show", {"id": pasted["id"]}))
    private = f.call("capture", {"kind": "note", "title": "Private attachment fixture",
                                 "sensitivity": "private", "text": "withheld evidence"})
    f.call("selection-put", {"object": captured["id"], "view": "source", "quote": "bounded"}, operator=True)
    selected = f.call("selection-get", {})
    assert selected["selections"][0]["object"] == captured["id"], selected
    assert f.call("selection-get", {}) == {"selections": []}
    for tool in shown["tools"]:
        if tool["execution_kind"] == "mutating" and not tool["agent_callable"]:
            f.denied(tool["name"].removeprefix("office."))
    f.mcp_capture()
    audit = f.data("audit", "list", "--tool", "office.capture", operator=True)
    assert any(row["subcommand"] == "run" and row["role"] == "codex" for row in audit), audit
    assert any(row["subcommand"] == "run-mcp" and row["effective_capabilities"] == ["agent"]
               and row["status"] == "success" for row in audit), audit
    print("passed: consent, CLI/MCP capture readback, selection and all privileged refusals", flush=True)
    # A new verb invalidates the old witness even though agent_tools was granted.
    wider = candidate.replace("    - name: transcribe\n", "    - name: transcribe\n      agent_callable: true\n")
    widened = f.export("widened", wider)
    upgraded = f.data("plugin", "upgrade", "office", widened, operator=True)
    assert upgraded["status"] == "disabled" and upgraded["granted"] == [], upgraded
    assert any(p["grant"] == "agent_tools" and p["widened"] for p in upgraded["permission_changes"])
    assert f.run("tool", "run", "office.capture", "--input", json.dumps(capture),
                 "--json", success=False).returncode != 0
    f.run("plugin", "upgrade", "office", current, operator=True)
    f.enable(agent=True)
    print("passed: verb widening clears consent and disables dispatch; explicit re-consent restores candidate", flush=True)
    knowledge_before = f.call("knowledge-list", {})
    proposal = f.call("proposal-create", {
        "title": "Review fixture knowledge", "description": "Needs human approval",
        "evidence": [captured["id"] + "#L1"],
        "payload": {"kind": "knowledge_add", "page": {
            "id": "dom-fixture", "title": "Fixture", "body": "Proposed knowledge",
            "cites": [captured["id"] + "#L1"]}}})
    task = f.data("tool", "run", "orbit.task.show", "--input",
                  json.dumps({"id": proposal["id"], "model": "codex"}))
    assert task.get("task", task)["status"] == "proposed", task
    assert f.call("knowledge-list", {}) == knowledge_before, "proposals must not apply knowledge"
    attached = f.call("attach", {"task": proposal["id"],
                                 "tags": ["source:" + captured["id"], "source:" + private["id"]]})
    public_copy = next(item for item in attached["attached"] if item["id"] == captured["id"])
    assert not any(item["id"] == private["id"] for item in attached["attached"]), attached
    assert any(item["id"] == private["id"] and item["reason"] == "private"
               for item in attached["omitted"]), attached
    task = f.data("tool", "run", "orbit.task.show", "--input", json.dumps({
        "id": proposal["id"], "model": "codex", "fields": ["status", "artifacts"]}))
    assert task["status"] == "proposed", task
    paths = {item["path"] for item in task["artifacts"]}
    assert {attached["manifest"], public_copy["artifact_path"], "proposal.json"} <= paths, task
    artifact = f.data("tool", "run", "orbit.task.artifact.get", "--input", json.dumps({
        "id": proposal["id"], "path": public_copy["artifact_path"], "model": "codex"}))
    assert "bounded capture persists" in artifact["content"], artifact
    assert f.call("knowledge-list", {}) == knowledge_before, "attachment must not apply knowledge"
    print("passed: proposal pending approval, attachment readback and private-source withholding", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--orbit", required=True, type=Path)
    parser.add_argument("--office", required=True, type=Path)
    args = parser.parse_args()
    scratch = REPO / ".orbit/tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="orbit-office-agent-permissions-", dir="/tmp") as temporary:
        root = Path(temporary)
        try:
            check(Fixture(args.orbit.resolve(), args.office.resolve(), root))
        finally:
            # Preserve diagnostics even when a command or assertion fails,
            # then remove only this run's private live fixture.
            for name in ("commands.json", "mcp-results.json", "mcp.stderr"):
                source = root / name
                if source.is_file():
                    archived = scratch / f"agent-permissions-{root.name}-{name}"
                    shutil.copy2(source, archived)
                    print(f"evidence: {archived}", flush=True)
    print("public authorization regression: passed; private fixture removed")


if __name__ == "__main__":
    main()
