"""Zip the signed Mac app next to the Windows archive."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
config = json.loads((root / "packaging.config.json").read_text())
subprocess.run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent",
                str(root / config["macos_app"]), str(root / config["macos_archive"])], check=True)
print(root / config["macos_archive"])
