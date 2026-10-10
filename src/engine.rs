use serde::{Deserialize, Serialize};

use crate::{Result, clock::Sample, config::Config};

pub const RESET_WINDOW_MS: u64 = 3_000;
const SUSPEND_THRESHOLD_MS: u64 = 100;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short break",
            Self::LongBreak => "long break",
        }
    }

    pub fn class(self) -> &'static str {
        match self {
            Self::Focus => "focus",
            Self::ShortBreak => "short-break",
            Self::LongBreak => "long-break",
        }
    }

    pub fn duration_ms(self, config: &Config) -> u64 {
        (match self {
            Self::Focus => config.focus_minutes,
            Self::ShortBreak => config.short_break_minutes,
            Self::LongBreak => config.long_break_minutes,
        }) * 60_000
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Waiting,
    Running,
    Paused,
}

impl Status {
    pub fn class(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Paused => "paused",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Start,
    Pause,
    Reset,
}

#[derive(Clone, Copy, Debug)]
pub struct Completion {
    pub finished: Phase,
    pub next: Phase,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u8,
    pub config: Config,
    pub phase: Phase,
    pub status: Status,
    pub completed_focuses: u32,
    pub remaining_ms: u64,
    pub deadline_ms: Option<u64>,
    pub sleep_offset_ms: u64,
    pub reset_armed_until_ms: Option<u64>,
}

impl State {
    pub fn new(config: Config, now: Sample) -> Self {
        let remaining_ms = Phase::Focus.duration_ms(&config);
        Self {
            version: 1,
            config,
            phase: Phase::Focus,
            status: Status::Waiting,
            completed_focuses: 0,
            remaining_ms,
            deadline_ms: None,
            sleep_offset_ms: now.sleep_offset_ms(),
            reset_armed_until_ms: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.config.validate()?;
        let counter_valid = match self.phase {
            Phase::Focus => self.completed_focuses < self.config.focuses_before_long_break,
            Phase::ShortBreak => {
                self.completed_focuses > 0
                    && self.completed_focuses < self.config.focuses_before_long_break
            }
            Phase::LongBreak => self.completed_focuses == self.config.focuses_before_long_break,
        };
        if self.version != 1
            || !counter_valid
            || self.remaining_ms > self.phase.duration_ms(&self.config)
            || (self.status == Status::Running) != self.deadline_ms.is_some()
        {
            return Err(
                "invalid runtime state; restart all waybar panels to start a fresh session".into(),
            );
        }
        Ok(())
    }

    pub fn remaining_at(&self, now: Sample) -> u64 {
        self.deadline_ms.map_or(self.remaining_ms, |deadline| {
            deadline.saturating_sub(now.monotonic_ms)
        })
    }

    pub fn reset_armed(&self, now: Sample) -> bool {
        self.reset_armed_until_ms
            .is_some_and(|until| now.boottime_ms < until)
    }

    pub fn update(&mut self, now: Sample) -> Option<Completion> {
        if !self.reset_armed(now) {
            self.reset_armed_until_ms = None;
        }

        let suspended =
            now.sleep_offset_ms().saturating_sub(self.sleep_offset_ms) > SUSPEND_THRESHOLD_MS;
        if suspended {
            self.sleep_offset_ms = now.sleep_offset_ms();
            self.reset_armed_until_ms = None;
        }

        if self.status != Status::Running {
            return None;
        }
        let remaining = self.remaining_at(now);
        if remaining == 0 {
            return Some(self.finish());
        }
        if suspended {
            self.remaining_ms = remaining;
            self.deadline_ms = None;
            self.status = Status::Paused;
        }
        None
    }

    pub fn apply(&mut self, action: Action, now: Sample) -> Option<Completion> {
        let completed = self.update(now);
        match action {
            Action::Start => {
                self.reset_armed_until_ms = None;
                if self.status != Status::Running {
                    self.status = Status::Running;
                    self.deadline_ms = Some(now.monotonic_ms.saturating_add(self.remaining_ms));
                }
            }
            Action::Pause => {
                self.reset_armed_until_ms = None;
                if self.status == Status::Running {
                    self.remaining_ms = self.remaining_at(now);
                    self.deadline_ms = None;
                    self.status = Status::Paused;
                }
            }
            Action::Reset => {
                if self.reset_armed(now) {
                    *self = Self::new(self.config.clone(), now);
                } else {
                    self.reset_armed_until_ms =
                        Some(now.boottime_ms.saturating_add(RESET_WINDOW_MS));
                }
            }
        }
        completed
    }

    fn finish(&mut self) -> Completion {
        let finished = self.phase;
        self.phase = match finished {
            Phase::Focus => {
                self.completed_focuses += 1;
                if self.completed_focuses == self.config.focuses_before_long_break {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak => Phase::Focus,
            Phase::LongBreak => {
                self.completed_focuses = 0;
                Phase::Focus
            }
        };
        self.status = Status::Waiting;
        self.remaining_ms = self.phase.duration_ms(&self.config);
        self.deadline_ms = None;
        self.reset_armed_until_ms = None;
        Completion {
            finished,
            next: self.phase,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Sample {
        Sample {
            monotonic_ms: ms,
            boottime_ms: ms,
        }
    }

    #[test]
    fn full_cycle_waits_at_every_transition() {
        let mut state = State::new(Config::default(), at(0));
        let mut now = 0;
        for round in 1..=4 {
            assert_eq!(state.phase, Phase::Focus);
            assert_eq!(state.status, Status::Waiting);
            now += 100_000;
            assert!(state.update(at(now)).is_none());
            assert_eq!(state.remaining_at(at(now)), 25 * 60_000);
            state.apply(Action::Start, at(now));
            now += 25 * 60_000;
            let completed = state.update(at(now)).unwrap();
            assert_eq!(completed.finished, Phase::Focus);
            assert_eq!(state.completed_focuses, round);
            assert_eq!(
                state.phase,
                if round == 4 {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            );
            assert_eq!(state.status, Status::Waiting);
            assert!(state.update(at(now + 1_000_000)).is_none());
            now += 1_000_000;
            state.apply(Action::Start, at(now));
            now += if round == 4 { 30 } else { 5 } * 60_000;
            assert!(state.update(at(now)).is_some());
            state.validate().unwrap();
        }
        assert_eq!(state.phase, Phase::Focus);
        assert_eq!(state.completed_focuses, 0);
        assert_eq!(state.status, Status::Waiting);
    }

    #[test]
    fn custom_round_count_and_one_round_cycle() {
        let mut state = State::new(
            Config {
                focuses_before_long_break: 1,
                ..Config::default()
            },
            at(0),
        );
        state.apply(Action::Start, at(0));
        state.update(at(25 * 60_000));
        assert_eq!(state.phase, Phase::LongBreak);
        state.validate().unwrap();
    }

    #[test]
    fn pause_and_resume_keep_the_remaining_time() {
        let mut state = State::new(Config::default(), at(0));
        state.apply(Action::Pause, at(0));
        assert_eq!(state.status, Status::Waiting);
        state.apply(Action::Start, at(0));
        state.apply(Action::Start, at(1_000));
        assert_eq!(state.deadline_ms, Some(1_500_000));
        state.apply(Action::Pause, at(10_250));
        assert_eq!(state.remaining_ms, 1_489_750);
        state.apply(Action::Pause, at(20_000));
        assert_eq!(state.remaining_ms, 1_489_750);
        state.apply(Action::Start, at(500_000));
        assert!(state.update(at(1_989_749)).is_none());
        assert!(state.update(at(1_989_750)).is_some());
    }

    #[test]
    fn reset_requires_two_clicks_strictly_inside_the_window() {
        for delay in [2_999, 3_000, 3_001] {
            let mut state = State::new(Config::default(), at(0));
            state.apply(Action::Start, at(0));
            state.apply(Action::Reset, at(1_000));
            assert_eq!(state.status, Status::Running);
            assert_eq!(state.remaining_at(at(2_000)), 1_498_000);
            state.apply(Action::Reset, at(1_000 + delay));
            assert_eq!(
                state.status,
                if delay < 3_000 {
                    Status::Waiting
                } else {
                    Status::Running
                }
            );
            assert_eq!(state.reset_armed(at(1_000 + delay)), delay >= 3_000);
        }
    }

    #[test]
    fn controls_and_transitions_cancel_reset() {
        for action in [Action::Start, Action::Pause] {
            let mut state = State::new(Config::default(), at(0));
            state.apply(Action::Start, at(0));
            state.apply(Action::Reset, at(1_000));
            state.apply(action, at(1_500));
            assert!(!state.reset_armed(at(1_500)));
        }
        let mut state = State::new(Config::default(), at(0));
        state.apply(Action::Start, at(0));
        state.apply(Action::Reset, at(1_499_000));
        state.update(at(1_500_000));
        assert!(!state.reset_armed(at(1_500_000)));
    }

    #[test]
    fn sleep_pauses_without_counting_suspended_time() {
        let mut state = State::new(Config::default(), at(0));
        state.apply(Action::Start, at(0));
        state.apply(Action::Reset, at(1_000));
        let resumed = Sample {
            monotonic_ms: 10_000,
            boottime_ms: 3_610_000,
        };
        assert!(state.update(resumed).is_none());
        assert_eq!(state.status, Status::Paused);
        assert_eq!(state.remaining_ms, 1_490_000);
        assert!(!state.reset_armed(resumed));
        state.apply(Action::Start, resumed);
        assert_eq!(state.status, Status::Running);
        assert!(state.update(resumed).is_none());
        assert_eq!(state.status, Status::Running);
    }

    #[test]
    fn sleep_does_not_start_waiting_or_paused_stages() {
        let mut state = State::new(Config::default(), at(0));
        state.update(Sample {
            monotonic_ms: 1_000,
            boottime_ms: 3_601_000,
        });
        assert_eq!(state.status, Status::Waiting);
        state.apply(
            Action::Start,
            Sample {
                monotonic_ms: 2_000,
                boottime_ms: 3_602_000,
            },
        );
        assert_eq!(state.status, Status::Running);
    }

    #[test]
    fn late_update_completes_only_one_phase() {
        let mut state = State::new(Config::default(), at(0));
        state.apply(Action::Start, at(0));
        assert!(state.update(at(100_000_000)).is_some());
        assert!(state.update(at(100_000_001)).is_none());
        assert_eq!(state.phase, Phase::ShortBreak);
        assert_eq!(state.remaining_ms, 300_000);
    }
}
