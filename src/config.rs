use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub focus_minutes: u64,
    pub short_break_minutes: u64,
    pub long_break_minutes: u64,
    pub focuses_before_long_break: u32,
    pub notifications: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 30,
            focuses_before_long_break: 4,
            notifications: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(format!("cannot read {}: {err}", path.display()).into()),
        };
        let config: Self = toml::from_str(&contents)
            .map_err(|err| format!("invalid config {}: {err}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        for (name, minutes) in [
            ("focus_minutes", self.focus_minutes),
            ("short_break_minutes", self.short_break_minutes),
            ("long_break_minutes", self.long_break_minutes),
        ] {
            if minutes == 0
                || minutes
                    .checked_mul(60_000)
                    .is_none_or(|ms| ms > u64::MAX / 2)
            {
                return Err(format!(
                    "{name} must be a positive whole number of minutes that fits in the timer"
                )
                .into());
            }
        }
        if self.focuses_before_long_break == 0 {
            return Err("focuses_before_long_break must be a positive whole number".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_uses_defaults() {
        let config: Config = toml::from_str("focus_minutes = 15\nnotifications = false").unwrap();
        config.validate().unwrap();
        assert_eq!(config.focus_minutes, 15);
        assert_eq!(config.short_break_minutes, 5);
        assert!(!config.notifications);
    }

    #[test]
    fn invalid_values_and_typos_are_rejected() {
        for source in [
            "focus_minutes = -1",
            "focus_minutes = 1.5",
            "focus_minute = 25",
            "notifications = 'yes'",
        ] {
            assert!(toml::from_str::<Config>(source).is_err(), "{source}");
        }
        for source in ["focus_minutes = 0", "focuses_before_long_break = 0"] {
            assert!(
                toml::from_str::<Config>(source)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let config = Config {
            long_break_minutes: u64::MAX,
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }
}
