import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts/install-waybar.py"
SPEC = importlib.util.spec_from_file_location("waybar_installer", SCRIPT)
INSTALLER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALLER)

STYLE = """@define-color surface #3c3836;
#clock,
#custom-dnd,
#battery {
  background: @surface;
  padding: 0 12px;
  margin: 10px 3px;
  border-radius: 8px;
}
#custom-pomodoro.waiting { color: #fabd2f; }
"""


class WaybarStyles(unittest.TestCase):
    def test_separate_icon_settings_survive_preparing_each_panel(self):
        with tempfile.TemporaryDirectory() as directory:
            config_home = Path(directory)
            waybar = config_home / "waybar"
            waybar.mkdir()
            coloured = "<span foreground='#fe8019' size='large'>󰔟</span>"
            original = [
                {"modules-center": ["clock", "custom/pomodoro", "custom/dnd"],
                 "custom/pomodoro": {"format": "{text}", "format-icons": {"focus-running": coloured}}},
                {"modules-center": ["clock", "custom/dnd"],
                 "custom/pomodoro": {"format": "{icon} <b>{text}</b>", "tooltip": False}},
            ]
            (waybar / "config").write_text(json.dumps(original))
            (waybar / "style.css").write_text(STYLE)
            data, _, count, _ = INSTALLER.prepare(config_home, Path("/test/timer"), Path("/test/waybar"))
            prepared = json.loads(data)
            self.assertEqual(count, 2)
            first, second = [bar["custom/pomodoro"] for bar in prepared]
            self.assertEqual(first["exec"], "/test/timer watch --text-only")
            self.assertEqual(first["format"], "{icon}  {text}")
            self.assertEqual(first["format-icons"]["focus-running"], coloured)
            self.assertIn("long-break-paused", first["format-icons"])
            self.assertEqual(second["format"], "{icon} <b>{text}</b>")
            self.assertFalse(second["tooltip"])
            self.assertEqual(json.loads((waybar / "config").read_text()), original)

    def test_only_the_selector_changes_and_custom_rules_are_preserved(self):
        result = INSTALLER.join_shared_styles(STYLE)
        self.assertEqual(result.replace("#custom-pomodoro,\n", "", 1), STYLE)
        self.assertIn("#custom-dnd,\n#custom-pomodoro,\n#battery", result)
        self.assertEqual(INSTALLER.join_shared_styles(result), result)

    def test_existing_base_styles_are_not_overwritten(self):
        style = "#custom-pomodoro { background: pink; padding: 8px; }"
        self.assertEqual(INSTALLER.join_shared_styles(style), style)

    def test_an_unknown_theme_is_not_replaced_by_invented_defaults(self):
        with self.assertRaisesRegex(ValueError, "shared chip selector"):
            INSTALLER.join_shared_styles("#clock { padding: 2px; }")

    def test_styles_are_present_when_service_restarts_and_failure_rolls_back(self):
        if not (ROOT / "target/release/waybar-pomodoro").is_file():
            self.skipTest("release binary is required for installer integration")
        for fail in [False, True]:
            with self.subTest(failed_restart=fail), tempfile.TemporaryDirectory() as directory:
                home = Path(directory)
                config_home = home / ".config"
                waybar = config_home / "waybar"
                waybar.mkdir(parents=True)
                original_config = [{"modules-center": ["clock", "custom/dnd"]}]
                (waybar / "config").write_text(json.dumps(original_config))
                immutable = home / "managed-style.css"
                immutable.write_text(STYLE)
                immutable.chmod(0o444)
                active = waybar / "style.css"
                active.symlink_to(immutable)
                stubbin = home / "bin"
                stubbin.mkdir()
                for name, contents in {
                    "waybar": "import sys\nraise SystemExit(0)\n",
                    "systemctl": """import os, pathlib, shlex, sys
home = pathlib.Path(os.environ['HOME'])
drop_in = home / '.config/systemd/user/waybar.service.d/50-pomodoro.conf'
if 'restart' in sys.argv and drop_in.exists():
    command = next(line.removeprefix('ExecStart=') for line in drop_in.read_text().splitlines() if line.startswith('ExecStart=') and line != 'ExecStart=')
    args = shlex.split(command)
    style = pathlib.Path(args[args.index('--style') + 1])
    assert style == home / '.config/waybar/style.css'
    assert '#custom-dnd,\\n#custom-pomodoro,\\n#battery' in style.read_text()
    (home / 'styled-restart').write_text('yes')
    if os.environ.get('POMODORO_TEST_FAIL'):
        raise SystemExit(1)
""",
                }.items():
                    stub = stubbin / name
                    stub.write_text(f"#!{sys.executable}\n" + contents)
                    stub.chmod(0o755)
                env = dict(os.environ, HOME=str(home), XDG_CONFIG_HOME=str(config_home),
                           PATH=str(stubbin) + os.pathsep + os.environ["PATH"])
                if fail:
                    env["POMODORO_TEST_FAIL"] = "1"
                result = subprocess.run([sys.executable, str(SCRIPT)], env=env,
                                        capture_output=True, text=True)
                self.assertTrue((home / "styled-restart").is_file(), result.stderr)
                self.assertEqual(immutable.read_text(), STYLE)
                self.assertEqual(json.loads((waybar / "config").read_text()), original_config)
                backup = waybar / "style.css.pomodoro-backup"
                if fail:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertTrue(active.is_symlink())
                    self.assertEqual(active.read_text(), STYLE)
                    self.assertFalse(backup.exists())
                    self.assertFalse((config_home / "systemd/user/waybar.service.d/50-pomodoro.conf").exists())
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertFalse(active.is_symlink())
                    self.assertEqual(backup.read_text(), STYLE)
                    metadata = json.loads((waybar / "style.css.pomodoro-backup.json").read_text())
                    self.assertEqual(metadata["symlink"], str(immutable))
                    self.assertEqual(INSTALLER.join_shared_styles(active.read_text()), active.read_text())


if __name__ == "__main__":
    unittest.main()
