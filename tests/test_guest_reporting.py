"""Exercise the serial short-write behavior observed in the real VM fixture."""
import ast
import json
from pathlib import Path
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "guest/provision.py"
tree = ast.parse(SOURCE.read_text(), filename=str(SOURCE))
definitions = [node for node in tree.body
               if isinstance(node, (ast.FunctionDef, ast.ClassDef))
               and node.name in ("ProvisionError", "report")]


class ShortWriter:
    def __init__(self, limit):
        self.limit = limit
        self.data = bytearray()

    def write(self, data):
        count = min(self.limit, len(data))
        self.data.extend(data[:count])
        return count


class GuestReportingTest(unittest.TestCase):
    def namespace(self, writer):
        context = {"json": json, "channel": writer}
        exec(compile(ast.Module(body=definitions, type_ignores=[]), str(SOURCE), "exec"), context)
        return context

    def test_short_writes_preserve_one_complete_utf8_json_record(self):
        writer = ShortWriter(7)
        context = self.namespace(writer)
        context["report"]("network_guard_ready", "断网保护已启用", network_guard_version=1)
        self.assertEqual(json.loads(writer.data), {
            "phase": "network_guard_ready", "message": "断网保护已启用", "network_guard_version": 1,
        })
        self.assertTrue(writer.data.endswith(b"\n"))
        self.assertEqual(writer.data.count(b"\n"), 1)

    def test_closed_writer_fails_instead_of_looping_forever(self):
        context = self.namespace(ShortWriter(0))
        with self.assertRaises(context["ProvisionError"]):
            context["report"]("ready", "ready")


if __name__ == "__main__":
    unittest.main()
