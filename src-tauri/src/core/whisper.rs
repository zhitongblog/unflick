use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Locate bundled whisper binary and tiny model alongside the running executable.
/// Looks at `<exe_dir>/whisper/` and `<exe_dir>/resources/whisper/` (NSIS install layout).
pub fn find_bundled_whisper() -> Option<(PathBuf, PathBuf)> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let bin_name = if cfg!(target_os = "windows") { "whisper-cli.exe" } else { "whisper-cli" };
    for sub in ["whisper", "resources/whisper"] {
        let dir = exe_dir.join(sub);
        let bin = dir.join(bin_name);
        let model = dir.join("ggml-tiny.bin");
        if bin.exists() && model.exists() {
            return Some((bin, model));
        }
    }
    None
}

/// Hide the console flash on Windows when launching subprocesses.
fn suppress_console(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmd;
    }
}

/// On Windows, convert a path containing non-ASCII characters (e.g. Chinese)
/// to its 8.3 short form via GetShortPathNameW. Many native CLIs like
/// whisper-cli.exe and ffmpeg.exe parse argv using the system ANSI codepage,
/// so Unicode paths get mangled. Short paths are pure ASCII and always work.
/// Returns the original path string if conversion fails or isn't needed.
pub fn to_safe_path(p: &str) -> String {
    if p.is_ascii() {
        return p.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use std::ffi::{OsStr, OsString};

        unsafe extern "system" {
            fn GetShortPathNameW(lpsz_long: *const u16, lpsz_short: *mut u16, cch: u32) -> u32;
        }

        let wide: Vec<u16> = OsStr::new(p).encode_wide().chain(std::iter::once(0)).collect();
        let mut buf: Vec<u16> = vec![0; 1024];
        let len = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if len > 0 && (len as usize) < buf.len() {
            let s = OsString::from_wide(&buf[..len as usize]);
            return s.to_string_lossy().into_owned();
        }
    }
    p.to_string()
}

/// Generate subtitles using local whisper.cpp CLI.
///
/// Extracts audio via ffmpeg, runs the whisper binary, returns the .srt path.
/// `ffmpeg_path` should point to a bundled or system ffmpeg executable.
pub fn transcribe_local(
    video_path: &str,
    whisper_binary: &str,
    model_path: &str,
    output_dir: &str,
    ffmpeg_path: &str,
) -> Result<String> {
    // Convert paths to ASCII-safe short forms for native CLIs that don't
    // grok UTF-8 argv on Windows.
    let video_safe = to_safe_path(video_path);
    let whisper_safe = to_safe_path(whisper_binary);
    let model_safe = to_safe_path(model_path);
    let ffmpeg_safe = to_safe_path(ffmpeg_path);
    let output_dir_safe = to_safe_path(output_dir);

    // --- 1. Extract audio to WAV via ffmpeg ---
    let audio_path = format!("{}/temp_audio.wav", output_dir_safe);
    let mut cmd = Command::new(&ffmpeg_safe);
    cmd.args([
        "-y",
        "-i",
        &video_safe,
        "-ar",
        "16000",
        "-ac",
        "1",
        "-c:a",
        "pcm_s16le",
        &audio_path,
    ]);
    suppress_console(&mut cmd);
    let ffmpeg_result = cmd.output();

    match ffmpeg_result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!(
                    "ffmpeg failed (exit {}). ffmpeg={} input={}\nstderr: {}",
                    output.status.code().unwrap_or(-1),
                    ffmpeg_path,
                    video_path,
                    stderr.chars().rev().take(800).collect::<String>().chars().rev().collect::<String>()
                );
            }
        }
        Err(e) => bail!(
            "ffmpeg failed to launch: {} (path: {})",
            e, ffmpeg_path
        ),
    }

    if !Path::new(&audio_path).exists() {
        bail!(
            "ffmpeg ran but produced no audio file at {}",
            audio_path
        );
    }

    // --- 2. Run whisper on the extracted audio ---
    let output_stem = format!("{}/temp_audio", output_dir_safe);
    let mut cmd = Command::new(&whisper_safe);
    cmd.args([
        "-m",
        &model_safe,
        "-f",
        &audio_path,
        "-osrt",
        "-of",
        &output_stem,
    ]);
    suppress_console(&mut cmd);
    let result = cmd.output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let combined = format!("{}{}", stderr, stdout);
                bail!(
                    "whisper failed (exit {}). bin={} model={}\nlog: {}",
                    output.status.code().unwrap_or(-1),
                    whisper_binary,
                    model_path,
                    combined.chars().rev().take(800).collect::<String>().chars().rev().collect::<String>()
                );
            }
        }
        Err(e) => bail!(
            "whisper failed to launch: {} (path: {})",
            e, whisper_binary
        ),
    }

    // --- 3. Clean up temporary audio ---
    let _ = std::fs::remove_file(&audio_path);

    // The .srt is written next to the requested output stem. Return the
    // original (possibly UTF-8) directory path so the rest of the app
    // (frontend file fetching) sees the natural Unicode path.
    let srt_path = format!("{}/temp_audio.srt", output_dir);
    if !Path::new(&srt_path).exists() {
        bail!("whisper ran successfully but produced no .srt output");
    }

    Ok(srt_path)
}

