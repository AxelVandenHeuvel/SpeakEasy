use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Resource directory — set once at startup by main.rs using Tauri's app handle
pub static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Config directory — user-writable location for config.json
pub static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snippet {
    pub trigger: String,
    pub replacement: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stats {
    #[serde(default)]
    pub words_today: u32,
    #[serde(default)]
    pub transcriptions_today: u32,
    #[serde(default)]
    pub date: String,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            words_today: 0,
            transcriptions_today: 0,
            date: String::new(),
        }
    }
}

impl Stats {
    pub fn record(&mut self, word_count: u32) {
        let today = chrono_today();
        if self.date != today {
            self.words_today = 0;
            self.transcriptions_today = 0;
            self.date = today;
        }
        self.words_today += word_count;
        self.transcriptions_today += 1;
    }
}

fn chrono_today() -> String {
    // Simple date without pulling in chrono crate
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let days = now / 86400;
    // Approximate — good enough for day change detection
    format!("day-{}", days)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default = "default_whisper")]
    pub whisper_model: String,
    #[serde(default = "default_refiner")]
    pub refiner_model: String,
    #[serde(default = "default_lang")]
    pub language: String,
    #[serde(default = "default_vad")]
    pub vad_threshold: f64,
    #[serde(default = "default_max_record")]
    pub max_record_seconds: u32,
    #[serde(default)]
    pub use_refiner: bool,
    #[serde(default = "default_refiner_mode")]
    pub refiner_mode: String,  // "off", "clean", "rewrite"
    #[serde(default = "default_tone")]
    pub tone: String,  // "normal", "formal"
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub first_run_complete: bool,
    #[serde(default = "default_dictation")]
    pub dictation_enabled: bool,
    #[serde(default)]
    pub dictionary: Vec<String>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
    #[serde(default)]
    pub stats: Stats,
}

fn default_whisper() -> String { "base.en".into() }
fn default_refiner() -> String { "qwen2.5-1.5b-instruct-q4_k_m".into() }
fn default_lang() -> String { "en".into() }
fn default_vad() -> f64 { 0.003 }
fn default_max_record() -> u32 { 30 }
fn default_refiner_mode() -> String { "rewrite".into() }
fn default_tone() -> String { "normal".into() }
fn default_hotkey() -> String { "Option".into() }
fn default_theme() -> String { "dusk-sprinter".into() }
fn default_dictation() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            whisper_model: default_whisper(),
            refiner_model: default_refiner(),
            language: default_lang(),
            vad_threshold: default_vad(),
            max_record_seconds: default_max_record(),
            use_refiner: true,
            refiner_mode: default_refiner_mode(),
            tone: default_tone(),
            hotkey: default_hotkey(),
            theme: default_theme(),
            first_run_complete: false,
            dictation_enabled: default_dictation(),
            dictionary: vec![],
            snippets: vec![],
            stats: Stats::default(),
        }
    }
}

impl Config {
    /// Find the SpeakEasy project root (contains models/ and config.json)
    pub fn project_dir() -> PathBuf {
        // Walk up from current_dir looking for the models/ folder
        let mut dir = std::env::current_dir().unwrap_or_default();
        for _ in 0..10 {
            if dir.join("models").is_dir() {
                return dir;
            }
            if !dir.pop() { break; }
        }
        // Fallback: try from exe path
        let mut dir = std::env::current_exe().unwrap_or_default();
        for _ in 0..10 {
            dir.pop();
            if dir.join("models").is_dir() {
                return dir;
            }
        }
        // Last resort
        std::env::current_dir().unwrap_or_default()
    }

    pub fn config_path() -> PathBuf {
        // Prefer the user-writable config dir set by Tauri's app_config_dir
        if let Some(dir) = CONFIG_DIR.get() {
            let _ = fs::create_dir_all(dir);
            return dir.join("config.json");
        }
        // Dev fallback: walk up to find project root
        Self::project_dir().join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, data);
        }
    }

    pub fn match_snippet(&self, text: &str) -> Option<String> {
        let normalized = text.trim().to_lowercase()
            .replace('.', "").replace(',', "");
        for snippet in &self.snippets {
            if normalized == snippet.trigger.to_lowercase() {
                return Some(snippet.replacement.clone());
            }
        }
        None
    }

    pub fn models_dir() -> PathBuf {
        // In production: use Tauri's resource_dir (models bundled into app)
        // Tauri bundles resources with their relative path prefix preserved,
        // so "../../models/foo" becomes "_up_/_up_/models/foo"
        if let Some(dir) = RESOURCE_DIR.get() {
            let candidates = [
                dir.join("_up_/_up_/models"),
                dir.join("_up_/models"),
                dir.join("models"),
                dir.clone(),
            ];
            for candidate in candidates {
                if candidate.join("ggml-base.en.bin").exists() {
                    return candidate;
                }
            }
        }
        // Dev mode: walk up from current dir looking for models/
        Self::project_dir().join("models")
    }
}
