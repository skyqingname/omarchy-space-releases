"""Exercise the actual patched functions without starting the bridge or network."""
import ast
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parent
tree = ast.parse((ROOT.parent / "guest/transcript_follow.py").read_text(encoding="utf-8"))
functions = ast.Module(body=[node for node in tree.body if isinstance(node, ast.FunctionDef)
                             and node.name in ("read_json", "api_configured", "status")], type_ignores=[])
program = compile(functions, "patched_helper.py", "exec")


class AuthDetectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.home = Path(self.temp.name)
        self.cwd = str(self.home / "workspace")
        (self.home / ".claude").mkdir()
        self.env_patch = patch.dict(os.environ, {}, clear=True)
        self.env_patch.start()
        self.addCleanup(self.env_patch.stop)
        self.cfg = {"claude_credentials_path": ".claude/.credentials.json",
                    "claude_user_settings_path": ".claude/settings.json",
                    "claude_state_path": ".claude.json", "claude_settings": {}}
        self.ns = dict(os=os, json=json, home=str(self.home), cwd=self.cwd, cfg=self.cfg)
        exec(program, self.ns)

    def write_settings(self, data):
        (self.home / ".claude/settings.json").write_text(json.dumps(data), encoding="utf-8")

    def status(self):
        return self.ns["status"]()

    def test_no_credentials_stays_signed_out(self):
        self.assertEqual(self.status(), {"signed_in": False, "onboarded": False, "trusted": False})

    def test_existing_oauth_behavior_is_preserved(self):
        (self.home / ".claude/.credentials.json").write_text('{}')
        self.assertIs(self.status()["signed_in"], True)

    def test_user_settings_token(self):
        self.write_settings({"env": {"ANTHROPIC_AUTH_TOKEN": "fixture-token"}})
        self.assertIs(self.status()["signed_in"], True)
        self.assertFalse((self.home / ".claude/.credentials.json").exists())

    def test_user_settings_api_key(self):
        self.write_settings({"env": {"ANTHROPIC_API_KEY": "fixture-key"}})
        self.assertIs(self.status()["signed_in"], True)

    def test_inherited_environment(self):
        for key in ("ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"):
            with self.subTest(key=key), patch.dict(os.environ, {key: "fixture"}, clear=True):
                self.assertIs(self.status()["signed_in"], True)

    def test_launch_settings(self):
        self.cfg["claude_settings"] = {"env": {"ANTHROPIC_AUTH_TOKEN": "fixture"}}
        self.assertIs(self.status()["signed_in"], True)

    def test_blank_or_invalid_values_do_not_sign_in(self):
        for value in ("", " \t\n", None, False, 1, {}, []):
            with self.subTest(value=value):
                self.write_settings({"env": {"ANTHROPIC_AUTH_TOKEN": value}})
                self.assertIs(self.status()["signed_in"], False)

    def test_base_url_alone_is_not_authentication(self):
        self.write_settings({"env": {"ANTHROPIC_BASE_URL": "https://example.invalid"}})
        self.assertIs(self.status()["signed_in"], False)

    def test_malformed_settings(self):
        for raw in ('{', 'null', '[]', '{"env":null}', '{"env":[]}'):
            with self.subTest(raw=raw):
                (self.home / ".claude/settings.json").write_text(raw)
                self.assertIs(self.status()["signed_in"], False)

    def test_onboarding_and_trust_remain_independent(self):
        self.write_settings({"env": {"ANTHROPIC_AUTH_TOKEN": "fixture"}})
        self.assertEqual(self.status(), {"signed_in": True, "onboarded": False, "trusted": False})
        (self.home / ".claude.json").write_text(json.dumps({
            "hasCompletedOnboarding": True, "projects": {self.cwd: {"hasTrustDialogAccepted": True}}}))
        self.assertEqual(self.status(), {"signed_in": True, "onboarded": True, "trusted": True})


if __name__ == "__main__":
    unittest.main(verbosity=2)
