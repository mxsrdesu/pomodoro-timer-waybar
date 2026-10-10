use std::{
    env,
    io::{self, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

use waybar_pomodoro::{
    Result,
    engine::Action,
    notify::Notifier,
    render::{Output, RenderMode},
    store::{Paths, Store},
};

const HELP: &str = "waybar-pomodoro — pomodoro timer for waybar

usage: waybar-pomodoro [--config path] <command>

watch    stream json for waybar (one shared timer across panels)
start    start or resume; does nothing while already running
pause    pause the running phase
reset    arm reset; repeat within 3 seconds to reset the whole cycle

watch --text-only    output time only; select icons with waybar's format-icons

config: $XDG_CONFIG_HOME/waybar-pomodoro/config.toml
        (defaults to ~/.config/waybar-pomodoro/config.toml)
";

fn main() {
    if let Err(err) = run() {
        eprintln!("waybar-pomodoro: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut config = None;
    let mut command = None;
    let mut text_only = false;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--help" | "-h") => {
                print!("{HELP}");
                return Ok(());
            }
            Some("--version" | "-V") => {
                println!("waybar-pomodoro {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--config") => {
                config = Some(PathBuf::from(args.next().ok_or("--config needs a path")?));
            }
            Some("--text-only") if !text_only => text_only = true,
            Some("watch" | "start" | "pause" | "reset") if command.is_none() => {
                command = arg.to_str().map(str::to_owned);
            }
            _ => {
                return Err(format!(
                    "unknown or extra argument: {}\n{HELP}",
                    arg.to_string_lossy()
                )
                .into());
            }
        }
    }
    let command = command.ok_or(HELP)?;
    if text_only && command != "watch" {
        return Err("--text-only is only available with watch".into());
    }
    let paths = Paths::discover(config)?;
    if command == "watch" {
        watch(
            &paths,
            if text_only {
                RenderMode::TextOnly
            } else {
                RenderMode::Combined
            },
        )
    } else {
        let action = match command.as_str() {
            "start" => Action::Start,
            "pause" => Action::Pause,
            "reset" => Action::Reset,
            _ => unreachable!(),
        };
        let snapshot = Store::control(&paths, action)?;
        Notifier::default().send(&snapshot);
        println!(
            "{}",
            serde_json::to_string(&Output::from_state(&snapshot.state, snapshot.now))?
        );
        Ok(())
    }
}

fn watch(paths: &Paths, mode: RenderMode) -> Result<()> {
    let mut stdout = io::stdout().lock();
    let mut last_output = String::new();
    let mut last_error = String::new();
    let mut notifier = Notifier::default();
    let store = loop {
        match Store::watch(paths) {
            Ok(store) => break store,
            Err(err) => {
                report_error(
                    &mut stdout,
                    &mut last_output,
                    &mut last_error,
                    err.as_ref(),
                    mode,
                )?;
                thread::sleep(Duration::from_secs(2));
            }
        }
    };
    loop {
        match store.tick() {
            Ok(snapshot) => {
                last_error.clear();
                notifier.send(&snapshot);
                emit(
                    &mut stdout,
                    &mut last_output,
                    &Output::from_state_with_mode(&snapshot.state, snapshot.now, mode),
                )?;
            }
            Err(err) => report_error(
                &mut stdout,
                &mut last_output,
                &mut last_error,
                err.as_ref(),
                mode,
            )?,
        }
        notifier.reap();
        thread::sleep(Duration::from_millis(200));
    }
}

fn report_error(
    stdout: &mut impl Write,
    last_output: &mut String,
    last_error: &mut String,
    err: &dyn std::error::Error,
    mode: RenderMode,
) -> Result<()> {
    let message = err.to_string();
    if message != *last_error {
        eprintln!("waybar-pomodoro: {message}");
        last_error.clone_from(&message);
    }
    emit(
        stdout,
        last_output,
        &Output::error_with_mode(&message, mode),
    )
}

fn emit(stdout: &mut impl Write, last: &mut String, output: &Output) -> Result<()> {
    let json = serde_json::to_string(output)?;
    if json != *last {
        writeln!(stdout, "{json}")?;
        stdout.flush()?;
        *last = json;
    }
    Ok(())
}
