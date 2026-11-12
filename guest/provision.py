"""Boot-time provisioning over a dedicated virtio serial device.

The host supplies CONFIG (public product parameters) before this source.
Node credentials are received over the port, never embedded in the boot unit.
"""
import io
import json
import os
import re
from pathlib import Path
import pwd
import socket
import subprocess
import time
import urllib.request
import urllib.error


class ProvisionError(Exception):
    pass


class NodeError(ProvisionError):
    """The link in the app is the only source of the node, so this is fixed there."""
    def __init__(self, message, detail=""):
        super().__init__(message)
        self.detail = detail


def command(*args, input=None, diagnostic=False, timeout=None):
    try:
        result = subprocess.run(args, input=input, capture_output=True,
                                timeout=timeout or CONFIG["guest"]["command_timeout_seconds"])
    except subprocess.TimeoutExpired:
        raise ProvisionError(args[0] + " 执行超时") from None
    if result.returncode:
        # Subprocess diagnostics can contain the node password or URL.
        detail = (result.stderr or result.stdout).decode(errors="replace").strip()[-300:] if diagnostic else ""
        raise ProvisionError(args[0] + " failed (exit " + str(result.returncode) + ")" + (": " + detail if detail else ""))
    return result.stdout.decode().strip()


def report(phase, message, **extra):
    pending = memoryview((json.dumps({"phase": phase, "message": message, **extra}) + "\n").encode())
    while pending:
        written = channel.write(pending)
        if not written:
            raise ProvisionError("客机配置回执通道已关闭")
        pending = pending[written:]


