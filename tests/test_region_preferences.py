"""Regression for Chromium omitting derived Accept-Language preferences on exit."""
import copy
import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("guest_region", ROOT / "guest/region.py")
region_module = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(region_module)


class RegionPreferencesTest(unittest.TestCase):
    def setUp(self):
        self.directory = TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        home = Path(self.directory.name)
        config = json.loads((ROOT / "product.config.json").read_text())
        region_module.CONFIG = copy.deepcopy(config)
        policy = region_module.CONFIG["region"]
        policy["locale_conf_path"] = str(home / "locale.conf")
        policy["browser"]["flags_path"] = str(home / "chromium-flags.conf")
        self.region = {
            **policy["country_profiles"]["US"],
            "timezone": "America/Los_Angeles",
        }
        self.account = SimpleNamespace(pw_dir=str(home))
        region_module.ProvisionError = RuntimeError
        region_module.region_adapter = lambda: {"mode_command": None}
        def command(*args):
            if args == ("timedatectl", "show", "-p", "Timezone", "--value"):
                return self.region["timezone"]
            if args == ("locale", "-a"):
                return "en_US.utf8"
            raise AssertionError("Unexpected command: " + repr(args))
        region_module.command = command
        values = region_module.region_locale_values(self.region)
        Path(policy["locale_conf_path"]).write_text(
            "".join(k + "=" + v + "\n" for k, v in values.items()))
        user_locale = home / policy["user_locale_path"]
        user_locale.parent.mkdir(parents=True)
        user_locale.write_text("".join(k + "=" + v + "\n" for k, v in
                                      {**values, "TZ": self.region["timezone"]}.items()))
        Path(policy["browser"]["flags_path"]).write_text("--lang=en-US\n")
        self.profile = home / policy["browser"]["config_directory"] / "Default/Preferences"
        self.profile.parent.mkdir(parents=True)

    def verify(self, intl):
        self.profile.write_text(json.dumps({"intl": intl}))
        region_module.verify_region(self.region, self.account)

    def test_chromium_saved_profile_without_derived_preference(self):
        self.verify({"selected_languages": "en-US,en"})

    def test_missing_selected_languages_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "浏览器首选语言"):
            self.verify({"accept_languages": "en-US,en"})

    def test_wrong_selected_languages_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "浏览器首选语言"):
            self.verify({"selected_languages": "ja-JP,ja", "accept_languages": "en-US,en"})

    def test_other_user_profile_must_also_match(self):
        other = self.profile.parent.parent / "Profile 1/Preferences"
        other.parent.mkdir()
        other.write_text(json.dumps({"intl": {"selected_languages": "ja-JP,ja"}}))
        with self.assertRaisesRegex(RuntimeError, "浏览器首选语言"):
            self.verify({"selected_languages": "en-US,en"})


if __name__ == "__main__":
    unittest.main()
