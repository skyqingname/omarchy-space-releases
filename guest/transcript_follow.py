"""Tell the desktop what the chat's Claude Code is doing, over SSH.

The desktop sends this source on stdin and one JSON argument: the paths of
`terminal` in product.config.json plus the session to launch. Before Claude
starts, this writes the settings file Claude is launched with: the product's
settings, hooks that append what Claude reports to a file here, and a status
line command that keeps the last status Claude handed it. A status line of the
account's own is still run on the same input and shows as before.

Output is JSON lines. The transcript's own records pass through, limited to
the kinds the chat shows and without image data or structured tool results.
Of the attachments only a message taken from the queue mid-turn is one.
Marker lines:
{"gotocs_ready": settings path} once Claude may be launched,
{"gotocs_session": id} whenever the followed transcript changes (/clear and
/resume move Claude to another one, which its SessionStart hook reports),
{"gotocs_live": true} where existing history ends and new activity begins,
{"gotocs_status": {...}} whenever sign-in, onboarding or folder trust changes,
{"gotocs_hook": {...}} for each hook Claude ran,
{"gotocs_meter": {...}} whenever the status line reports new figures.
"""
import json
import os
import re
import sys
import time

cfg = json.loads(sys.argv[1])
home = os.path.expanduser("~")
cwd = os.path.realpath(os.path.join(home, cfg["working_directory"]))
directory = os.path.join(home, cfg["claude_projects_path"], re.sub(r"[^A-Za-z0-9]", "-", cwd))
runtime = os.path.join(home, cfg["runtime_directory"])
hooks_path = os.path.join(runtime, cfg["hooks_name"])
meter_path = os.path.join(runtime, cfg["meter_name"])
settings_path = os.path.join(runtime, cfg["settings_name"])
kinds = set(cfg["record_types"])
# Most of a transcript is request bookkeeping, attachments above all; only
# lines naming a shown kind, or a shown kind of attachment, are parsed.
marks = [('"type":"' + kind + '"').encode() for kind in (kinds - {"attachment"}) | set(cfg["attachment_types"])]
out = sys.stdout.buffer
parent = os.getppid()


def emit(value):
    out.write(json.dumps(value).encode() + b"\n")


def quote(text):
    return "'" + text.replace("'", "'\\''") + "'"


def read_json(path):
    try:
        with open(path) as file:
            return json.load(file)
    except (OSError, ValueError):
        return {}


def prepare():
    os.makedirs(runtime, exist_ok=True)
    for path in (hooks_path, meter_path):
        open(path, "w").close()
    settings = dict(cfg["claude_settings"])
    append = {"hooks": [{"type": "command", "command": "{ cat; echo; } >> " + quote(hooks_path)}]}
    settings["hooks"] = {name: [append] for name in cfg["hook_events"]}
    keep = f"s=$(cat); printf '%s\\n' \"$s\" > {quote(meter_path + '.new')} && mv -f {quote(meter_path + '.new')} {quote(meter_path)}"
    own = read_json(os.path.join(home, cfg["claude_user_settings_path"])).get("statusLine")
    if isinstance(own, dict) and own.get("type") == "command" and isinstance(own.get("command"), str):
        settings["statusLine"] = {**own, "command": keep + "; printf '%s' \"$s\" | {\n" + own["command"] + "\n}"}
    else:
        settings["statusLine"] = {"type": "command", "command": keep}
    with open(settings_path, "w") as file:
        json.dump(settings, file)
    emit({"gotocs_ready": settings_path})
    out.flush()


def status():
    state = read_json(os.path.join(home, cfg["claude_state_path"]))
    project = state.get("projects", {}).get(cwd, {})
    return {"signed_in": os.path.isfile(os.path.join(home, cfg["claude_credentials_path"])),
            "onboarded": state.get("hasCompletedOnboarding") is True,
            "trusted": project.get("hasTrustDialogAccepted") is True}


def lean(content):
    # Pictures and attached documents stay in the VM; the chat only counts them.
    if not isinstance(content, list):
        return content
    blocks = []
    for block in content:
        if isinstance(block, dict) and block.get("type") in ("image", "document"):
            block = {"type": block["type"]}
        elif isinstance(block, dict) and block.get("type") == "tool_result":
            block = {**block, "content": lean(block.get("content"))}
        blocks.append(block)
    return blocks


def forward(line):
    if not any(mark in line for mark in marks):
        return
    try:
        record = json.loads(line)
    except ValueError:
        return
    if record.get("type") not in kinds or record.get("isSidechain") or record.get("subtype") in cfg["skipped_system_subtypes"]:
        return
    if record["type"] == "attachment" and record["attachment"].get("type") not in cfg["attachment_types"]:
        return
    record.pop("toolUseResult", None)
    message = record.get("message")
    if isinstance(message, dict):
        message["content"] = lean(message.get("content"))
    emit(record)


class Tail:
    """Whole lines appended to a file since it was last read."""

    def __init__(self, path):
        self.path, self.handle, self.rest = path, None, b""

    def lines(self):
        if self.handle is None:
            if not os.path.isfile(self.path):
                return
            self.handle = open(self.path, "rb")
        while chunk := self.handle.read(cfg["read_chunk_bytes"]):
            *lines, self.rest = (self.rest + chunk).split(b"\n")
            yield from lines


def follow(session, path):
    """Start on another transcript: what it already holds is history."""
    global transcript
    transcript = Tail(path)
    emit({"gotocs_session": session})
    for line in transcript.lines():
        forward(line)
    emit({"gotocs_live": True})


prepare()
transcript = None
hooks = Tail(hooks_path)
follow(cfg["session"], os.path.join(directory, cfg["session"] + ".jsonl"))
followed, reported, checked, metered = cfg["session"], None, 0.0, 0
while os.getppid() == parent:
    if time.monotonic() >= checked:
        value = status()
        if value != reported:
            emit({"gotocs_status": value})
            reported = value
        checked = time.monotonic() + cfg["status_poll_seconds"]
    # The transcript before the hooks: what ended a turn is read before the
    # prompt that began the next one.
    for line in transcript.lines():
        forward(line)
    for line in hooks.lines():
        try:
            hook = json.loads(line)
        except ValueError:
            continue
        if hook.get("hook_event_name") == "SessionStart" and hook.get("session_id") != followed:
            followed = hook["session_id"]
            follow(followed, hook["transcript_path"])
        emit({"gotocs_hook": {key: hook[key] for key in cfg["hook_fields"] if key in hook}})
    try:
        changed = os.stat(meter_path).st_mtime_ns
    except OSError:
        changed = metered
    if changed != metered:
        metered = changed
        meter = read_json(meter_path)
        if meter:
            emit({"gotocs_meter": {key: meter[key] for key in cfg["meter_fields"] if key in meter}})
    out.flush()
    time.sleep(cfg["follow_poll_seconds"])
