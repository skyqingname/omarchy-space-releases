"""Build-time downloads; the delivered desktop installer consumes local files."""
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

root = Path(__file__).resolve().parents[1]
settings = json.loads((root / "asset-sources.json").read_text())
source = settings["windows-x86_64"]
destination = root / source["destination"]


def fetch(item):
    name, url = item
    path = destination / name
    if path.exists():
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + ".partial")
    subprocess.run(["curl", "--fail", "--location", "--silent", "--show-error", "--max-time",
                    str(settings["download_timeout_seconds"]), "--output", str(partial), url], check=True)
    partial.replace(path)
    print("Prepared " + name, flush=True)


with ThreadPoolExecutor(max_workers=settings["parallel_downloads"]) as pool:
    list(pool.map(fetch, source["files"].items()))
with zipfile.ZipFile(destination / "downloads/winq-runtime.zip") as archive:
    archive.extractall(destination / "runtime")
with zipfile.ZipFile(destination / "downloads/zstd.zip") as archive:
    name = next(name for name in archive.namelist() if name.endswith("/zstd.exe") or name == "zstd.exe")
    with archive.open(name) as source, (destination / "zstd.exe").open("wb") as target:
        shutil.copyfileobj(source, target)
profile = json.loads((root / "runtime-profiles.json").read_text())["windows-x86_64"]
(destination / "manifest.json").write_text(json.dumps(profile, indent=2) + "\n")
print("Windows offline resource pack prepared", flush=True)