/// Generate subtitles using the OpenAI Whisper API.
///
/// Extracts audio to MP3 via ffmpeg, then POSTs to the transcriptions endpoint
/// using curl (avoids adding reqwest as a dependency).
pub fn transcribe_api(
    video_path: &str,
    api_key: &str,
    output_dir: &str,
    ffmpeg_path: &str,
) -> Result<String> {
    // --- 1. Extract audio to MP3 ---
    let audio_path = format!("{}/temp_audio.mp3", output_dir);
    let mut cmd = Command::new(ffmpeg_path);
    cmd.args([
        "-y",
        "-i",
        video_path,
        "-ar",
        "16000",
        "-ac",
        "1",
        "-b:a",
        "64k",
        &audio_path,
    ]);
    suppress_console(&mut cmd);
    let ffmpeg_result = cmd.output();

    match ffmpeg_result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!(
                    "ffmpeg audio extraction failed: {}",
                    stderr.chars().take(500).collect::<String>()
                );
            }
        }
        Err(e) => bail!("ffmpeg not found or failed to launch: {}", e),
    }

    // --- 2. Call OpenAI transcriptions endpoint via curl ---
    let srt_path = format!("{}/whisper_output.srt", output_dir);
    let auth_header = format!("Authorization: Bearer {}", api_key);
    let file_arg = format!("file=@{}", audio_path);

    let result = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "https://api.openai.com/v1/audio/transcriptions",
            "-H",
            &auth_header,
            "-F",
            &file_arg,
            "-F",
            "model=whisper-1",
            "-F",
            "response_format=srt",
            "-o",
            &srt_path,
        ])
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                bail!("curl exited with non-zero status");
            }
        }
        Err(e) => bail!("curl not found or failed to launch: {}", e),
    }

    // --- 3. Clean up temporary audio ---
    let _ = std::fs::remove_file(&audio_path);

    if !Path::new(&srt_path).exists() {
        bail!("transcription API call produced no output file");
    }

    // Sanity-check: if the file looks like a JSON error from the API, surface it
    if let Ok(content) = std::fs::read_to_string(&srt_path) {
        if content.trim_start().starts_with('{') {
            let trimmed = content.chars().take(300).collect::<String>();
            bail!("OpenAI API returned an error: {}", trimmed);
        }
    }

    Ok(srt_path)
}

/// Translate an SRT file using OpenAI API
pub fn translate_srt(
    srt_path: &str,
    target_lang: &str,
    api_key: &str,
    output_dir: &str,
) -> Result<String> {
    let content = std::fs::read_to_string(srt_path)
        .map_err(|e| anyhow::anyhow!("failed to read SRT: {}", e))?;

    let output_path = format!("{}/translated_{}.srt", output_dir, target_lang);

    // Use curl to call OpenAI ChatGPT API for translation
    let prompt = format!(
        "Translate the following SRT subtitle content to {}. Keep the SRT format exactly (timestamps, numbering). Only output the translated SRT, nothing else.\n\n{}",
        target_lang, content
    );

    // Escape the prompt for JSON
    let escaped_prompt = prompt.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "");
    let body = format!(
        r#"{{"model":"gpt-4o-mini","messages":[{{"role":"user","content":"{}"}}],"temperature":0.3}}"#,
        escaped_prompt
    );

    let auth_header = format!("Authorization: Bearer {}", api_key);

    let result = Command::new("curl")
        .args([
            "-s", "-X", "POST",
            "https://api.openai.com/v1/chat/completions",
            "-H", &auth_header,
            "-H", "Content-Type: application/json",
            "-d", &body,
        ])
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                bail!("curl failed for translation");
            }
            let response = String::from_utf8_lossy(&output.stdout);
            // Parse JSON response to extract content
            let json: serde_json::Value = serde_json::from_str(&response)
                .map_err(|e| anyhow::anyhow!("failed to parse API response: {}", e))?;

            let translated = json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("no content in API response"))?;

            std::fs::write(&output_path, translated)
                .map_err(|e| anyhow::anyhow!("failed to write translated SRT: {}", e))?;
        }
        Err(e) => bail!("curl not found: {}", e),
    }

    Ok(output_path)
}
