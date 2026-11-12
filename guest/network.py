"""Fail-closed guest egress. Loaded before provision.py; never changes the host."""
import ipaddress
import fcntl
import json
import os
from pathlib import Path
import pwd
import re
import shlex
import shutil
import subprocess
import time


def network_account():
    name = CONFIG["network"]["guard"]["service_user"]
    if not re.fullmatch(r"[a-z_][a-z0-9_-]{0,30}", name):
        raise ProvisionError("代理服务账号配置无效")
    try:
        account = pwd.getpwnam(name)
    except KeyError:
        command("useradd", "--system", "--no-create-home", "--user-group",
                "--shell", "/usr/bin/nologin", name)
        account = pwd.getpwnam(name)
    if account.pw_uid == 0 or account.pw_uid == CONFIG["guest"]["owner_uid"]:
        raise ProvisionError("代理服务必须使用独立系统账号")
    return account


def network_guard_rules(uid, host_access):
    network = CONFIG["network"]
    guard = network["guard"]
    table, tun = guard["table"], network["tun_interface"]
    if not re.fullmatch(r"[a-zA-Z0-9_]+", table) or not re.fullmatch(r"[a-zA-Z0-9_-]{1,15}", tun):
        raise ProvisionError("网络保护表名或接口名无效")
    addresses = [ipaddress.ip_network(value) for value in guard["blocked_destination_cidrs"]]
    private_rules = "\n".join(
        "    " + family + " daddr { " + ", ".join(str(value) for value in addresses if value.version == version) + " } counter drop"
        for family, version in [("ip", 4), ("ip6", 6)]
        if any(value.version == version for value in addresses))
    dhcp_server = str(ipaddress.IPv4Address(guard["dhcp_server"]))
    # Only the Omarchy account, only the host's SSH port; every other private
    # address and host port stays blocked.
    host_rule = ""
    if host_access:
        target = CONFIG["host_access"]
        host_rule = (f'    meta skuid {int(CONFIG["guest"]["owner_uid"])} ip daddr {ipaddress.IPv4Address(target["address"])} '
                     f'tcp dport {int(target["port"])} accept')
    # The desktop's terminal reaches sshd through QEMU's forward from the host
    # address; only replies on those connections may leave toward it. The
    # destination hook runs before conntrack (-200), so it checks the port and
    # the postrouting hook enforces that the packet is a reply.
    terminal_rule = (f'    ip daddr {ipaddress.IPv4Address(CONFIG["host_access"]["address"])} '
                     f'tcp sport {int(CONFIG["terminal"]["guest_port"])}')
    terminal_reply_rule = terminal_rule + " ct direction reply accept"
    terminal_rule += " accept"
    controls = f'''    ip daddr {{ 255.255.255.255, {dhcp_server} }} udp sport 68 udp dport 67 accept
    ip6 daddr {{ fe80::/10, ff02::/16 }} ip6 hoplimit 255 icmpv6 type {{ nd-router-solicit, nd-neighbor-solicit, nd-neighbor-advert }} accept'''
    # Two independent hooks: inspect destinations before sing-box redirects
    # them, then inspect the final route in postrouting (output still exposes
    # the old oif after sing-box's DNAT). Reject physical egress except the
    # proxy account and link configuration. Do not accept all established connections:
    # their routes could change to the physical NIC when the TUN disappears.
    return f'''add table inet {table}
flush table inet {table}
table inet {table} {{
  chain destination_guard {{
    type filter hook output priority -250; policy accept;
    oifname "lo" accept
    meta skuid {uid} accept
{controls}
    meta l4proto {{ tcp, udp }} th dport 53 accept
{host_rule}
{terminal_rule}
{private_rules}
  }}
  chain egress_guard {{
    type filter hook postrouting priority 10; policy drop;
    oifname "lo" accept
    meta skuid {uid} accept
    oifname "{tun}" accept
{host_rule}
{terminal_reply_rule}
{controls}
    counter drop
  }}
  chain forward_guard {{
    type filter hook forward priority 10; policy drop;
    counter drop
  }}
}}
'''


def replace_public(path, content, backup=None):
    if path.exists() and path.read_text() == content:
        return
    if backup is not None and path.exists():
        backup.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, backup / ("previous-" + path.name))
    pending = path.with_name(path.name + ".next")
    write_public(pending, content)
    os.replace(pending, path)


