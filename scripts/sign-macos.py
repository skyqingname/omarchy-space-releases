"""Seal the local development app without re-signing the QEMU runtime payload."""
import json
from pathlib import Path
import subprocess

root = Path(__file__).resolve().parents[1]
config = json.loads((root / "packaging.config.json").read_text())
app = root / config["macos_app"]
subprocess.run(["codesign", "--force", "--sign", config["macos_signature_identity"], str(app)], check=True)
subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
print("Local app bundle signature verified:", app)
