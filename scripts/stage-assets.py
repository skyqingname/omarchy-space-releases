"""Assemble the bundled Mac runtime; the Omarchy image is downloaded by the app."""
import json
from pathlib import Path
import shutil
import sys

root = Path(__file__).resolve().parents[1]
config = json.loads(Path(sys.argv[1]).read_text())
profile = json.loads((root / "runtime-profiles.json").read_text())[config["platform"]]
destination = root / config["destination"]
destination.mkdir(parents=True, exist_ok=True)
shutil.copytree(config["runtime_source"], destination / "runtime", dirs_exist_ok=True)
shutil.copytree(config["firmware_source"], destination / "firmware", dirs_exist_ok=True)
shutil.copy2(config["sing_box_source"], destination / profile["sing_box_package"])
for notice in config["notices"]:
    shutil.copy2(notice["source"], destination / notice["destination"])
(destination / "manifest.json").write_text(json.dumps(profile, ensure_ascii=False, indent=2) + "\n")
print("Runtime pack prepared:", destination)