def install_network_guard(account, host_access, backup=None):
    guard = CONFIG["network"]["guard"]
    if shutil.which("nft") is None:
        raise ProvisionError("客机缺少 nftables，网络保持关闭")
    rules = network_guard_rules(account.pw_uid, host_access)
    # nft applies a batch atomically. A malformed replacement cannot remove
    # the active rules, and the NIC remains down until this step succeeds.
    command("nft", "--check", "-f", "-", input=rules.encode(), diagnostic=True)
    command("nft", "-f", "-", input=rules.encode(), diagnostic=True)
    rules_path = Path(guard["rules_path"])
    replace_public(rules_path, rules, backup)
    unit = (
        "[Unit]\nDescription=Omarchy Space fail-closed network guard\n"
        "DefaultDependencies=no\nWants=network-pre.target\n"
        "Before=network-pre.target shutdown.target\n"
        "After=systemd-sysctl.service local-fs.target\nConflicts=shutdown.target\n\n"
        "[Service]\nType=oneshot\nRemainAfterExit=yes\n"
        "ExecStart=/usr/bin/nft -f " + str(rules_path) + "\n\n"
        "[Install]\nWantedBy=multi-user.target\n"
    )
    # No ExecStop: stopping the service must never remove the packet filter.
    replace_public(Path("/etc/systemd/system") / (guard["service_name"] + ".service"), unit, backup)
    command("systemctl", "daemon-reload")
    command("systemctl", "enable", guard["service_name"])
    command("systemctl", "start", guard["service_name"])
    if command("systemctl", "is-active", guard["service_name"]) != "active":
        raise ProvisionError("断网保护服务未就绪")


def apply_network_policy(network, account):
    guard = CONFIG["network"]["guard"]
    outbounds = network.get("outbounds", [])
    if len(outbounds) != 1 or outbounds[0].get("tag") != "proxy" or outbounds[0].get("type") == "direct":
        raise ProvisionError("网络配置必须只使用所选代理节点")
    for inbound in network.get("inbounds", []):
        if inbound.get("type") == "tun":
            inbound["exclude_uid"] = [account.pw_uid]
            # Keep egress on the TUN rather than a second nftables NAT/NFQUEUE
            # redirect layer. The independent guard blocks physical bypasses.
            inbound["auto_redirect"] = False
            inbound["stack"] = CONFIG["network"]["tun_stack"]
            # Terminal replies always go back to the host address; host access
            # also connects to it. Both stay off the TUN.
            inbound["route_exclude_address"] = [CONFIG["host_access"]["address"] + "/32"]
    # Many nodes have no IPv6 exit, yet the TUN accepts the connection locally,
    # so an application that prefers IPv6 would wait until it times out.
    network.setdefault("dns", {})["strategy"] = CONFIG["network"]["dns_strategy"]
    route = network.setdefault("route", {})
    route["rules"] = [
        {"port": 53, "action": "hijack-dns"},
        {"action": "resolve", "server": "remote-dns", "strategy": CONFIG["network"]["dns_strategy"]},
        {"ip_version": 6, "action": "reject"},
        {"ip_cidr": guard["blocked_destination_cidrs"], "action": "reject"},
    ]
    route["final"] = "proxy"
    return network


def network_service_unit(account, encrypted):
    guest = CONFIG["guest"]
    guard = CONFIG["network"]["guard"]
    return (
        "[Unit]\nDescription=Omarchy Space sing-box\n"
        "Requires=" + guard["service_name"] + ".service\n"
        "After=" + guard["service_name"] + ".service network-online.target\n"
        "Wants=network-online.target\nBefore=multi-user.target\nStartLimitIntervalSec=0\n\n"
        "[Service]\nType=exec\nUser=" + account.pw_name + "\n"
        "CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_RAW CAP_NET_BIND_SERVICE\n"
        "AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW CAP_NET_BIND_SERVICE\n"
        "NoNewPrivileges=yes\nRestart=always\nRestartSec=" + str(guard["restart_seconds"]) + "\n"
        "LoadCredentialEncrypted=" + guest["credential_name"] + ":" + str(encrypted) + "\n"
        "ExecStart=/usr/bin/sing-box run -c %d/" + guest["credential_name"] + "\n"
        "ExecStartPost=+/usr/bin/python3 " + guard["dns_helper_path"] + "\n"
        "TimeoutStartSec=" + str(CONFIG["network"]["startup_timeout_seconds"] + 15) + "\n\n"
        "[Install]\nWantedBy=multi-user.target\n"
    )


def network_dns_helper():
    network = CONFIG["network"]
    addresses = [str(ipaddress.ip_interface(value).ip + 1) for value in network["tun_addresses"]]
    # The unprivileged proxy has NET_ADMIN, but systemd-resolved's D-Bus
    # authorization still requires root. Register DNS for every new TUN,
    # including automatic service restarts, without granting apps direct DNS.
    return ("import subprocess, time\nfrom pathlib import Path\n"
            "interface = " + repr(network["tun_interface"]) + "\n"
            "deadline = time.monotonic() + " + str(network["startup_timeout_seconds"]) + "\n"
            "while not (Path('/sys/class/net') / interface).exists():\n"
            "    if time.monotonic() >= deadline: raise SystemExit('TUN startup timed out')\n"
            "    time.sleep(0.1)\n"
            "for args in [[\"dns\", interface, *" + repr(addresses) + "], [\"domain\", interface, \"~.\"], [\"default-route\", interface, \"yes\"]]:\n"
            "    subprocess.run(['/usr/bin/resolvectl', *args], check=True, timeout=5)\n")


