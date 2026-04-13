#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod refiner;
mod vad;

use config::Config;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

struct AppState {
    whisper: Mutex<Option<WhisperContext>>,
    refiner_llm: Mutex<Option<refiner::LlmRefiner>>,
    config: Mutex<Config>,
    last_pasted: Mutex<String>,
}

// -- Tauri Commands --

#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> Config {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn save_config(state: tauri::State<'_, AppState>, config: Config, handle: tauri::AppHandle) {
    let old_whisper = state.config.lock().unwrap().whisper_model.clone();
    let old_refiner_mode = state.config.lock().unwrap().refiner_mode.clone();
    let old_refiner_model = state.config.lock().unwrap().refiner_model.clone();

    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = config.clone();
        cfg.save();
    }

    // Reload whisper if model changed
    if config.whisper_model != old_whisper {
        let handle = handle.clone();
        std::thread::spawn(move || {
            emit_status(&handle, "Reloading Whisper model...");
            let model_path = Config::models_dir().join(format!("ggml-{}.bin", config.whisper_model));
            if model_path.exists() {
                let params = WhisperContextParameters::default();
                if let Ok(ctx) = WhisperContext::new_with_params(model_path.to_str().unwrap(), params) {
                    let state = handle.state::<AppState>();
                    *state.whisper.lock().unwrap() = Some(ctx);
                }
            }
            emit_status(&handle, "Ready");
        });
    }

    // Reload refiner if mode or model changed
    if config.refiner_mode != old_refiner_mode || config.refiner_model != old_refiner_model {
        let handle = handle.clone();
        std::thread::spawn(move || {
            let state = handle.state::<AppState>();
            if config.refiner_mode == "rewrite" {
                emit_status(&handle, "Loading refiner...");
                let refiner_path = Config::models_dir().join(format!("{}.gguf", config.refiner_model));
                if refiner_path.exists() {
                    match refiner::LlmRefiner::new(&refiner_path) {
                        Ok(r) => {
                            *state.refiner_llm.lock().unwrap() = Some(r);
                        }
                        Err(e) => eprintln!("[Main] Refiner reload error: {}", e),
                    }
                }
            } else {
                // Kill the server if switching away from rewrite
                *state.refiner_llm.lock().unwrap() = None;
            }
            emit_status(&handle, "Ready");
        });
    }
}

#[tauri::command]
fn get_stats(state: tauri::State<'_, AppState>) -> (u32, u32) {
    let cfg = state.config.lock().unwrap();
    (cfg.stats.words_today, cfg.stats.transcriptions_today)
}

// --- Permission checks (macOS) ---

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[cfg(target_os = "macos")]
fn is_accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

#[cfg(not(target_os = "macos"))]
fn is_accessibility_granted() -> bool { true }

#[tauri::command]
fn check_permissions() -> serde_json::Value {
    serde_json::json!({
        "accessibility": is_accessibility_granted(),
        "microphone": is_microphone_granted(),
    })
}

/// Check mic permission by briefly opening a stream and testing if samples flow.
/// Returns true if we can receive non-silent audio; false if denied.
fn is_microphone_granted() -> bool {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::OnceLock;
    // Cache result — we only need to check once per session
    static CACHED: OnceLock<bool> = OnceLock::new();
    if let Some(&v) = CACHED.get() { return v; }

    let buffer = audio::new_buffer();
    let stream = match audio::start_recording(&buffer) {
        Ok(s) => s,
        Err(_) => {
            CACHED.set(false).ok();
            return false;
        }
    };
    // Wait briefly for samples
    std::thread::sleep(std::time::Duration::from_millis(300));
    drop(stream);

    let samples = audio::take_samples(&buffer);
    // If we got samples and any are non-zero, mic is granted.
    // If we got samples but all are zero, mic is silently denied (common on macOS without permission).
    let granted = !samples.is_empty() && samples.iter().any(|&s| s.abs() > 0.0001);
    // Only cache positive result; denial might change after user grants permission.
    if granted {
        CACHED.set(true).ok();
    }
    granted
}

