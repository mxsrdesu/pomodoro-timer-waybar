use serde::Serialize;

use crate::{
    clock::Sample,
    engine::{Phase, State, Status},
};

#[derive(Clone, Copy, Default)]
pub enum RenderMode {
    #[default]
    Combined,
    TextOnly,
}

#[derive(Debug, Serialize)]
pub struct Output {
    pub text: String,
    pub alt: String,
    pub tooltip: String,
    pub class: Vec<String>,
}

impl Output {
    pub fn from_state(state: &State, now: Sample) -> Self {
        Self::from_state_with_mode(state, now, RenderMode::Combined)
    }

    pub fn from_state_with_mode(state: &State, now: Sample, mode: RenderMode) -> Self {
        let armed = state.reset_armed(now);
        let icon = if armed {
            "󰀦"
        } else {
            match state.status {
                Status::Waiting => "󰐊",
                Status::Paused => "󰏤",
                Status::Running => match state.phase {
                    Phase::Focus => "󰔟",
                    Phase::ShortBreak => "󰅶",
                    Phase::LongBreak => "󰒲",
                },
            }
        };
        let round = state.completed_focuses + u32::from(state.phase == Phase::Focus);
        let status = match state.status {
            Status::Waiting => "waiting",
            Status::Running => "running",
            Status::Paused => "paused",
        };
        let tooltip = format!(
            "{} · {status}\rround {round}/{}",
            state.phase.name(),
            state.config.focuses_before_long_break
        );
        let mut class = vec![state.phase.class().into(), state.status.class().into()];
        if armed {
            class.push("reset-armed".into());
        }
        let time = format_time(state.remaining_at(now));
        let (text, alt) = match mode {
            RenderMode::Combined => (format!("{icon}  {time}"), state.phase.class().into()),
            RenderMode::TextOnly => (
                time,
                if armed {
                    "reset-armed".into()
                } else {
                    format!("{}-{}", state.phase.class(), state.status.class())
                },
            ),
        };
        Self {
            text,
            alt,
            tooltip,
            class,
        }
    }

    pub fn error(message: &str) -> Self {
        Self::error_with_mode(message, RenderMode::Combined)
    }

    pub fn error_with_mode(message: &str, mode: RenderMode) -> Self {
        Self {
            text: match mode {
                RenderMode::Combined => "󰀦  --:--",
                RenderMode::TextOnly => "--:--",
            }
            .into(),
            alt: "error".into(),
            tooltip: escape_markup(message),
            class: vec!["error".into()],
        }
    }
}

fn format_time(ms: u64) -> String {
    let seconds = ms.div_ceil(1_000);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn escape_markup(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, engine::Action};

    #[test]
    fn countdown_rounds_up_and_supports_long_durations() {
        assert_eq!(format_time(1_500_000), "25:00");
        assert_eq!(format_time(1_499_999), "25:00");
        assert_eq!(format_time(1_499_000), "24:59");
        assert_eq!(format_time(1), "00:01");
        assert_eq!(format_time(0), "00:00");
        assert_eq!(format_time(120 * 60_000), "120:00");
    }

    #[test]
    fn output_distinguishes_waiting_paused_and_armed_states() {
        let now = Sample {
            monotonic_ms: 0,
            boottime_ms: 0,
        };
        let mut state = State::new(Config::default(), now);
        let output = Output::from_state(&state, now);
        assert_eq!(output.class, ["focus", "waiting"]);
        assert!(output.tooltip.contains("round 1/4"));
        state.apply(Action::Start, now);
        state.apply(Action::Pause, now);
        assert_eq!(Output::from_state(&state, now).class, ["focus", "paused"]);
        state.apply(Action::Reset, now);
        assert!(
            Output::from_state(&state, now)
                .class
                .contains(&"reset-armed".into())
        );
        assert!(
            Output::error("bad <config> & input")
                .tooltip
                .contains("&lt;config&gt; &amp;")
        );
    }
}
