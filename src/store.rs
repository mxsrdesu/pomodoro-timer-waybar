use std::{
    env,
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use crate::{
    Result,
    clock::Sample,
    config::Config,
    engine::{Action, Completion, State},
};

pub struct Paths {
    pub runtime: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    pub fn discover(config: Option<PathBuf>) -> Result<Self> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .filter(|value| !value.is_empty())
            .ok_or("XDG_RUNTIME_DIR is missing; run this inside your wayland session")?;
        let config = match config {
            Some(path) => path,
            None => {
                let home = match env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
                    Some(path) => PathBuf::from(path),
                    None => {
                        PathBuf::from(env::var_os("HOME").ok_or("HOME is missing")?).join(".config")
                    }
                };
                home.join("waybar-pomodoro/config.toml")
            }
        };
        Ok(Self {
            runtime: PathBuf::from(runtime).join("waybar-pomodoro"),
            config,
        })
    }
}

pub struct Snapshot {
    pub state: State,
    pub now: Sample,
    pub completion: Option<Completion>,
}

pub struct Store {
    directory: PathBuf,
    mutex: File,
    // A watch process holds this shared lock for its entire lifetime. No PID
    // files or stale-session heuristics: the kernel releases it on exit.
    presence: File,
}

impl Store {
    fn open(paths: &Paths) -> Result<Self> {
        match fs::DirBuilder::new().mode(0o700).create(&paths.runtime) {
            Ok(()) => (),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(err) => {
                return Err(format!("cannot create {}: {err}", paths.runtime.display()).into());
            }
        }
        Ok(Self {
            directory: paths.runtime.clone(),
            mutex: open_lock(&paths.runtime.join("state.lock"))?,
            presence: open_lock(&paths.runtime.join("presence.lock"))?,
        })
    }

    pub fn watch(paths: &Paths) -> Result<Self> {
        let store = Self::open(paths)?;
        {
            let _guard = Guard::acquire(&store.mutex)?;
            match store.presence.try_lock() {
                Ok(()) => {
                    let config = Config::load(&paths.config)?;
                    store.write(&State::new(config, Sample::now()))?;
                    // Explicitly unlock before changing lock type. The state
                    // lock keeps another join from racing this handover.
                    store.presence.unlock()?;
                }
                Err(TryLockError::WouldBlock) => {
                    store.read()?;
                }
                Err(TryLockError::Error(err)) => return Err(err.into()),
            }
            store.presence.lock_shared()?;
        }
        Ok(store)
    }

    pub fn tick(&self) -> Result<Snapshot> {
        let _guard = Guard::acquire(&self.mutex)?;
        self.transaction(None)
    }

    pub fn control(paths: &Paths, action: Action) -> Result<Snapshot> {
        let store = Self::open(paths)?;
        let _guard = Guard::acquire(&store.mutex)?;
        match store.presence.try_lock() {
            Ok(()) => {
                return Err(
                    "no active pomodoro panel; start waybar (or run waybar-pomodoro watch) first"
                        .into(),
                );
            }
            Err(TryLockError::WouldBlock) => (),
            Err(TryLockError::Error(err)) => return Err(err.into()),
        }
        store.transaction(Some(action))
    }

    fn transaction(&self, action: Option<Action>) -> Result<Snapshot> {
        let mut state = self.read()?;
        let previous = state.clone();
        let now = Sample::now();
        let completion = match action {
            Some(action) => state.apply(action, now),
            None => state.update(now),
        };
        if state != previous {
            // Commit the transition before attempting notification delivery;
            // only the process that claims it may send the notification.
            self.write(&state)?;
        }
        Ok(Snapshot {
            state,
            now,
            completion,
        })
    }

    fn read(&self) -> Result<State> {
        let contents = fs::read(self.directory.join("state.json"))?;
        let state: State = serde_json::from_slice(&contents)
            .map_err(|err| format!("invalid runtime state: {err}; restart all waybar panels"))?;
        state.validate()?;
        Ok(state)
    }

    fn write(&self, state: &State) -> Result<()> {
        let temporary = self.directory.join("state.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        serde_json::to_writer(&mut file, state)?;
        file.flush()?;
        fs::rename(temporary, self.directory.join("state.json"))?;
        Ok(())
    }
}

fn open_lock(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?)
}

struct Guard<'a>(&'a File);

impl<'a> Guard<'a> {
    fn acquire(file: &'a File) -> Result<Self> {
        file.lock()?;
        Ok(Self(file))
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
