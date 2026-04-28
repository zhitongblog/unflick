use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

/// Generate subtitles using local whisper.cpp CLI.
///
/// Extracts audio via ffmpeg, runs the whisper binary, returns the .srt path.
pub fn transcribe_local(
    video_path: &str,
    whisper_binary: &str,
    model_path: &str,
    output_dir: &str,
) -> Result<String> {
    // --- 1. Extract audio to WAV via ffmpeg ---
    let audio_path = format!("{}/temp_audio.wav", output_dir);
    let ffmpeg_result = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            video_path,
            "-ar",
            "16000",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            &audio_path,
        ])
        .output();

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

    // --- 2. Run whisper on the extracted audio ---
    // whisper.cpp CLI writes <output_stem>.srt when -osrt is passed.
    // We pass "-of <output_dir>/temp_audio" so the output base is fixed.
    let output_stem = format!("{}/temp_audio", output_dir);
    let result = Command::new(whisper_binary)
        .args([
            "-m",
            model_path,
            "-f",
            &audio_path,
            "-osrt",
            "-of",
            &output_stem,
        ])
        .output();

    match result {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!(
                    "whisper failed: {}",
                    stderr.chars().take(500).collect::<String>()
                );
            }
        }
        Err(e) => bail!("whisper binary not found or failed to launch: {}", e),
    }

    // --- 3. Clean up temporary audio ---
    let _ = std::fs::remove_file(&audio_path);

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
) -> Result<String> {
    // --- 1. Extract audio to MP3 ---
    let audio_path = format!("{}/temp_audio.mp3", output_dir);
    let ffmpeg_result = Command::new("ffmpeg")
        .args([
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
        ])
        .output();

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