def write_public(path, content, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    path.chmod(mode)


def wait_network_ready():
    network = CONFIG["network"]
    deadline = time.monotonic() + network["startup_timeout_seconds"]
    while time.monotonic() < deadline:
        service = subprocess.run(["systemctl", "is-active", CONFIG["guest"]["service_name"]], capture_output=True, text=True).stdout.strip()
        if service == "failed":
            raise NodeError("这个节点无法启动 sing-box。请检查订阅链接，或换一个节点")
        if service == "active" and (Path("/sys/class/net") / network["tun_interface"]).exists():
            try:
                with socket.create_connection(("127.0.0.1", network["mixed_port"]), timeout=network["startup_poll_seconds"]):
                    return
            except OSError:
                pass
        time.sleep(network["startup_poll_seconds"])
    raise NodeError("这个节点没有在规定时间内就绪。请检查订阅链接，或换一个节点")


def verify_network():
    wait_network_ready()
    direct = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    explicit = urllib.request.build_opener(urllib.request.ProxyHandler({
        "https": "http://127.0.0.1:" + str(CONFIG["network"]["mixed_port"])
    }))
    attempts = {}
    def trace_request(opener, label):
        request = urllib.request.Request(CONFIG["network"]["probe_url"], headers={"User-Agent": CONFIG["subscription"]["user_agent"]})
        limit = CONFIG["network"]["probe_attempts"]
        for attempt in range(1, limit + 1):
            try:
                with opener.open(request, timeout=CONFIG["network"]["probe_timeout_seconds"]) as response:
                    trace = dict(line.split("=", 1) for line in response.read().decode().splitlines() if "=" in line)
                if not trace.get("ip") or not trace.get("loc"):
                    raise ProvisionError(label + "出口探测响应缺少 IP 或地区")
                attempts[label] = attempt
                return trace
            except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
                # Read-only probes can meet a transient reset just after startup.
                # Retry the same protected path; never fall back to direct egress.
                retryable = not isinstance(error, urllib.error.HTTPError) or error.code >= 500
                if attempt == limit or not retryable:
                    reason = getattr(error, "reason", str(error))
                    raise NodeError("节点连不通，访问互联网失败（已试 " + str(attempt) + " 次）。请检查订阅链接，或换一个节点",
                                    label + "：" + str(reason)) from None
                time.sleep(CONFIG["network"]["probe_retry_seconds"])
    proxy_trace = trace_request(explicit, "显式代理")
    trace = trace_request(direct, "TUN")
    if trace["ip"] != proxy_trace["ip"]:
        raise NodeError("这个节点的两次出口不一致。请换一个节点")
    # Applications choose addresses differently from urllib, so check one as well.
    try:
        command("curl", "-fsS", "-o", "/dev/null", "-m", str(CONFIG["network"]["probe_timeout_seconds"]),
                CONFIG["network"]["probe_url"], diagnostic=True)
    except ProvisionError as error:
        raise NodeError("节点连不通，普通程序无法访问互联网。请检查订阅链接，或换一个节点", str(error)) from None
    return {"egress_country": trace["loc"], "tun_matches_proxy": True, "exit_ip": trace["ip"],
            "tun_interface_present": True, "network_probe_attempts": attempts}, trace["ip"]


def owner():
    wait_seconds = CONFIG["guest"]["owner_wait_seconds"]
    deadline = None if wait_seconds is None else time.monotonic() + wait_seconds
    next_report = time.monotonic() + CONFIG["guest"]["owner_heartbeat_seconds"]
    while True:
        try:
            account = pwd.getpwuid(CONFIG["guest"]["owner_uid"])
            if Path(account.pw_dir).is_dir() and not Path(CONFIG["guest"]["owner_pending_path"]).exists():
                return account
        except KeyError:
            pass
        if deadline is not None and time.monotonic() >= deadline:
            raise ProvisionError("guest account setup was not completed in time")
        if time.monotonic() >= next_report:
            report("account", "等待你在 Omarchy 窗口中完成首次账号设置")
            next_report = time.monotonic() + CONFIG["guest"]["owner_heartbeat_seconds"]
        time.sleep(CONFIG["guest"]["owner_poll_seconds"])


def verify_agent(account):
    version = command("runuser", "--login", account.pw_name, "--command", "claude --version")
    selected = command("runuser", "--login", account.pw_name, "--command", "omarchy-default-agent")
    if selected != "claude":
        raise ProvisionError("default agent selection was not applied")
    return {"claude_version": version, "default_agent": selected}


def install_claude(account, backup):
    binary = Path(account.pw_dir) / CONFIG["guest"]["claude_path"]
    # The official installer leaves a link; a node change must not reinstall.
    if binary.is_symlink() and binary.exists():
        return
    # Otherwise this path holds Omarchy's lazy mise wrapper, which must not win.
    if binary.is_file() and not binary.is_symlink():
        (backup / "previous-claude").write_bytes(binary.read_bytes())
    binary.unlink(missing_ok=True)
    # The installer runs as the account, so the directory has to be the account's.
    binary.parent.mkdir(parents=True, exist_ok=True)
    os.chown(binary.parent, account.pw_uid, account.pw_gid)
    installer = CONFIG["guest"]["claude_installer"]
    script = "set -o pipefail; curl -fsSL " + installer["url"] + " | bash"
    for attempt in range(1, installer["attempts"] + 1):
        try:
            command("runuser", "--login", account.pw_name, "--command", script,
                    diagnostic=True, timeout=installer["timeout_seconds"])
            return
        except ProvisionError as error:
            if attempt == installer["attempts"]:
                raise NodeError("Claude Code 官方安装失败（已试 " + str(attempt) + " 次）。请确认节点能访问 claude.ai，或换一个节点",
                                str(error)) from None
            time.sleep(installer["retry_seconds"])


def managed_block(path, begin, end, block, account):
    text = path.read_text() if path.exists() else ""
    text = re.sub(re.escape(begin) + r".*?" + re.escape(end) + r"\n", "", text, flags=re.S)
    if block is None and not path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text + (begin + "\n" + block + end + "\n" if block else ""))
    path.chmod(0o600)
    os.chown(path, account.pw_uid, account.pw_gid)
    os.chown(path.parent, account.pw_uid, account.pw_gid)


def configure_host_access(account, host):
    settings = CONFIG["host_access"]
    home = Path(account.pw_dir)
    key = home / ".ssh" / settings["key_comment"]
    ssh_config = None if host is None else (
        "Host " + settings["alias"] + "\n  HostName " + settings["address"] + "\n  Port " + str(settings["port"]) +
        "\n  User \"" + host["user"] + "\"\n  IdentityFile " + str(key) +
        "\n  IdentitiesOnly yes\n  StrictHostKeyChecking accept-new\n  ServerAliveInterval 30\n")
    guidance = None if host is None else (
        "## 宿主机\n\n这台虚拟机可以用 `ssh " + settings["alias"] + "` 操作用户的宿主机（" + host["platform"] + "，用户 " + host["user"] + "）。\n"
        "用户要读写、编辑宿主机文件或在宿主机运行命令时，都用 `ssh " + settings["alias"] + " '<命令>'`。\n")
    managed_block(home / ".ssh/config", "# omarchy-space begin", "# omarchy-space end", ssh_config, account)
    managed_block(home / ".claude/CLAUDE.md", "<!-- omarchy-space begin -->", "<!-- omarchy-space end -->", guidance, account)
    if host is None:
        return {"host_access_active": False}
    (home / ".ssh").chmod(0o700)
    if not key.exists():
        command("runuser", "-u", account.pw_name, "--", "ssh-keygen", "-q", "-t", "ed25519", "-N", "",
                "-C", settings["key_comment"], "-f", str(key))
    return {"host_access_active": True, "host_public_key": key.with_name(key.name + ".pub").read_text().strip()}


