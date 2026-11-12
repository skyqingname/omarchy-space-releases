"""Apply the selected exit's measured region. Loaded before provision.py."""
import ipaddress
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time
import urllib.request
from zoneinfo import ZoneInfo


def measured_region(exit_ip):
    policy = CONFIG["region"]
    geo = policy["geolocation"]
    ip = str(ipaddress.ip_address(exit_ip))
    proxy = urllib.request.build_opener(urllib.request.ProxyHandler({
        "https": "http://127.0.0.1:" + str(CONFIG["network"]["mixed_port"])
    }))
    request = urllib.request.Request(geo["url_template"].format(ip=ip),
                                     headers={"User-Agent": geo["user_agent"]})
    for attempt in range(1, geo["attempts"] + 1):
        try:
            with proxy.open(request, timeout=geo["timeout_seconds"]) as response:
                location = json.load(response)
            break
        except (OSError, ValueError) as error:
            if attempt == geo["attempts"]:
                raise NodeError("无法查询这个节点的出口地区，尚未套用任何地区默认值。请稍后重试，或换一个节点",
                                str(error)) from None
            time.sleep(geo["retry_seconds"])
    fields = geo["fields"]
    if not all(location.get(fields[key]) for key in ["ip", "country", "timezone"]):
        raise ProvisionError("出口定位没有提供完整的 IP、国家和时区")
    if ipaddress.ip_address(location[fields["ip"]]) != ipaddress.ip_address(ip):
        raise ProvisionError("定位返回的 IP 与所选节点实测出口不一致")
    country = location[fields["country"]]
    subdivision = location.get(fields["subdivision"])
    key = country + ":" + str(subdivision)
    if key in policy["subdivision_profiles"]:
        profile = policy["subdivision_profiles"][key]
    elif country in policy["country_profiles"]:
        profile = policy["country_profiles"][country]
    else:
        raise ProvisionError("实测出口地区尚未配置语言映射，请补充 product.config.json")
    zone = location[fields["timezone"]]
    ZoneInfo(zone)
    return {"country": country, "subdivision": subdivision, "timezone": zone,
            "timezone_mode": policy["timezone_mode"], **profile}


def region_adapter():
    adapters = CONFIG["region"]["timezone_adapters"]
    architecture = platform.machine()
    if architecture not in adapters:
        raise ProvisionError("当前客机尚未配置固定时区适配器")
    return adapters[architecture]


def region_locale_values(region):
    values = {key: region["locale"] for key in CONFIG["region"]["locale_categories"]}
    values["LANGUAGE"] = ":".join(tag.replace("-", "_") for tag in region["preferred_languages"])
    return values


def user_directory(path, account):
    missing = []
    current = path
    while not current.exists():
        missing.append(current)
        current = current.parent
    path.mkdir(parents=True, exist_ok=True)
    for directory in missing:
        os.chown(directory, account.pw_uid, account.pw_gid)


def browser_profiles(account, create=False):
    browser = CONFIG["region"]["browser"]
    directory = Path(account.pw_dir) / browser["config_directory"]
    paths = sorted(directory.glob(browser["preferences_glob"]))
    if not paths and create:
        paths = [directory / browser["initial_profile"] / "Preferences"]
    return paths


def close_guest_browser(account):
    browser = CONFIG["region"]["browser"]
    def pids():
        result = subprocess.run(["pgrep", "-u", str(account.pw_uid), "-x", browser["process_name"]], capture_output=True, text=True)
        if result.returncode not in [0, 1]:
            raise ProvisionError("无法读取客机浏览器状态")
        return [int(pid) for pid in result.stdout.split()]
    for pid in pids():
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + browser["shutdown_timeout_seconds"]
    while pids():
        if time.monotonic() >= deadline:
            raise ProvisionError("请先关闭客机 Chromium，再继续应用浏览器语言")
        time.sleep(browser["poll_seconds"])


