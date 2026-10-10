use std::process::{Child, Command, Stdio};

use crate::store::Snapshot;

#[derive(Default)]
pub struct Notifier {
    children: Vec<Child>,
}

impl Notifier {
    pub fn send(&mut self, snapshot: &Snapshot) {
        if !snapshot.state.config.notifications {
            return;
        }
        let Some(completed) = snapshot.completion else {
            return;
        };
        let summary = format!("{} finished", completed.finished.name());
        let body = format!("next: {}. left-click to start.", completed.next.name());
        match Command::new("notify-send")
            .args([
                "--app-name=waybar-pomodoro",
                "--urgency=normal",
                "--expire-time=8000",
                "--icon=appointment-soon",
            ])
            .arg(summary)
            .arg(body)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => self.children.push(child),
            Err(err) => eprintln!("waybar-pomodoro: could not send notification: {err}"),
        }
    }

    pub fn reap(&mut self) {
        self.children.retain_mut(|child| match child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                if !status.success() {
                    eprintln!("waybar-pomodoro: notify-send exited with {status}");
                }
                false
            }
            Err(err) => {
                eprintln!("waybar-pomodoro: could not reap notification process: {err}");
                false
            }
        });
    }
}
