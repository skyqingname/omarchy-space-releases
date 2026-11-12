"""Show the VM's browser on its own headless screen and stream that screen.

The desktop runs this over SSH as the account, with one JSON argument holding
the browser settings and the first size. It prints {"ready": true} once wayvnc
listens (plain VNC on the guest loopback; wayvnc's websocket listener crashes
on connect in this image), then {"reserved": [left, top, right, bottom]}
whenever the area Omarchy's bar keeps changes, and reads size updates
({"width", "height", "scale"}) as JSON lines on stdin. When the desktop disconnects, stdin closes:
wayvnc stops and the screen is removed, so the browser windows return to the
VM's own display.

Hyprland here is configured in Lua: settings go through `hyprctl eval` and
dispatchers are hl.dsp calls. Omarchy gives every screen its own block of
numbered workspaces, so the browser lives on whichever workspace the new
screen shows, maximized. Every screen also gets Omarchy's bar, and fullscreen
neither covers it nor keeps Chromium's tabs, so the desktop crops the reserved
area instead.
"""
import glob
import json
import os
import socket
import subprocess
import sys
import threading
import time

settings = json.loads(sys.argv[1])
runtime = os.environ["XDG_RUNTIME_DIR"]
instance = max(glob.glob(runtime + "/hypr/*/.socket.sock"), key=os.path.getmtime).split("/")[-2]
display = next(path for path in sorted(glob.glob(runtime + "/wayland-*")) if not path.endswith(".lock"))
env = dict(os.environ, HYPRLAND_INSTANCE_SIGNATURE=instance, WAYLAND_DISPLAY=os.path.basename(display))
output = settings["output"]


def hypr(*args):
    return subprocess.run(["hyprctl", *args], env=env, capture_output=True, text=True, check=True).stdout.strip()


def apply(*args):
    reply = hypr(*args)
    if reply != "ok":
        raise SystemExit("hyprctl " + " ".join(args) + ": " + reply)


def query(*args):
    return json.loads(hypr("-j", *args))


def resize(size):
    apply("eval", f'hl.monitor({{ output = "{output}", mode = "{size["width"]}x{size["height"]}@{settings["refresh_rate"]}", '
                  f'position = "auto", scale = {size["scale"]} }})')


def dispatch(call):
    apply("dispatch", call)


if output not in [monitor["name"] for monitor in query("monitors", "all")]:
    apply("output", "create", "headless", output)
resize(settings)
launched = 0.0


def place():
    global launched, reserved, bare
    screen = next(monitor for monitor in query("monitors") if monitor["name"] == output)
    target = screen["activeWorkspace"]["id"]
    # The window should fill the screen edge to edge, without Omarchy's gaps and border.
    if target != bare:
        apply("eval", f'hl.workspace_rule({{ workspace = "{target}", gaps_in = 0, gaps_out = 0, no_border = true }})')
        bare = target
    if screen["reserved"] != reserved:
        reserved = screen["reserved"]
        print(json.dumps({"reserved": reserved}), flush=True)
    browsers = [client for client in query("clients") if client["class"] == settings["window_class"]]
    if not browsers and time.monotonic() - launched >= settings["relaunch_seconds"]:
        dispatch(f'hl.dsp.exec_cmd("{settings["launch_command"]}")')
        launched = time.monotonic()
    for client in browsers:
        window = f'"address:{client["address"]}"'
        if client["workspace"]["id"] != target:
            dispatch(f'hl.dsp.window.move({{ workspace = "{target}", follow = false, window = {window} }})')
    # A newly focused window (a sign-in popup) takes over the maximized state on its own.
    shown = [client for client in browsers if client["workspace"]["id"] == target]
    if shown and not any(client["fullscreen"] for client in shown):
        newest = min(shown, key=lambda client: client["focusHistoryID"])
        dispatch(f'hl.dsp.window.fullscreen({{ mode = "maximized", action = "set", window = "address:{newest["address"]}" }})')
    return browsers


reserved = bare = None
vnc = subprocess.Popen(["wayvnc", "--output=" + output, "--max-fps=" + str(settings["max_fps"]),
                        f'127.0.0.1:{settings["port"]}'], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
deadline = time.monotonic() + settings["ready_timeout_seconds"]
while True:
    try:
        socket.create_connection(("127.0.0.1", settings["port"]), timeout=1).close()
        break
    except OSError:
        if vnc.poll() is not None or time.monotonic() >= deadline:
            raise SystemExit("wayvnc did not start")
        time.sleep(settings["poll_seconds"])
print(json.dumps({"ready": True}), flush=True)
place()

closed = threading.Event()


def updates():
    for line in sys.stdin:
        resize(json.loads(line))
    closed.set()


threading.Thread(target=updates, daemon=True).start()
# Windows opened later (a sign-in page, a link from Claude) join the stream too.
while not closed.is_set() and vnc.poll() is None:
    place()
    closed.wait(settings["poll_seconds"])
vnc.terminate()
vnc.wait()
# Back on the VM's own screen the windows should look as they did before.
for client in query("clients"):
    if client["class"] == settings["window_class"] and client["fullscreen"]:
        dispatch(f'hl.dsp.window.fullscreen({{ mode = "maximized", action = "unset", window = "address:{client["address"]}" }})')
apply("output", "remove", output)
