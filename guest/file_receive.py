"""Receive one explicitly selected host file on SSH stdin, without executing it."""
import hashlib
import json
import os
import signal
from pathlib import Path
import sys
import uuid


def component(value):
    if (not isinstance(value, str) or value in ("", ".", "..")
            or len(value.encode("utf-8")) > 240
            or any(c in "/\\" or ord(c) < 32 or ord(c) == 127 for c in value)):
        raise ValueError("不支持的文件名或目录名")
    return value


def receive(cfg, stream, home=None):
    name = component(cfg["name"])
    if name == ".partial":
        raise ValueError("不支持的文件名")
    transfer = str(uuid.UUID(cfg["id"]))
    size = cfg["size"]
    if type(size) is not int or not 0 <= size <= cfg["max_bytes"]:
        raise ValueError("文件超过上传大小限制")
    parts = cfg["directory"].split("/")
    for part in parts:
        component(part)
    home = Path(home or Path.home()).resolve()
    # Walk with directory descriptors: neither an existing attachment directory
    # nor a concurrent symlink swap may redirect this write outside the workspace.
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    parent = os.open(str(home), flags)
    folder = None
    staged = False
    try:
        for part in parts:
            try:
                os.mkdir(part, 0o700, dir_fd=parent)
            except FileExistsError:
                pass
            child = os.open(part, flags, dir_fd=parent)
            os.close(parent)
            parent = child
        # A fresh UUID directory avoids overwriting any previous upload.
        os.mkdir(transfer, 0o700, dir_fd=parent)
        folder = os.open(transfer, flags, dir_fd=parent)
        fd = os.open(".partial", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                     0o600, dir_fd=folder)
        staged = True
        digest = hashlib.sha256()
        remaining = size
        with os.fdopen(fd, "wb") as output:
            while remaining:
                chunk = stream.read(min(65536, remaining))
                if not chunk:
                    raise ValueError("文件传输中断，请重新上传")
                output.write(chunk)
                digest.update(chunk)
                remaining -= len(chunk)
            if stream.read(1):
                raise ValueError("文件在上传时发生变化，请重新上传")
            output.flush()
            os.fsync(output.fileno())
        # link() fails if the destination exists; unlike replace(), it cannot
        # overwrite a file inserted by another guest process during the upload.
        os.link(".partial", name, src_dir_fd=folder, dst_dir_fd=folder,
                follow_symlinks=False)
        os.unlink(".partial", dir_fd=folder)
        staged = False
        return {"id": transfer, "name": name, "size": size,
                "path": str(home.joinpath(*parts, transfer, name)),
                "sha256": digest.hexdigest()}
    finally:
        if folder is not None:
            if staged:
                os.unlink(".partial", dir_fd=folder)
            os.close(folder)
            if staged:
                os.rmdir(transfer, dir_fd=parent)
        os.close(parent)


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise InterruptedError("文件传输已取消")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGHUP, interrupted)
    try:
        print(json.dumps(receive(json.loads(sys.argv[1]), sys.stdin.buffer)), flush=True)
    except (OSError, ValueError, KeyError) as error:
        # No file contents or credentials in error output.
        print(json.dumps({"error": str(error)}), flush=True)
        sys.exit(1)