#[tauri::command]
fn request_accessibility() {
    #[cfg(target_os = "macos")]
    {
        // Open System Settings > Privacy & Security > Accessibility
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
}

/// Start the hotkey listener. Called after onboarding completes to avoid
/// triggering the Accessibility prompt on first launch.
#[tauri::command]
fn start_hotkey_listener_cmd(handle: tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return; // already started
    }
    std::thread::spawn(move || {
        start_hotkey_listener(handle);
    });
}

#[tauri::command]
fn request_microphone() {
    #[cfg(target_os = "macos")]
    {
        // First try to trigger the system mic prompt by opening a stream briefly.
        // On the very first attempt, macOS shows the permission dialog.
        let buffer = audio::new_buffer();
        if let Ok(_stream) = audio::start_recording(&buffer) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // Also open System Settings to the Microphone pane as a fallback / for revoked permissions
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn();
    }
}

#[tauri::command]
fn get_model_status(state: tauri::State<'_, AppState>) -> serde_json::Value {
    let cfg = state.config.lock().unwrap();
    let whisper_loaded = state.whisper.lock().unwrap().is_some();
    let refiner_loaded = state.refiner_llm.lock().unwrap().is_some();
    serde_json::json!({
        "whisper": cfg.whisper_model,
        "whisperStatus": if whisper_loaded { "Loaded" } else { "Loading..." },
        "refiner": if cfg.use_refiner { cfg.refiner_model.as_str() } else { "Disabled" },
        "refinerStatus": if refiner_loaded { "Loaded (LLM)" } else if cfg.use_refiner { "Loading..." } else { "Off" },
    })
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            whisper: Mutex::new(None),
            refiner_llm: Mutex::new(None),
            config: Mutex::new(Config::default()),
            last_pasted: Mutex::new(String::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_config, save_config, get_stats, get_model_status,
            check_permissions, request_accessibility, request_microphone,
            start_hotkey_listener_cmd
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Store the bundled resource path so models can be found at runtime
            if let Ok(resource_dir) = app.path().resource_dir() {
                let _ = config::RESOURCE_DIR.set(resource_dir);
            }
            // Store user-writable config dir (~/Library/Application Support/com.speakeasy.desktop)
            if let Ok(config_dir) = app.path().app_config_dir() {
                let _ = config::CONFIG_DIR.set(config_dir);
            }

            // Load config now that paths are known
            let loaded = Config::load();
            *handle.state::<AppState>().config.lock().unwrap() = loaded;

            // Load whisper model in background
            let handle2 = handle.clone();
            std::thread::spawn(move || {
                emit_status(&handle2, "Loading Whisper model...");
                let cfg: Config = handle2.state::<AppState>().config.lock().unwrap().clone();
                let model_path = Config::models_dir().join(format!("ggml-{}.bin", cfg.whisper_model));

                if !model_path.exists() {
                    emit_status(&handle2, "Error: Whisper model not found");
                    eprintln!("[Main] Model not found: {:?}", model_path);
                    return;
                }

                let params = WhisperContextParameters::default();
                match WhisperContext::new_with_params(model_path.to_str().unwrap(), params) {
                    Ok(ctx) => {
                        let state = handle2.state::<AppState>();
                        *state.whisper.lock().unwrap() = Some(ctx);
                        // Wait a moment for frontend to be ready
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        emit_status(&handle2, "Ready");
                        let _ = handle2.emit("models-loaded", serde_json::json!({
                            "whisper": cfg.whisper_model,
                            "whisperStatus": "Loaded",
                            "refiner": if cfg.use_refiner { cfg.refiner_model.as_str() } else { "Disabled" },
                            "refinerStatus": if cfg.use_refiner { "Rule-based" } else { "Off" },
                        }));

                        // Load refiner if enabled
                        if cfg.use_refiner {
                            emit_status(&handle2, "Loading refiner model...");
                            let refiner_path = Config::models_dir()
                                .join(format!("{}.gguf", cfg.refiner_model));
                            if refiner_path.exists() {
                                match refiner::LlmRefiner::new(&refiner_path) {
                                    Ok(r) => {
                                        let state = handle2.state::<AppState>();
                                        *state.refiner_llm.lock().unwrap() = Some(r);
                                    }
                                    Err(e) => {
                                        eprintln!("[Main] Refiner error: {}", e);
                                    }
                                }
                            } else {
                                eprintln!("[Main] Refiner model not found: {:?}", refiner_path);
                            }
                        }

                        // Wait for frontend, then update status
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        emit_status(&handle2, "Ready");
                        let state = handle2.state::<AppState>();
                        let has_refiner = state.refiner_llm.lock().unwrap().is_some();
                        let _ = handle2.emit("models-loaded", serde_json::json!({
                            "whisper": cfg.whisper_model,
                            "whisperStatus": "Loaded",
                            "refiner": if cfg.use_refiner { cfg.refiner_model.as_str() } else { "Disabled" },
                            "refinerStatus": if has_refiner { "Loaded" } else if cfg.use_refiner { "Rule-based" } else { "Off" },
                        }));
                    }
                    Err(e) => {
                        emit_status(&handle2, "Error: Failed to load Whisper");
                        eprintln!("[Main] Whisper error: {}", e);
                    }
                }
            });

            // Start hotkey listener ONLY if onboarding is already complete.
            // Otherwise wait for the user to finish the onboarding (frontend will
            // call `start_hotkey_listener_cmd` after they click Continue).
            let handle3 = handle.clone();
            std::thread::spawn(move || {
                // Give setup a moment to load config
                std::thread::sleep(std::time::Duration::from_millis(100));
                let cfg = handle3.state::<AppState>().config.lock().unwrap().clone();
                if cfg.first_run_complete {
                    start_hotkey_listener(handle3);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running SpeakEasy");
}

// -- Hotkey Listener (polling-based, avoids CGEventTap crashes) --

fn start_hotkey_listener(handle: tauri::AppHandle) {
    use device_query::{DeviceQuery, DeviceState, Keycode};


    let device_state = DeviceState::new();
    let buffer = audio::new_buffer();
    let mut is_recording = false;
    let stream_handle: std::sync::Arc<Mutex<Option<Box<dyn std::any::Any>>>> = std::sync::Arc::new(Mutex::new(None));

    loop {
        let keys = device_state.get_keys();

        let alt_held = keys.contains(&Keycode::LAlt) || keys.contains(&Keycode::RAlt)
            || keys.contains(&Keycode::LOption) || keys.contains(&Keycode::ROption);

        if alt_held && !is_recording {
            // Start recording
            is_recording = true;
            emit_status(&handle, "Recording...");
            let _ = handle.emit("recording", true);
            match audio::start_recording(&buffer) {
                Ok(stream) => { *stream_handle.lock().unwrap() = Some(stream); }
                Err(e) => eprintln!("[Hotkey] Record error: {}", e),
            }
        } else if !alt_held && is_recording {
            // Stop recording
            is_recording = false;
            let _ = handle.emit("recording", false);
            *stream_handle.lock().unwrap() = None; // Drop stops recording
            let samples = audio::take_samples(&buffer);
            let h = handle.clone();
            std::thread::spawn(move || {
                process_samples(&h, samples);
            });
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

// -- Pipeline --

fn process_samples(handle: &tauri::AppHandle, samples: Vec<f32>) {
    if samples.is_empty() {
        emit_status(handle, "No audio captured");
        return;
    }

    let duration = samples.len() as f32 / 16000.0;
    let max_val = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    let rms: f64 = (samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len() as f64).sqrt();

    emit_transcript(handle, &format!("Audio: {:.1}s, {} samples, max={:.4}, rms={:.6}", duration, samples.len(), max_val, rms));
    emit_status(handle, &format!("Processing {:.1}s audio...", duration));

    let state = handle.state::<AppState>();
    let cfg = state.config.lock().unwrap().clone();

    if !vad::has_speech(&samples, cfg.vad_threshold) {
        emit_transcript(handle, &format!("VAD: no speech (threshold={})", cfg.vad_threshold));
        emit_status(handle, "Ready");
        return;
    }
    emit_transcript(handle, "VAD: speech detected");

    // Whisper transcription
    emit_status(handle, "Transcribing...");
    let whisper_guard = state.whisper.lock().unwrap();
    let whisper = match whisper_guard.as_ref() {
        Some(w) => w,
        None => {
            emit_status(handle, "Error: Whisper not loaded");
            return;
        }
    };

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(&cfg.language));
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    // Personal dictionary — biases Whisper toward correct spellings
    // Format as a sentence for better context, whisper.cpp uses this as initial_prompt
    let initial_prompt;
    if !cfg.dictionary.is_empty() {
        initial_prompt = format!("The following words may appear: {}.", cfg.dictionary.join(", "));
        eprintln!("[Whisper] Using initial_prompt: {}", initial_prompt);
        params.set_initial_prompt(&initial_prompt);
    }

    let mut wstate = whisper.create_state().expect("whisper state");
    if let Err(e) = wstate.full(params, &samples) {
        eprintln!("[Pipeline] Whisper error: {}", e);
        emit_status(handle, "Whisper error");
        return;
    }

    let n_segments = wstate.full_n_segments().unwrap_or(0);
    let mut text = String::new();
    for i in 0..n_segments {
        if let Ok(seg) = wstate.full_get_segment_text(i) {
            text.push_str(&seg);
        }
    }
    let text = text.trim().to_string();
    drop(whisper_guard);

    if text.is_empty() {
        emit_status(handle, "Ready");
        return;
    }
    emit_transcript(handle, &format!("[whisper] {}", text));

    // Check for action commands (undo, select all, etc.) — only if dictation enabled
    if cfg.dictation_enabled {
    match refiner::check_action(&text) {
        refiner::Action::Undo => {
            emit_transcript(handle, "[action] delete last");
            let last = state.last_pasted.lock().unwrap().clone();
            if !last.is_empty() {
                delete_chars(last.chars().count());
                *state.last_pasted.lock().unwrap() = String::new();
            }
            emit_status(handle, "Ready");
            return;
        }
        refiner::Action::SelectAll => {
            emit_transcript(handle, "[action] select all");
            trigger_action("a");
            emit_status(handle, "Ready");
            return;
        }
        refiner::Action::None => {}
    }
    }

    // Snippet check (before refining, on raw whisper text)
    if let Some(replacement) = cfg.match_snippet(&text) {
        emit_transcript(handle, &format!("[snippet] {}", replacement));
        paste_text(&replacement);
        // Track pasted text (with trailing space) for "delete that"
        *state.last_pasted.lock().unwrap() = format!("{} ", replacement);
        track_words(handle, &replacement);
        emit_status(handle, "Ready");
        return;
    }

    // Refine based on mode: "off", "clean", "rewrite"
    let mut output = text;
    match cfg.refiner_mode.as_str() {
        "clean" => {
            emit_status(handle, "Cleaning...");
            match refiner::refine_rules_only(&output) {
                Some(cleaned) => {
                    emit_transcript(handle, &format!("[cleaned] {}", cleaned));
                    output = cleaned;
                }
                None => {
                    emit_transcript(handle, "[skipped filler]");
                    emit_status(handle, "Ready");
                    return;
                }
            }
        }
        "rewrite" => {
            emit_status(handle, "Refining...");
            let state = handle.state::<AppState>();
            let refiner_guard = state.refiner_llm.lock().unwrap();
            let result = if let Some(ref llm) = *refiner_guard {
                llm.refine(&output, &cfg.tone)
            } else {
                refiner::refine_rules_only(&output)
            };
            drop(refiner_guard);

            match result {
                Some(refined) => {
                    emit_transcript(handle, &format!("[refined] {}", refined));
                    output = refined;
                }
                None => {
                    emit_transcript(handle, "[skipped filler]");
                    emit_status(handle, "Ready");
                    return;
                }
            }
        }
        _ => {} // "off" — use raw whisper output
    }

    // Apply dictation commands AFTER refining (so the LLM doesn't eat them)
    let output = if cfg.dictation_enabled {
        refiner::apply_commands(&output)
    } else {
        output
    };

    emit_transcript(handle, &format!("[output] {}", output));
    paste_text(&output);
    *state.last_pasted.lock().unwrap() = format!("{} ", output);
    track_words(handle, &output);
    emit_status(handle, "Ready");
}

/// Send N backspace key presses to delete the last N characters
fn delete_chars(n: usize) {
    std::thread::sleep(std::time::Duration::from_millis(500));

    #[cfg(target_os = "macos")]
    {
        // Use osascript to press backspace N times
        let script = format!(
            "tell application \"System Events\" to repeat {} times\nkey code 51\nend repeat",
            n
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output();
    }

    #[cfg(target_os = "windows")]
    {
        use enigo::{Enigo, Keyboard, Settings, Key, Direction};
        if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
            for _ in 0..n {
                let _ = enigo.key(Key::Backspace, Direction::Click);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn trigger_action(key: &str) {
    std::thread::sleep(std::time::Duration::from_millis(600));
    let script = format!(
        "tell application \"System Events\" to keystroke \"{}\" using command down",
        key
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output();
}

#[cfg(target_os = "windows")]
fn trigger_action(key: &str) {
    std::thread::sleep(std::time::Duration::from_millis(600));
    use enigo::{Enigo, Keyboard, Settings, Key, Direction};
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(Key::Control, Direction::Press);
        if let Some(ch) = key.chars().next() {
            let _ = enigo.key(Key::Unicode(ch), Direction::Click);
        }
        let _ = enigo.key(Key::Control, Direction::Release);
    }
}

fn paste_text(text: &str) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Err(e) = clipboard.set_text(format!("{} ", text)) {
            eprintln!("[Paste] Clipboard error: {}", e);
            return;
        }
    }

    // Wait for Option key to fully release
    std::thread::sleep(std::time::Duration::from_millis(400));

    // Use osascript on macOS (more reliable than enigo for Accessibility)
    #[cfg(target_os = "macos")]
    {
        let result = std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to keystroke \"v\" using command down"])
            .output();
        match result {
            Ok(output) => {
                if output.status.success() {
                } else {
                    eprintln!("[Paste] osascript failed: {}", String::from_utf8_lossy(&output.stderr));
                    // Fallback to enigo
                    paste_with_enigo();
                }
            }
            Err(e) => {
                eprintln!("[Paste] osascript error: {}", e);
                paste_with_enigo();
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        paste_with_enigo();
    }
}

fn paste_with_enigo() {
    use enigo::{Enigo, Keyboard, Settings, Key, Direction};
    match Enigo::new(&Settings::default()) {
        Ok(mut enigo) => {
            #[cfg(target_os = "macos")]
            {
                let _ = enigo.key(Key::Meta, Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                let _ = enigo.key(Key::Meta, Direction::Release);
            }
            #[cfg(target_os = "windows")]
            {
                let _ = enigo.key(Key::Control, Direction::Press);
                let _ = enigo.key(Key::Unicode('v'), Direction::Click);
                let _ = enigo.key(Key::Control, Direction::Release);
            }
        }
        Err(e) => eprintln!("[Paste] enigo init failed: {}", e),
    }
}

fn track_words(handle: &tauri::AppHandle, text: &str) {
    let word_count = text.split_whitespace().count() as u32;
    let state = handle.state::<AppState>();
    let mut cfg = state.config.lock().unwrap();
    cfg.stats.record(word_count);
    cfg.save();
}

fn emit_status(handle: &tauri::AppHandle, msg: &str) {
    let _ = handle.emit("status", msg);
}

fn emit_transcript(handle: &tauri::AppHandle, line: &str) {
    let _ = handle.emit("transcript", line);
}