def configure_terminal(account, terminal):
    settings = CONFIG["terminal"]
    home = Path(account.pw_dir)
    marker = settings["authorized_keys_marker"]
    managed_block(home / ".ssh/authorized_keys", "# " + marker + " begin", "# " + marker + " end",
                  terminal["public_key"] + "\n", account)
    (home / ".ssh").chmod(0o700)
    command("ufw", "allow", "proto", "tcp", "from", CONFIG["host_access"]["address"],
            "to", "any", "port", str(settings["guest_port"]), diagnostic=True)
    command("systemctl", "enable", "--now", settings["ssh_service"], diagnostic=True)
    return {"terminal_user": account.pw_name}


def install_browser_stream():
    # Streams the VM browser into the desktop app; already installed is a no-op.
    command("pacman", "-S", "--needed", "--noconfirm", CONFIG["browser"]["package"], diagnostic=True)


def clipboard_receiver(account):
    settings = CONFIG["guest"]["clipboard"]
    port = "/dev/virtio-ports/" + CONFIG["vm"]["clipboard_port_name"]
    runtime = "/run/user/" + str(account.pw_uid)
    # The port is opened read-only: text only ever arrives from the host.
    return ("import glob, struct, subprocess, time\n"
            "def read(channel, size):\n"
            "    data = b''\n"
            "    while len(data) < size:\n"
            "        chunk = channel.read(size - len(data))\n"
            "        if not chunk:\n"
            "            raise SystemExit(1)\n"
            "        data += chunk\n"
            "    return data\n"
            "def session():\n"
            "    while True:\n"
            "        sockets = [path for path in glob.glob(" + repr(runtime + "/wayland-*") + ") if not path.endswith('.lock')]\n"
            "        if sockets:\n"
            "            return sockets[0].rsplit('/', 1)[1]\n"
            "        time.sleep(" + repr(settings["session_poll_seconds"]) + ")\n"
            "with open(" + repr(port) + ", 'rb', buffering=0) as channel:\n"
            "    while True:\n"
            "        text = read(channel, struct.unpack('>I', read(channel, 4))[0])\n"
            "        subprocess.run(['runuser', '-u', " + repr(account.pw_name) + ", '--', 'env', 'XDG_RUNTIME_DIR=" + runtime + "',\n"
            "                        'WAYLAND_DISPLAY=' + session(), 'wl-copy'], input=text,\n"
            "                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)\n")


def install_clipboard_receiver(account):
    settings = CONFIG["guest"]["clipboard"]
    script = Path(settings["receiver_path"])
    replace_public(script, clipboard_receiver(account))
    replace_public(Path("/etc/systemd/system") / (settings["service_name"] + ".service"),
                   "[Unit]\nDescription=Omarchy Space host clipboard\n\n"
                   "[Service]\nExecStart=/usr/bin/python3 " + str(script) + "\n"
                   "Restart=always\nRestartSec=" + str(settings["restart_seconds"]) + "\n\n"
                   "[Install]\nWantedBy=multi-user.target\n")
    command("systemctl", "daemon-reload")
    command("systemctl", "enable", settings["service_name"])
    command("systemctl", "restart", settings["service_name"])