def install_reconnect_hook(backup):
    guard = CONFIG["network"]["guard"]
    if command("systemctl", "show", "NetworkManager.service", "-p", "LoadState", "--value") != "loaded":
        raise ProvisionError("客机缺少 NetworkManager，无法启用代理重连保护")
    service = shlex.quote(CONFIG["guest"]["service_name"])
    tun = shlex.quote(CONFIG["network"]["tun_interface"])
    lock = shlex.quote(guard["configuration_lock_path"])
    script = (
        "#!/bin/sh\n[ \"$1\" = " + tun + " ] && exit 0\n"
        "case \"$2\" in\n  up|dhcp4-change|dhcp6-change)\n"
        "    /usr/bin/flock -n -E 75 " + lock + " /usr/bin/systemctl --no-block start " + service + "\n"
        "    proxy_start_status=$?\n"
        "    [ \"$proxy_start_status\" -eq 75 ] && exit 0\n"
        "    exit \"$proxy_start_status\"\n    ;;\nesac\n"
    )
    path = Path(guard["reconnect_hook_path"])
    replace_public(path, script, backup)
    os.chown(path, 0, 0)
    path.chmod(0o755)


def wait_uplink():
    network = CONFIG["network"]
    deadline = time.monotonic() + network["startup_timeout_seconds"]
    while time.monotonic() < deadline:
        routes = json.loads(command("ip", "-j", "route", "show", "default"))
        for route in routes:
            name = route.get("dev")
            if name and name != network["tun_interface"]:
                carrier = Path("/sys/class/net") / name / "carrier"
                if carrier.exists() and carrier.read_text().strip() == "1":
                    return
        time.sleep(network["startup_poll_seconds"])
    raise ProvisionError("底层网络尚未取得地址，应用保持断网")


def prepare_network(host_access, network=None, backup=None):
    if backup is None:
        backup = Path(CONFIG["guest"]["state_directory"]) / "backups" / str(time.time_ns())
    lock_path = Path(CONFIG["network"]["guard"]["configuration_lock_path"])
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    with lock_path.open("a") as lock:
        lock_path.chmod(0o600)
        deadline = time.monotonic() + CONFIG["network"]["startup_timeout_seconds"]
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise ProvisionError("代理网络正在配置，请等待当前操作完成") from None
                time.sleep(CONFIG["network"]["startup_poll_seconds"])
        install_reconnect_hook(backup)
        return prepare_network_locked(host_access, network, backup)


def prepare_network_locked(host_access, network, backup):
    guest = CONFIG["guest"]
    guard = CONFIG["network"]["guard"]
    account = network_account()
    unit_path = Path("/etc/systemd/system") / (guest["service_name"] + ".service")
    # Each boot starts with the virtual NIC disconnected, including upgrades
    # from an earlier version whose network unit still runs as root.
    if unit_path.exists():
        command("systemctl", "stop", guest["service_name"])
    install_network_guard(account, host_access, backup)
    encrypted = Path(guest["credential_directory"]) / guest["credential_name"]
    previous = None
    if network is None:
        raw = command("systemd-creds", "--name=" + guest["credential_name"],
                      "decrypt", str(encrypted), "-")
        network = json.loads(raw)
        previous = json.dumps(network, sort_keys=True)
        raw = None
    network = apply_network_policy(network, account)
    config = json.dumps(network).encode()
    try:
        command("sing-box", "check", "-c", "/dev/stdin", input=config)
    except ProvisionError:
        raise NodeError("这个节点的配置没有通过 sing-box 校验。请检查订阅链接，或换一个节点") from None
    if previous != json.dumps(network, sort_keys=True):
        encrypted.parent.mkdir(parents=True, exist_ok=True)
        if encrypted.exists():
            backup.mkdir(parents=True, exist_ok=True)
            shutil.copy2(encrypted, backup / "previous-credential.encrypted")
        pending = encrypted.with_name(encrypted.name + ".next")
        command("systemd-creds", "--with-key=host", "--name=" + guest["credential_name"],
                "encrypt", "-", str(pending), input=config)
        pending.chmod(0o600)
        os.replace(pending, encrypted)
    config = None
    replace_public(Path(guard["dns_helper_path"]), network_dns_helper(), backup)
    replace_public(unit_path, network_service_unit(account, encrypted), backup)
    command("systemctl", "daemon-reload")
    command("systemctl", "enable", guest["service_name"])
    report("network_guard_ready", "断网保护已启用，正在启动 sing-box",
           network_guard_version=guard["version"])
    acknowledgement = json.loads(reader.readline())
    if acknowledgement != {"network_up": True}:
        raise ProvisionError("宿主尚未确认受保护网络链路")
    wait_uplink()
    command("systemctl", "restart", guest["service_name"])
    return {"network_guard_version": guard["version"], "network_guard_active": True,
            "proxy_required": True, "proxy_service_user": account.pw_name,
            "proxy_reconnect_autostart": True}
