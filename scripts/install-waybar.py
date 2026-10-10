#!/usr/bin/env python3
"""install the pomodoro module into an existing home-manager waybar session."""

import argparse
import difflib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent


def atomic_write(path, data, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".pomodoro-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def unit_quote(value):
    value = str(value)
    if any(character in value for character in "\n\r\x00"):
        raise ValueError("paths must not contain control characters")
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace("%", "%%") + '"'


def join_shared_styles(style):
    """Join the user's existing chip rules without choosing any CSS values."""
    if re.search(r"#custom-pomodoro\s*(?:,|\{)", style):
        return style
    anchor = re.search(r"#custom-dnd\s*,", style)
    if not anchor:
        raise ValueError("cannot find the shared chip selector group; add #custom-pomodoro to your external waybar stylesheet")
    return style[:anchor.end()] + "\n#custom-pomodoro," + style[anchor.end():]


def prepare(config_home, binary, waybar):
    config = json.loads((config_home / "waybar/config").read_text())
    bars = config if isinstance(config, list) else [config]
    if not bars or not all(isinstance(bar, dict) for bar in bars):
        raise ValueError("expected a waybar object or a list of bars")
    module = json.loads((ROOT / "examples/waybar.jsonc").read_text())["custom/pomodoro"]
    for key, action in [("exec", "watch --text-only"), ("on-click", "start"),
                        ("on-click-right", "pause"), ("on-click-middle", "reset")]:
        module[key] = f"{shlex.quote(str(binary))} {action}"
    for bar in bars:
        center = [name for name in bar.get("modules-center", []) if name != "custom/pomodoro"]
        if "clock" not in center or "custom/dnd" not in center:
            raise ValueError("each bar needs clock and custom/dnd in modules-center")
        center.insert(center.index("clock") + 1, "custom/pomodoro")
        bar["modules-center"] = center
        existing = bar.get("custom/pomodoro", {})
        settings = {**module, **existing}
        if existing.get("format") == "{text}":
            settings["format"] = module["format"]
        if isinstance(existing.get("format-icons", {}), dict):
            settings["format-icons"] = {**module["format-icons"], **existing.get("format-icons", {})}
        for key in ["exec", "on-click", "on-click-right", "on-click-middle"]:
            settings[key] = module[key]
        bar["custom/pomodoro"] = settings
    timer_home = config_home / "waybar-pomodoro"
    arguments = [waybar, "--config", timer_home / "bar-config.json",
                 "--style", config_home / "waybar/style.css"]
    override = "[Service]\nExecStart=\nExecStart=" + " ".join(map(unit_quote, arguments)) + "\n"
    style = join_shared_styles((config_home / "waybar/style.css").read_text())
    return json.dumps(config, ensure_ascii=False, indent=2).encode() + b"\n", override.encode(), len(bars), style.encode()


def systemctl(*arguments):
    subprocess.run(["systemctl", "--user", *arguments], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--prepare-only", type=Path, metavar="DIRECTORY",
                        help="write preview files only; do not install or restart anything")
    args = parser.parse_args()
    home = Path.home()
    config_home = Path(os.environ.get("XDG_CONFIG_HOME") or home / ".config")
    binary = home / ".local/bin/waybar-pomodoro"
    waybar = shutil.which("waybar")
    if not waybar:
        raise RuntimeError("waybar is not on your path")
    config, override, count, style = prepare(config_home, binary, waybar)
    if args.prepare_only:
        style_patch = "".join(difflib.unified_diff(
            (config_home / "waybar/style.css").read_text().splitlines(keepends=True),
            style.decode().splitlines(keepends=True),
            fromfile="style.css", tofile="style.css",
        )).encode()
        for name, data in [("config.json", config), ("50-pomodoro.conf", override),
                           ("style.css.patch", style_patch)]:
            atomic_write(args.prepare_only / name, data)
        print(f"prepared {count} panels in {args.prepare_only}; nothing installed or restarted")
        return

    release = ROOT / "target/release/waybar-pomodoro"
    if not release.is_file():
        raise RuntimeError("build the binary first: nix develop -c cargo build --locked --release")
    # Verify access before writing any installation files.
    systemctl("show", "waybar.service", "--property=ActiveState")
    timer_home = config_home / "waybar-pomodoro"
    drop_in = config_home / "systemd/user/waybar.service.d/50-pomodoro.conf"
    files = [(binary, release.read_bytes(), 0o755),
             (timer_home / "bar-config.json", config, 0o644),
             (drop_in, override, 0o644)]
    style_path = config_home / "waybar/style.css"
    original_style = style_path.read_bytes()
    if style != original_style:
        backup_path = style_path.with_name("style.css.pomodoro-backup")
        backup_metadata = style_path.with_name("style.css.pomodoro-backup.json")
        if not backup_path.exists() and not backup_metadata.exists():
            files.extend([
                (backup_path, original_style, 0o644),
                (backup_metadata, json.dumps({
                    "symlink": os.readlink(style_path) if style_path.is_symlink() else None,
                    "mode": style_path.stat().st_mode & 0o777,
                }).encode() + b"\n", 0o644),
            ])
        # A home-manager symlink targets an immutable store file. Replace the
        # link at the external config path, never modify the Nix store target.
        files.append((style_path, style, (style_path.stat().st_mode & 0o777) | 0o200))
    if not (timer_home / "config.toml").exists():
        files.append((timer_home / "config.toml", (ROOT / "examples/config.toml").read_bytes(), 0o644))
    previous = []
    try:
        for path, data, mode in files:
            if path.is_symlink():
                saved = ("symlink", os.readlink(path), None)
            elif path.exists():
                saved = ("file", path.read_bytes(), path.stat().st_mode & 0o777)
            else:
                saved = None
            previous.append((path, saved))
            atomic_write(path, data, mode)
        systemctl("daemon-reload")
        systemctl("restart", "waybar.service")
        time.sleep(1)
        systemctl("is-active", "--quiet", "waybar.service")
    except Exception:
        for path, saved in reversed(previous):
            if saved is None:
                path.unlink(missing_ok=True)
            elif saved[0] == "symlink":
                path.unlink(missing_ok=True)
                path.symlink_to(saved[1])
            else:
                atomic_write(path, saved[1], saved[2])
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=False)
        subprocess.run(["systemctl", "--user", "restart", "waybar.service"], check=False)
        raise
    print(f"pomodoro installed on {count} panels; waybar restarted")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"could not install pomodoro: {error}") from error