def configure(payload):
    guest = CONFIG["guest"]
    state = Path(guest["state_directory"])
    state.mkdir(parents=True, exist_ok=True)
    report("network_install", "正在客机内安装 sing-box")
    package = Path("/run/omarchy-space-sing-box.pkg.tar.zst")
    receive_file(package, payload["sing_box_bytes"])
    command("pacman", "-U", "--noconfirm", str(package))
    backup = state / "backups" / str(time.time_ns())
    backup.mkdir(parents=True)
    protection = prepare_network(payload["host"] is not None, payload["network"], backup)
    report("account", "请在 Omarchy 窗口中完成首次账号设置")
    account = owner()
    install_clipboard_receiver(account)
    host_access = configure_host_access(account, payload["host"])
    terminal = configure_terminal(account, payload["terminal"])
    report("region", "正在按所选节点的实测出口设置语言、地区和固定时区")
    network, exit_ip = verify_network()
    region = measured_region(exit_ip)
    apply_region(region, account, backup)
    report("claude_install", "正在通过官方脚本安装 Claude Code（约 300 MB），节点较慢时需要几分钟")
    install_claude(account, backup)
    install_browser_stream()
    default = Path(account.pw_dir) / guest["default_agent_path"]
    if default.exists():
        (backup / "previous-default-agent").write_bytes(default.read_bytes())
    default.parent.mkdir(parents=True, exist_ok=True)
    default.write_text("claude\n")
    os.chown(default, account.pw_uid, account.pw_gid)
    os.chown(default.parent, account.pw_uid, account.pw_gid)
    agent = verify_agent(account)
    report("verify", "正在核验服务、默认 Agent 和出口地区设置")
    verify_region(region, account)
    result = {"phase": "restart_required", "message": "出口地区设置已保存，请重启环境让桌面和浏览器使用新语言",
              "sing_box_version": command("sing-box", "version").splitlines()[0],
              "backup_directory": str(backup), "region": region, "needs_session_restart": True,
              "region_applied_boot_id": Path(CONFIG["region"]["boot_id_path"]).read_text().strip(), **agent, **network, **protection, **host_access, **terminal}
    write_public(state / "installed.json", json.dumps(result))
    report(**result)


def receive_file(path, size):
    with path.open("wb") as target:
        remaining = size
        while remaining:
            chunk = reader.read(min(remaining, io.DEFAULT_BUFFER_SIZE * 128))
            if not chunk:
                raise ProvisionError("offline component transfer interrupted")
            target.write(chunk)
            remaining -= len(chunk)


port = "/dev/virtio-ports/" + CONFIG["vm"]["provision_port_name"]
with open(port, "r+b", buffering=0) as channel:
    reader = io.BufferedReader(channel, buffer_size=io.DEFAULT_BUFFER_SIZE * 128)
    marker = Path(CONFIG["guest"]["state_directory"]) / "installed.json"
    report("connected", "客机配置通道已连接", installed=marker.exists())
    payload = json.loads(reader.readline())
    try:
        if payload["action"] == "configure":
            configure(payload)
        elif payload["action"] == "status":
            result = json.loads(marker.read_text())
            protection = prepare_network(payload["host"] is not None)
            network, exit_ip = verify_network()
            install_browser_stream()
            if "region" not in result:
                raise ProvisionError("此环境尚未应用出口地区策略，请重新配置网络")
            # The same exit keeps its region, so a daily start needs no lookup service.
            region = result["region"] if result.get("exit_ip") == exit_ip else measured_region(exit_ip)
            if any(region[key] != result["region"][key] for key in ["country", "timezone", "locale", "browser_language", "preferred_languages"]):
                raise NodeError("这个节点的出口地区变了。请到网络节点重新选择，让语言和时区跟着更新")
            account = owner()
            install_clipboard_receiver(account)
            result.pop("host_public_key", None)
            result.update(configure_host_access(account, payload["host"]))
            result.update(configure_terminal(account, payload["terminal"]))
            verify_region(region, account)
            result.update(network)
            result.update(protection)
            result["region"] = region
            result.update(verify_agent(account))
            result["sing_box_version"] = command("sing-box", "version").splitlines()[0]
            needs_restart = result["region_applied_boot_id"] == Path(CONFIG["region"]["boot_id_path"]).read_text().strip()
            result["needs_session_restart"] = needs_restart
            result["phase"] = "restart_required" if needs_restart else "ready"
            result["message"] = "请重启环境应用出口语言" if needs_restart else "环境已就绪，可以开始对话"
            write_public(marker, json.dumps(result, ensure_ascii=False))
            report(**result)
    except NodeError as error:
        report("failed", str(error), failure={"kind": "node", "detail": error.detail})
    except Exception as error:
        message = str(error) if isinstance(error, ProvisionError) else type(error).__name__
        report("failed", "客机配置未完成：" + message)
        # Keep the stage in the desktop record; no raw payload/error text is logged.
