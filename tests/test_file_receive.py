"""File transfer protocol tests; filesystem integration runs on the Linux guest."""
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import tempfile
import unittest
import uuid

spec = importlib.util.spec_from_file_location("file_receive", Path(__file__).resolve().parents[1] / "guest/file_receive.py")
receiver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(receiver)


class Names(unittest.TestCase):
    def test_unicode_quotes_and_spaces(self):
        self.assertEqual(receiver.component("报告 final's.docx"), "报告 final's.docx")

    def test_traversal_controls_and_long_names(self):
        for value in ("", ".", "..", "../a", "/etc/passwd", "C:\\x", "a\nb", "a\x00b", "长" * 100):
            with self.subTest(value=value), self.assertRaises(ValueError):
                receiver.component(value)

    def test_invalid_metadata_is_rejected_before_filesystem_access(self):
        cfg = {"id": str(uuid.uuid4()), "directory": "workspace/attachments",
               "name": "a.txt", "size": 3, "max_bytes": 1024}
        for patch in ({"name": ".partial"}, {"size": -1}, {"size": True},
                      {"size": 1025}, {"directory": "../x"}, {"id": "not-a-uuid"}):
            with self.subTest(patch=patch), self.assertRaises(ValueError):
                receiver.receive({**cfg, **patch}, io.BytesIO(b"abc"))


@unittest.skipUnless(os.name == "posix", "Linux/POSIX directory descriptor integration")
class Receive(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)

    def config(self, payload, name="报告 final's.docx"):
        return {"id": str(uuid.uuid4()), "directory": "workspace/attachments", "name": name,
                "size": len(payload), "max_bytes": 1024 * 1024}

    def receive(self, cfg, payload):
        return receiver.receive(cfg, io.BytesIO(payload), home=self.home)

    def test_binary_and_empty_files_are_preserved(self):
        for payload in (b"", bytes(range(256)) * 300):
            cfg = self.config(payload)
            receipt = self.receive(cfg, payload)
            self.assertEqual(Path(receipt["path"]).read_bytes(), payload)
            self.assertEqual(receipt["sha256"], hashlib.sha256(payload).hexdigest())
            self.assertEqual(receipt["size"], len(payload))
            self.assertEqual(Path(receipt["path"]).stat().st_mode & 0o777, 0o600)

    def test_repeated_names_never_overwrite(self):
        a = self.receive(self.config(b"first"), b"first")
        b = self.receive(self.config(b"second"), b"second")
        self.assertNotEqual(a["path"], b["path"])
        self.assertEqual(Path(a["path"]).read_bytes(), b"first")

    def test_truncated_and_extra_data_leave_no_file(self):
        for payload in (b"x", b"too long"):
            cfg = self.config(b"abc")
            with self.assertRaises(ValueError):
                self.receive(cfg, payload)
            self.assertFalse(self.home.joinpath(cfg["directory"], cfg["id"]).exists())

    def test_rejects_size_and_directory_before_writing(self):
        for patch in ({"size": -1}, {"size": True}, {"size": 2000000},
                      {"directory": "../outside"}, {"directory": "/tmp"}, {"id": "../x"}):
            cfg = {**self.config(b"abc"), **patch}
            with self.subTest(patch=patch), self.assertRaises(ValueError):
                self.receive(cfg, b"abc")
        self.assertEqual(list(self.home.iterdir()), [])

    def test_symlink_parent_does_not_redirect_upload(self):
        outside = self.home / "outside"
        outside.mkdir()
        (self.home / "workspace").symlink_to(outside, target_is_directory=True)
        with self.assertRaises(OSError):
            self.receive(self.config(b"abc"), b"abc")
        self.assertEqual(list(outside.iterdir()), [])

    def test_existing_transfer_id_is_not_overwritten(self):
        cfg = self.config(b"abc")
        result = self.receive(cfg, b"abc")
        with self.assertRaises(FileExistsError):
            self.receive(cfg, b"xyz")
        self.assertEqual(Path(result["path"]).read_bytes(), b"abc")


if __name__ == "__main__":
    unittest.main()
