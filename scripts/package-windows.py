"""Package the native executable and its runtime in one portable archive."""
import json
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[1]
config = json.loads((root / "packaging.config.json").read_text())
tauri = root / "src-tauri"
resources = json.loads((tauri / "tauri.windows.conf.json").read_text())["bundle"]["resources"]
archive = root / config["windows_archive"]
archive.parent.mkdir(parents=True, exist_ok=True)
partial = archive.with_name(archive.name + ".partial")
with zipfile.ZipFile(partial, "w", compression=zipfile.ZIP_DEFLATED, allowZip64=True) as target:
    target.write(root / config["windows_executable"], config["windows_program_name"])
    target.write(root / "docs/windows-preview.md", "使用说明.md")
    target.write(root / "assets/windows-x86_64/LICENSE.omarchy", "LICENSE.omarchy")
    for source, destination in resources.items():
        path = tauri / source
        if path.is_dir():
            for file in sorted(path.rglob("*")):
                if file.is_file():
                    target.write(file, str(Path(destination) / file.relative_to(path)))
        else:
            target.write(path, destination)
partial.replace(archive)
print(archive)
