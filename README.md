# pomodoro-timer-waybar

a pomodoro timer for waybar.

## install

requires linux, waybar, a nerd font and rust 1.89 or newer.
notifications use `notify-send` when available.

```sh
cargo install --path . --locked
```

add `~/.cargo/bin` to waybar's path, or use the full binary path.
merge [the module](examples/waybar.jsonc) into your waybar config and put
`custom/pomodoro` between `clock` and `custom/dnd` in `modules-center`.
the module streams updates; don't add `interval` or `signal`.

for nix, run `nix build` or use the [home manager example](examples/home-manager.nix).
for an existing home manager waybar session, build with
`cargo build --locked --release`, then run `python3 scripts/install-waybar.py`.

## config

copy [the example](examples/config.toml) to
`~/.config/waybar-pomodoro/config.toml`, or keep the defaults:

```toml
focus_minutes = 25
short_break_minutes = 5
long_break_minutes = 30
focuses_before_long_break = 4
notifications = true
```

durations are positive whole minutes. the path follows `$XDG_CONFIG_HOME`.
restart waybar after changing the config. for a custom path, add
`--config /path/to/config.toml` to all four module commands.

## usage

left-click starts or resumes. right-click pauses.
middle-click twice within three seconds resets the cycle.
each phase waits for a click to start, including breaks.
sleep pauses the timer; left-click to resume after waking.
panels share one timer. restarting all panels resets it.
notifications follow your notification daemon's dnd settings.

## style

edit your waybar stylesheet, usually `~/.config/waybar/style.css`.
add `#custom-pomodoro` to the shared module selectors for matching spacing
and background. the installer adds this selector without changing css values.
set colours using `focus`, `short-break`, `long-break`, `waiting`, `running`,
`paused`, `reset-armed` and `error` classes on `#custom-pomodoro`.

the example uses `watch --text-only` and `"format": "{icon}  {text}".
set icon colour, size and weight in waybar's `format-icons`, for example:

```json
"focus-running": "<span foreground='#fe8019' size='large' weight='bold'>󰔟</span>"
```

icon keys combine the phase and status: `focus-running`, `short-break-waiting`,
`long-break-paused`, and the other combinations. `reset-armed` and `error` take
priority. text colour comes from css; `.focus.paused` targets a specific pair.
plain `watch` still outputs the original icon and time together.

[mit license](LICENSE).
