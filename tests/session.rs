use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Session {
    directory: PathBuf,
}

impl Session {
    fn new(config: Option<&str>) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "waybar-pomodoro-test-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("bin")).unwrap();
        if let Some(config) = config {
            fs::write(directory.join("config.toml"), config).unwrap();
        }
        Self { directory }
    }

    fn command(&self, action: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_waybar-pomodoro"));
        command
            .arg("--config")
            .arg(self.directory.join("config.toml"))
            .arg(action)
            .env("XDG_RUNTIME_DIR", &self.directory)
            .env("PATH", self.directory.join("bin"))
            .env(
                "POMODORO_TEST_NOTIFICATIONS",
                self.directory.join("notifications"),
            );
        command
    }

    fn control(&self, action: &str) -> Value {
        let output = self.command(action).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn watch(&self) -> Watch {
        self.watch_with_mode(false)
    }

    fn watch_with_mode(&self, text_only: bool) -> Watch {
        let mut command = self.command("watch");
        if text_only {
            command.arg("--text-only");
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                let value = serde_json::from_str(&line)
                    .expect("watch must emit one complete json object per line");
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        Watch { child, output }
    }

    fn stub_notifications(&self) {
        let path = self.directory.join("bin/notify-send");
        fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$POMODORO_TEST_NOTIFICATIONS\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn expire_phase(&self) {
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.directory.join("waybar-pomodoro/state.lock"))
            .unwrap();
        lock.lock().unwrap();
        let path = self.directory.join("waybar-pomodoro/state.json");
        let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        state["deadline_ms"] = 0.into();
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .unwrap();
        file.write_all(&serde_json::to_vec(&state).unwrap())
            .unwrap();
        lock.unlock().unwrap();
    }

    fn notification_count(&self) -> usize {
        fs::read_to_string(self.directory.join("notifications"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

struct Watch {
    child: Child,
    output: mpsc::Receiver<Value>,
}

impl Watch {
    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let value = self
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("watch did not emit the expected state");
            if predicate(&value) {
                return value;
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn has_class(value: &Value, class: &str) -> bool {
    value["class"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == class)
}

#[test]
fn text_only_panel_shares_a_timer_with_legacy_panel_and_selects_icons_by_state() {
    let session = Session::new(Some("notifications = false\nfocuses_before_long_break = 2"));
    let legacy = session.watch();
    let initial_legacy = legacy.until(|value| has_class(value, "waiting"));
    let separate = session.watch_with_mode(true);
    for (index, phase) in ["focus", "short-break", "focus", "long-break"]
        .into_iter()
        .enumerate()
    {
        let waiting = separate.until(|value| value["alt"] == format!("{phase}-waiting"));
        let old = if index == 0 {
            initial_legacy.clone()
        } else {
            legacy.until(|value| value["alt"] == phase && has_class(value, "waiting"))
        };
        assert_eq!(waiting["class"], old["class"]);
        assert_eq!(waiting["tooltip"], old["tooltip"]);
        assert_eq!(
            old["text"].as_str().unwrap().split_once("  ").unwrap().1,
            waiting["text"]
        );
        session.control("start");
        separate.until(|value| value["alt"] == format!("{phase}-running"));
        session.control("pause");
        separate.until(|value| value["alt"] == format!("{phase}-paused"));
        session.control("reset");
        let armed = separate.until(|value| value["alt"] == "reset-armed");
        assert!(has_class(&armed, "reset-armed"));
        session.control("start");
        separate.until(|value| value["alt"] == format!("{phase}-running"));
        session.expire_phase();
    }
    let next = separate.until(|value| value["alt"] == "focus-waiting");
    assert_eq!(next["text"], "25:00");
    assert!(next["tooltip"].as_str().unwrap().contains("round 1/2"));
}

#[test]
fn text_only_errors_recover_and_controls_reject_the_watch_option() {
    let session = Session::new(Some("focus_minutes = 0"));
    let watch = session.watch_with_mode(true);
    let error = watch.until(|value| has_class(value, "error"));
    assert_eq!(error["alt"], "error");
    assert_eq!(error["text"], "--:--");
    fs::write(session.directory.join("config.toml"), "focus_minutes = 10").unwrap();
    let fixed = watch.until(|value| value["alt"] == "focus-waiting");
    assert_eq!(fixed["text"], "10:00");
    let rejected = session
        .command("start")
        .arg("--text-only")
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("only available with watch"));
    let duplicate = session
        .command("watch")
        .args(["--text-only", "--text-only"])
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
}

#[test]
fn phase_messages_and_tooltips_use_plain_text() {
    let session = Session::new(Some("focuses_before_long_break = 2"));
    session.stub_notifications();
    let watch = session.watch();
    watch.until(|value| has_class(value, "waiting"));
    for (index, (phase, next)) in [
        ("focus", "short break"),
        ("short break", "focus"),
        ("focus", "long break"),
        ("long break", "focus"),
    ]
    .into_iter()
    .enumerate()
    {
        let running = session.control("start");
        assert!(
            running["tooltip"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{phase} · running\r"))
        );
        let paused = session.control("pause");
        assert!(
            paused["tooltip"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{phase} · paused\r"))
        );
        let armed = session.control("reset");
        assert_eq!(armed["tooltip"], paused["tooltip"]);
        session.control("start");
        session.expire_phase();
        let waiting = watch.until(|value| {
            value["tooltip"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{next} · waiting\r"))
        });
        assert!(!has_class(&waiting, "reset-armed"));
        for value in [&running, &paused, &armed, &waiting] {
            let tooltip = value["tooltip"].as_str().unwrap();
            assert_eq!(tooltip.split('\r').count(), 2);
            assert!(!tooltip.contains("click"));
            assert!(!tooltip.contains("left:"));
            assert!(!tooltip.contains("right:"));
            assert!(!tooltip.contains("middle"));
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while session.notification_count() < index + 1 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        let messages = fs::read_to_string(session.directory.join("notifications")).unwrap();
        assert_eq!(messages.lines().count(), index + 1);
        assert!(messages.lines().nth(index).unwrap().ends_with(&format!(
            "{phase} finished next: {next}. left-click to start."
        )));
    }
}

#[test]
fn two_panels_share_controls_and_reset_confirmation() {
    let session = Session::new(Some("notifications = false"));
    let first = session.watch();
    first.until(|value| has_class(value, "waiting"));
    let second = session.watch();
    second.until(|value| has_class(value, "waiting"));
    session.control("start");
    first.until(|value| has_class(value, "running"));
    second.until(|value| has_class(value, "running"));
    session.control("pause");
    let first_paused = first.until(|value| has_class(value, "paused"));
    let second_paused = second.until(|value| has_class(value, "paused"));
    assert_eq!(first_paused, second_paused);
    session.control("reset");
    first.until(|value| has_class(value, "reset-armed"));
    second.until(|value| has_class(value, "reset-armed"));
    session.control("reset");
    let reset = first.until(|value| has_class(value, "waiting"));
    second.until(|value| has_class(value, "waiting"));
    assert!(reset["text"].as_str().unwrap().ends_with("25:00"));
    assert!(!has_class(&reset, "reset-armed"));
}

#[test]
fn joining_or_removing_one_panel_keeps_the_cycle_but_a_new_session_resets() {
    let session = Session::new(Some("notifications = false"));
    let first = session.watch();
    first.until(|value| has_class(value, "waiting"));
    session.control("start");
    session.expire_phase();
    first.until(|value| value["alt"] == "short-break");
    session.control("start");
    let second = session.watch();
    second.until(|value| value["alt"] == "short-break" && has_class(value, "running"));
    drop(first);
    assert!(has_class(&session.control("pause"), "paused"));
    drop(second);
    let restarted = session.watch();
    let fresh = restarted.until(|value| has_class(value, "waiting"));
    assert_eq!(fresh["alt"], "focus");
    assert!(fresh["tooltip"].as_str().unwrap().contains("round 1/4"));
}

#[test]
fn config_is_a_session_snapshot_and_defaults_work_without_a_file() {
    let session = Session::new(None);
    let first = session.watch();
    let initial = first.until(|value| has_class(value, "waiting"));
    assert!(initial["text"].as_str().unwrap().ends_with("25:00"));
    fs::write(session.directory.join("config.toml"), "focus_minutes = 15").unwrap();
    let second = session.watch();
    let joined = second.until(|value| has_class(value, "waiting"));
    assert!(joined["text"].as_str().unwrap().ends_with("25:00"));
    drop(first);
    drop(second);
    let restarted = session.watch();
    let fresh = restarted.until(|value| has_class(value, "waiting"));
    assert!(fresh["text"].as_str().unwrap().ends_with("15:00"));
}

#[test]
fn completion_is_claimed_once_and_notification_failures_do_not_stop_the_timer() {
    for (enabled, has_notify_send) in [(true, true), (false, true), (true, false)] {
        let session = Session::new(Some(&format!("notifications = {enabled}")));
        if has_notify_send {
            session.stub_notifications();
        }
        let first = session.watch();
        first.until(|value| has_class(value, "waiting"));
        let second = session.watch();
        second.until(|value| has_class(value, "waiting"));
        session.control("start");
        session.expire_phase();
        first.until(|value| value["alt"] == "short-break" && has_class(value, "waiting"));
        second.until(|value| value["alt"] == "short-break" && has_class(value, "waiting"));
        if enabled && has_notify_send {
            let deadline = Instant::now() + Duration::from_secs(3);
            while session.notification_count() == 0 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(20));
            }
        }
        thread::sleep(Duration::from_millis(450));
        assert_eq!(
            session.notification_count(),
            usize::from(enabled && has_notify_send)
        );
        assert!(has_class(&session.control("start"), "running"));
    }
}

#[test]
fn invalid_config_stays_visible_and_recovers_when_fixed() {
    let session = Session::new(Some("focus_minutes = 0"));
    let watch = session.watch();
    let error = watch.until(|value| has_class(value, "error"));
    assert!(error["tooltip"].as_str().unwrap().contains("focus_minutes"));
    fs::write(session.directory.join("config.toml"), "focus_minutes = 10").unwrap();
    let fixed = watch.until(|value| has_class(value, "waiting"));
    assert!(fixed["text"].as_str().unwrap().ends_with("10:00"));
}

#[test]
fn controls_need_an_active_session_and_cli_errors_are_readable() {
    let session = Session::new(None);
    let result = session.command("start").output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no active pomodoro panel"));
    for argument in ["--help", "--version"] {
        let result = Command::new(env!("CARGO_BIN_EXE_waybar-pomodoro"))
            .arg(argument)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(!result.stdout.is_empty());
    }
    let result = Command::new(env!("CARGO_BIN_EXE_waybar-pomodoro"))
        .arg("--config")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--config needs a path"));
}
