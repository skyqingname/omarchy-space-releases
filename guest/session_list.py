"""List the workspace's Claude Code sessions for the chat's history.

The desktop sends this source on stdin and one JSON argument: the paths of
`terminal` in product.config.json. Output is one JSON list, newest first, of
{"id", "title", "modified"}: the title Claude gave the session, or its first
prompt until it has one; `modified` is seconds since the epoch.
"""
import glob
import json
import os
import re
import sys

cfg = json.loads(sys.argv[1])
home = os.path.expanduser("~")
cwd = os.path.realpath(os.path.join(home, cfg["working_directory"]))
directory = os.path.join(home, cfg["claude_projects_path"], re.sub(r"[^A-Za-z0-9]", "-", cwd))


def title(path):
    named, asked = "", ""
    with open(path, "rb") as file:
        for line in file:
            if b'"type":"ai-title"' in line:
                named = json.loads(line).get("aiTitle") or named
            elif not asked and b'"type":"user"' in line:
                record = json.loads(line)
                content = record.get("message", {}).get("content")
                if record.get("type") == "user" and isinstance(content, str) and not record.get("isMeta") and not content.startswith("<"):
                    asked = content.strip()
    return named or asked[:cfg["history_title_chars"]]


paths = sorted(glob.glob(os.path.join(directory, "*.jsonl")), key=os.path.getmtime, reverse=True)
sessions = [{"id": os.path.basename(path)[:-len(".jsonl")], "title": title(path), "modified": os.path.getmtime(path)}
            for path in paths[:cfg["history_limit"]]]
print(json.dumps([session for session in sessions if session["title"]]))
