use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::config::Settings;
use crate::paths;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Listening,
    Transcribing,
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    state: State,
    child: Option<u32>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                state: State::Idle,
                child: None,
            })),
        }
    }

    pub fn state(&self) -> State {
        self.inner.lock().map(|g| g.state).unwrap_or(State::Idle)
    }

    pub fn start(&self, _settings: &Settings) -> Result<(), String> {
        let mut g = self.inner.lock().map_err(|e| e.to_string())?;
        if g.state == State::Listening {
            return Ok(());
        }
        let wav = wav_path();
        let wav_str = wav
            .to_str()
            .ok_or_else(|| "Voice path is not valid UTF-8".to_string())?
            .to_string();
        let _ = fs::remove_file(&wav);
        let child = Command::new("pw-record")
            .args([
                "--rate",
                "16000",
                "--channels",
                "1",
                "--format",
                "s16",
                &wav_str,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("pw-record failed: {e}"))?;
        g.child = Some(child.id());
        g.state = State::Listening;
        Ok(())
    }

    pub fn stop(&self, settings: &Settings) -> Result<String, String> {
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            if let Some(pid) = g.child.take() {
                stop_pid(pid);
            }
            g.state = State::Transcribing;
        }
        thread::sleep(Duration::from_millis(180));
        let result = transcribe(settings);
        if let Ok(mut g) = self.inner.lock() {
            g.state = State::Idle;
        }
        result
    }

    pub fn cancel(&self) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(pid) = g.child.take() {
                stop_pid(pid);
            }
            g.state = State::Idle;
        }
        let _ = fs::remove_file(wav_path());
    }
}

fn transcribe(settings: &Settings) -> Result<String, String> {
    let wav = wav_path();
    if !wav.exists() {
        return Err("No recording captured".into());
    }
    let meta = fs::metadata(&wav).map_err(|e| e.to_string())?;
    if meta.len() < 512 {
        let _ = fs::remove_file(&wav);
        return Err("That was too short — try again".into());
    }
    if !which("voxtype") {
        let _ = fs::remove_file(&wav);
        return Err("voxtype is required to transcribe".into());
    }
    let mut cmd = Command::new("voxtype");
    cmd.args(["transcribe", wav.to_str().unwrap_or("")]);
    if !settings.voice.language.is_empty() {
        cmd.args(["--language", &settings.voice.language]);
    }
    if !settings.voice.model.is_empty() {
        cmd.args(["--model", &settings.voice.model]);
    }
    let output = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("transcribe failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let _ = fs::remove_file(&wav);
    if !stdout.is_empty() {
        return Ok(stdout);
    }
    if !stderr.is_empty() {
        return Err(stderr.lines().next().unwrap_or("No speech detected").into());
    }
    Err("No speech detected".into())
}

fn stop_pid(pid: u32) {
    let _ = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status();
    thread::sleep(Duration::from_millis(80));
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
}

fn wav_path() -> std::path::PathBuf {
    paths::runtime_dir().join("voice.wav")
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}