def apply_region(region, account, backup):
    policy = CONFIG["region"]
    adapter = region_adapter()
    close_guest_browser(account)
    locale_gen = Path(policy["locale_gen_path"])
    locale_conf = Path(policy["locale_conf_path"])
    user_locale = Path(account.pw_dir) / policy["user_locale_path"]
    flags_path = Path(policy["browser"]["flags_path"])
    flags = flags_path.read_text().splitlines() if flags_path.exists() else []
    paths = browser_profiles(account, create=True)
    before = {"timezone": command("timedatectl", "show", "-p", "Timezone", "--value"),
              "files": {}, "browser_language_flags": [line for line in flags if line.startswith("--lang=")],
              "browser_intl": {}}
    for path in [locale_gen, locale_conf, user_locale, *[Path(name) for name in adapter["state_files"]]]:
        before["files"][str(path)] = path.read_text() if path.exists() else None
    for path in paths:
        preferences = json.loads(path.read_text()) if path.exists() else {}
        intl = preferences.get("intl", {})
        before["browser_intl"][str(path)] = {
            key: {"present": key in intl, "value": intl.get(key)}
            for key in ["accept_languages", "selected_languages"]
        }
    write_public(backup / "region-before.json", json.dumps(before, ensure_ascii=False, indent=2), 0o600)

    entry = region["locale"] + " UTF-8"
    lines = locale_gen.read_text().splitlines()
    if entry not in [line.strip() for line in lines]:
        lines.append(entry)
        locale_gen.write_text("\n".join(lines) + "\n")
    command("locale-gen", diagnostic=True)
    values = region_locale_values(region)
    # systemd 262 validates localectl assignments as single locale names;
    # LANGUAGE is a gettext priority list and may contain colons.
    command("localectl", "set-locale", *[key + "=" + value for key, value in values.items() if key != "LANGUAGE"], diagnostic=True)
    # Keep every selected category explicit; localed may simplify categories
    # equal to LANG, while LANGUAGE must be saved as a gettext priority list.
    write_public(locale_conf, "".join(key + "=" + value + "\n" for key, value in values.items()))
    user_directory(user_locale.parent, account)
    user_values = {**values, "TZ": region["timezone"]}
    user_locale.write_text("".join(key + "=" + value + "\n" for key, value in user_values.items()))
    os.chown(user_locale, account.pw_uid, account.pw_gid)
    command(*[part.format(timezone=region["timezone"]) for part in adapter["set_command"]])

    flags = [line for line in flags if not line.startswith("--lang=")]
    flags.append("--lang=" + region["browser_language"])
    write_public(flags_path, "\n".join(flags) + "\n")
    for path in paths:
        preferences = json.loads(path.read_text()) if path.exists() else {}
        intl = preferences.setdefault("intl", {})
        intl["selected_languages"] = ",".join(region["preferred_languages"])
        user_directory(path.parent, account)
        path.write_text(json.dumps(preferences, ensure_ascii=False, separators=(",", ":")))
        os.chown(path, account.pw_uid, account.pw_gid)
    verify_region(region, account)


def verify_region(region, account):
    policy = CONFIG["region"]
    adapter = region_adapter()
    zone = command("timedatectl", "show", "-p", "Timezone", "--value")
    if zone != region["timezone"]:
        raise ProvisionError("客机时区与实测出口不一致")
    if adapter["mode_command"] is not None and command(*adapter["mode_command"]) != policy["timezone_mode"]:
        raise ProvisionError("客机时区尚未固定")
    system_values = region_locale_values(region)
    expected_files = [(Path(policy["locale_conf_path"]), system_values),
                      (Path(account.pw_dir) / policy["user_locale_path"], {**system_values, "TZ": region["timezone"]})]
    for path, expected in expected_files:
        values = {}
        for line in path.read_text().splitlines():
            if "=" in line and not line.startswith("#"):
                key, value = line.split("=", 1)
                values[key] = value.strip('"')
        if any(values.get(key) != value for key, value in expected.items()):
            raise ProvisionError("系统语言或地区设置未正确保存")
    generated = {value.lower().replace("utf-8", "utf8") for value in command("locale", "-a").splitlines()}
    if region["locale"].lower().replace("utf-8", "utf8") not in generated:
        raise ProvisionError("目标系统语言尚未生成")
    flags = Path(policy["browser"]["flags_path"]).read_text().splitlines()
    if [line for line in flags if line.startswith("--lang=")] != ["--lang=" + region["browser_language"]]:
        raise ProvisionError("浏览器界面语言未正确保存")
    paths = browser_profiles(account)
    if not paths:
        raise ProvisionError("浏览器语言配置尚未创建")
    for path in paths:
        intl = json.loads(path.read_text()).get("intl", {})
        # Chromium derives accept_languages from selected_languages and policy.
        # It may omit the derived preference when saving a profile; only the
        # user-selected source is required to persist across browser sessions.
        if intl.get("selected_languages") != ",".join(region["preferred_languages"]):
            raise ProvisionError("浏览器首选语言与实测出口不一致")
