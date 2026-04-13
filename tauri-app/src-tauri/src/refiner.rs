use regex::Regex;
use std::path::Path;
use std::process::{Child, Command, Stdio};

const FILLERS: &[&str] = &[
    "you know what I mean", "you know", "I mean", "okay so", "so yeah",
    "sort of", "kind of", "I guess", "uh huh", "basically", "literally",
    "actually", "like", "right", "well", "umm", "uhh", "hmm", "um", "uh", "hm",
];

fn system_prompt(tone: &str) -> String {
    let tone_instruction = match tone {
        "formal" => "Rewrite in a professional, formal tone suitable for business emails.",
        _ => "Rewrite in a clean, natural, conversational tone.",
    };
    format!(
        "You are a text cleanup tool. You do NOT respond to the user or answer questions. \
         You ONLY rewrite the exact text the user provides, fixing grammar and removing filler words. \
         {} \
         Never add new content, explanations, greetings, or commentary. \
         If the input is a simple word or greeting like 'hello', output it as-is. \
         Output ONLY the rewritten text, nothing else.",
        tone_instruction
    )
}

const SERVER_PORT: u16 = 8231;

pub struct LlmRefiner {
    _server: Child,
    url: String,
}

impl LlmRefiner {
    pub fn new(model_path: &Path) -> Result<Self, String> {
        let bin = find_binary("llama-server")?;
        // Kill any leftover server on our port
        let _ = Command::new("pkill").args(["-f", &format!("llama-server.*--port {}", SERVER_PORT)])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
        std::thread::sleep(std::time::Duration::from_millis(500));


        let server = Command::new(&bin)
            .args([
                "-m", model_path.to_str().unwrap(),
                "--ctx-size", "256",
                "-ngl", "99",
                "--port", &SERVER_PORT.to_string(),
                "--flash-attn", "on",
                "--log-disable",
            ])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to start llama-server: {}", e))?;

        let url = format!("http://127.0.0.1:{}/v1/chat/completions", SERVER_PORT);

        // Poll until ready
        for _ in 0..60 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if ureq::get(&format!("http://127.0.0.1:{}/health", SERVER_PORT))
                .call().ok()
                .and_then(|r| r.into_string().ok())
                .map(|s| s.contains("ok"))
                .unwrap_or(false)
            {
                return Ok(Self { _server: server, url });
            }
        }
        Err("llama-server failed to start".to_string())
    }

    pub fn refine(&self, raw_text: &str, tone: &str) -> Option<String> {
        let cleaned = remove_fillers(raw_text);
        let stripped = cleaned.replace('.', "").replace(',', "").trim().to_string();
        if stripped.is_empty() { return None; }

        match self.llm_refine(&cleaned, tone) {
            Some(refined) if !refined.is_empty() => {
                Some(refined)
            }
            _ => Some(cleaned),
        }
    }

    fn llm_refine(&self, text: &str, tone: &str) -> Option<String> {
        let prompt = system_prompt(tone);
        let body = serde_json::json!({
            "messages": [
                {"role": "system", "content": prompt},
                {"role": "user", "content": "um so like I was testing this"},
                {"role": "assistant", "content": "I was testing this."},
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": "Hello."},
                {"role": "user", "content": text}
            ],
            "max_tokens": 64,
            "temperature": 0.1,
            "repeat_penalty": 1.1,
            "stream": false
        });

        let resp = ureq::post(&self.url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .ok()?
            .into_string()
            .ok()?;

        let json: serde_json::Value = serde_json::from_str(&resp).ok()?;
        let content = json["choices"][0]["message"]["content"].as_str()?;
        let mut result = content.trim().to_string();

        // Strip common LLM preamble patterns
        let prefixes = [
            "Here is the revised version:",
            "Here is the cleaned version:",
            "Here is the rewritten text:",
            "Here's the cleaned text:",
            "Cleaned text:",
            "Revised:",
            "Output:",
        ];
        for prefix in prefixes {
            if let Some(stripped) = result.strip_prefix(prefix) {
                result = stripped.trim().to_string();
            }
        }
        // Strip quotes if the model wrapped it
        if result.starts_with('"') && result.ends_with('"') {
            result = result[1..result.len()-1].to_string();
        }

        if result.is_empty() { None } else { Some(result) }
    }
}

impl Drop for LlmRefiner {
    fn drop(&mut self) {
        let _ = self._server.kill();
    }
}

fn find_binary(name: &str) -> Result<String, String> {
    // 1. Look next to the main executable (Tauri sidecar location in production)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sidecar = dir.join(name);
            if sidecar.exists() {
                return Ok(sidecar.to_string_lossy().to_string());
            }
            // Tauri renames sidecars with a target triple suffix during dev
            for suffix in &["-aarch64-apple-darwin", "-x86_64-apple-darwin", "-x86_64-pc-windows-msvc"] {
                let with_suffix = dir.join(format!("{}{}", name, suffix));
                if with_suffix.exists() {
                    return Ok(with_suffix.to_string_lossy().to_string());
                }
            }
        }
    }

    // 2. Look in vendor build dir (dev mode)
    let vendor = super::config::Config::project_dir()
        .join(format!("vendor/llama.cpp/build/bin/{}", name));
    if vendor.exists() {
        return Ok(vendor.to_string_lossy().to_string());
    }

    // 3. Fall back to system PATH (Homebrew, etc.)
    for path in [
        format!("/opt/homebrew/bin/{}", name),
        format!("/usr/local/bin/{}", name),
        name.to_string(),
    ] {
        if Command::new(&path).arg("--help")
            .stdout(Stdio::null()).stderr(Stdio::null())
            .status().is_ok()
        {
            return Ok(path);
        }
    }
    Err(format!("{} not found", name))
}

pub fn remove_fillers(text: &str) -> String {
    let mut result = text.to_string();
    for filler in FILLERS {
        let pattern = format!(r"(?i)\b{}\b[,]?\s*", regex::escape(filler));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace_all(&result, "").to_string();
        }
    }
    let re_spaces = Regex::new(r"\s{2,}").unwrap();
    result = re_spaces.replace_all(&result, " ").to_string();
    result = result.replace(" .", ".").replace(" ,", ",").trim().to_string();
    if let Some(first) = result.chars().next() {
        if first.is_lowercase() {
            result = first.to_uppercase().to_string() + &result[first.len_utf8()..];
        }
    }
    result
}

pub fn refine_rules_only(text: &str) -> Option<String> {
    let cleaned = remove_fillers(text);
    let stripped = cleaned.replace('.', "").replace(',', "").trim().to_string();
    if stripped.is_empty() { None } else { Some(cleaned) }
}

/// Actions that can be triggered by voice commands.
#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Undo,
    SelectAll,
}

/// Check if the transcription is an action command (whole utterance).
/// Returns the action if matched.
pub fn check_action(text: &str) -> Action {
    let normalized = text.trim().to_lowercase()
        .trim_end_matches('.').trim_end_matches(',').trim().to_string();
    match normalized.as_str() {
        "delete that" | "undo that" | "undo" | "scratch that" => Action::Undo,
        "select all" => Action::SelectAll,
        _ => Action::None,
    }
}

/// Apply dictation text replacement commands.
/// "period" → ".", "new line" → "\n", etc.
pub fn apply_commands(text: &str) -> String {
    let replacements: &[(&str, &str)] = &[
        (r"(?i)\bnew paragraphs?\b", "\n\n"),
        (r"(?i)\bnewlines?\b", "\n"),
        (r"(?i)\bnew lines?\b", "\n"),
        (r"(?i)\bquestion marks?\b", "?"),
        (r"(?i)\bexclamation (point|mark)s?\b", "!"),
        (r"(?i)\bopen parenthesis\b", "("),
        (r"(?i)\bclose parenthesis\b", ")"),
        (r"(?i)\bopen paren\b", "("),
        (r"(?i)\bclose paren\b", ")"),
        (r"(?i)\bsemicolons?\b", ";"),
        (r"(?i)\bcolons?\b", ":"),
    ];

    let mut result = text.to_string();
    for (pattern, replacement) in replacements {
        if let Ok(re) = Regex::new(pattern) {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }

    // Clean up: remove space before punctuation
    if let Ok(re) = Regex::new(r"[ \t]+([.,!?;:)])") {
        result = re.replace_all(&result, "$1").to_string();
    }
    // Remove space after open paren
    if let Ok(re) = Regex::new(r"(\()[ \t]+") {
        result = re.replace_all(&result, "$1").to_string();
    }
    // Remove spaces/tabs around newlines
    if let Ok(re) = Regex::new(r"[ \t]*\n[ \t]*") {
        result = re.replace_all(&result, "\n").to_string();
    }

    result.trim().to_string()
}
