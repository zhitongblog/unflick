//! Shared harness for the CLI/MCP integration tests.
//!
//! These tests drive the real binary against a real libmpv, because that is
//! where unflick's bugs actually live — the ones this suite was written for
//! (a global `pause` surviving `loadfile`, a resume point saved at EOF) are
//! invisible to unit tests and to `cargo check` alike.
//!
//! Three things keep a test run from colliding with the player the
//! developer is using at the time — and from colliding with each other,
//! since tests run concurrently:
//!
//!   * `UNFLICK_CONTROL_ADDR` — each `Daemon` binds its own port.
//!   * `UNFLICK_DATA_DIR` — each run gets a throwaway library database, so
//!     tests never write resume points into a real watch history.
//!   * `UNFLICK_CONFIG_DIR` — likewise for settings.json, which holds
//!     keybindings and subtitle styling. Without it the keybinding tests
//!     rewrite real preferences and race one another over the same file.
//!
//! Fixture media is generated once with the bundled ffmpeg and cached under
//! `target/test-fixtures/`, so a re-run costs nothing.

#![allow(dead_code)] // each test binary uses a different subset

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Ports are handed out sequentially from here. Well clear of the 19542
/// the real app uses, so a developer's running player is never touched.
static NEXT_PORT: AtomicU16 = AtomicU16::new(29542);

// ─── Fixture media ────────────────────────────────────────────────────────

pub struct Fixtures {
    /// 30s, 3 chapters ("Opening" / "Middle Part" / "Finale"), has audio.
    pub with_chapters: PathBuf,
    /// 60s, no chapters, with a sidecar .srt whose cues cluster into four
    /// groups separated by long pauses. "refund" appears in three cues.
    pub with_subtitles: PathBuf,
    /// 20s, no chapters, no subtitles. For "nothing to read" paths.
    pub plain: PathBuf,
    /// 5s of tone with title/artist/album tags and an embedded cover. The
    /// case music mode exists for — and the one where "has a video track"
    /// is true but "has video" must not be.
    pub audio: PathBuf,
}

pub fn fixtures() -> Fixtures {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).expect("create fixture dir");

    let with_chapters = dir.join("chapters.mkv");
    let with_subtitles = dir.join("subtitled.mp4");
    let subtitle_file = dir.join("subtitled.srt");
    let plain = dir.join("plain.mp4");

    if !with_chapters.exists() {
        build_chapter_fixture(&dir, &with_chapters);
    }
    if !plain.exists() {
        encode(&["-f", "lavfi", "-i", "testsrc2=duration=20:size=320x240:rate=25"], &plain);
    }
    if !with_subtitles.exists() || !subtitle_file.exists() {
        encode(
            &[
                "-f", "lavfi", "-i", "testsrc=duration=60:size=320x240:rate=25",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=60",
                "-shortest",
            ],
            &with_subtitles,
        );
        std::fs::write(&subtitle_file, SUBTITLE_FIXTURE).expect("write fixture srt");
    }

    let audio = dir.join("tagged.m4a");
    if !audio.exists() {
        build_audio_fixture(&dir, &audio);
    }

    Fixtures { with_chapters, with_subtitles, plain, audio }
}

/// Tone plus tags plus a cover picture, muxed as an attached picture the way
/// a real music file carries one.
fn build_audio_fixture(dir: &Path, out: &Path) {
    let cover = dir.join("cover.jpg");
    run_ffmpeg(&[
        "-y", "-loglevel", "error",
        "-f", "lavfi", "-i", "color=c=orange:s=240x240:d=1",
        "-frames:v", "1",
        &cover.to_string_lossy(),
    ]);

    run_ffmpeg(&[
        "-y", "-loglevel", "error",
        "-f", "lavfi", "-i", "sine=frequency=440:duration=5",
        "-i", &cover.to_string_lossy(),
        "-map", "0:a", "-map", "1:v",
        "-c:a", "aac", "-c:v", "mjpeg",
        "-disposition:v:0", "attached_pic",
        "-metadata", "title=Test Track",
        "-metadata", "artist=Fixture Ensemble",
        "-metadata", "album=Integration Suite",
        &out.to_string_lossy(),
    ]);
    assert!(out.exists(), "ffmpeg produced no audio fixture");
}

/// Four groups of three cues, separated by long silences so chapter
/// derivation has real pauses to find. "refund" is in cues 4, 5 and 10.
const SUBTITLE_FIXTURE: &str = "\
1
00:00:00,000 --> 00:00:02,500
Welcome to the show

2
00:00:03,000 --> 00:00:05,500
Today we cover three topics

3
00:00:06,000 --> 00:00:08,500
Let us begin

4
00:00:18,000 --> 00:00:20,500
First up is the refund policy

5
00:00:21,000 --> 00:00:23,500
You can request a refund within 30 days

6
00:00:24,000 --> 00:00:26,500
No questions asked

7
00:00:34,000 --> 00:00:36,500
Next we discuss shipping

8
00:00:37,000 --> 00:00:39,500
Orders ship within two business days

9
00:00:40,000 --> 00:00:42,500
Tracking is included

10
00:00:48,000 --> 00:00:50,500
Finally the refund policy again

11
00:00:51,000 --> 00:00:53,500
That is all for today

12
00:00:54,000 --> 00:00:56,500
Thanks for watching
";

fn build_chapter_fixture(dir: &Path, out: &Path) {
    let base = dir.join("chapters-base.mkv");
    encode(
        &[
            "-f", "lavfi", "-i", "testsrc=duration=30:size=320x240:rate=25",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=30",
            "-shortest",
        ],
        &base,
    );

    let meta = dir.join("chapters.txt");
    std::fs::write(
        &meta,
        ";FFMETADATA1\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=10000\ntitle=Opening\n\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=10000\nEND=20000\ntitle=Middle Part\n\n\
         [CHAPTER]\nTIMEBASE=1/1000\nSTART=20000\nEND=30000\ntitle=Finale\n",
    )
    .expect("write chapter metadata");

    run_ffmpeg(&[
        "-y", "-loglevel", "error",
        "-i", &base.to_string_lossy(),
        "-i", &meta.to_string_lossy(),
        "-map_metadata", "1", "-c", "copy",
        &out.to_string_lossy(),
    ]);
}

fn encode(input_args: &[&str], out: &Path) {
    let mut args = vec!["-y", "-loglevel", "error"];
    args.extend_from_slice(input_args);
    args.extend_from_slice(&["-c:v", "libx264", "-preset", "ultrafast"]);
    let out_str = out.to_string_lossy().into_owned();
    args.push(&out_str);
    run_ffmpeg(&args);
    assert!(out.exists(), "ffmpeg produced no output at {}", out.display());
}

fn run_ffmpeg(args: &[&str]) {
    let ffmpeg = find_ffmpeg().unwrap_or_else(|| {
        panic!(
            "ffmpeg not found. The integration tests build their fixtures with it. \
             Expected it at <repo>/src-tauri/ffmpeg/ffmpeg.exe or on PATH."
        )
    });
    let out = Command::new(&ffmpeg)
        .args(args)
        .output()
        .expect("failed to run ffmpeg");
    assert!(
        out.status.success(),
        "ffmpeg failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn find_ffmpeg() -> Option<PathBuf> {
    let bundled = manifest_dir()
        .join("ffmpeg")
        .join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
    if bundled.exists() {
        return Some(bundled);
    }
    which::which(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }).ok()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_dir() -> PathBuf {
    // `target/` sits next to the manifest; keeping fixtures there means
    // `cargo clean` disposes of them and they never land in git.
    manifest_dir().join("target").join("test-fixtures")
}

// ─── Daemon under test ────────────────────────────────────────────────────

/// A headless unflick daemon on a private port with a private database.
/// Dropping it kills the process, so a failing assertion can't leave one
/// behind.
pub struct Daemon {
    child: Child,
    addr: String,
    data_dir: PathBuf,
    /// Set while handing the data dir over to a replacement process; see
    /// `restart`.
    keep_data_on_drop: bool,
}

impl Daemon {
    pub fn start() -> Self {
        let port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        let addr = format!("127.0.0.1:{}", port);
        assert!(
            TcpListener::bind(&addr).is_ok(),
            "test port {} is already in use",
            addr
        );

        let data_dir = fixture_dir().join(format!("data-{}", port));
        let _ = std::fs::remove_dir_all(&data_dir);
        std::fs::create_dir_all(&data_dir).expect("create test data dir");

        let child = Command::new(env!("CARGO_BIN_EXE_unflick"))
            .arg("daemon")
            .env(unflick_lib::core::daemon::CONTROL_ADDR_ENV, &addr)
            .env(unflick_lib::db::DATA_DIR_ENV, &data_dir)
            // settings.json holds keybindings and subtitle styling. Without
            // its own copy, tests would rewrite the developer's real
            // preferences and race each other over the same file.
            .env(unflick_lib::core::settings::CONFIG_DIR_ENV, &data_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn unflick daemon");

        let daemon = Self {
            child,
            addr,
            data_dir,
            keep_data_on_drop: false,
        };
        daemon.wait_until_listening();
        daemon
    }

    fn wait_until_listening(&self) {
        // Generous on purpose. Starting a daemon means loading libmpv and
        // initialising it, and these tests routinely run while rustc is
        // saturating the machine compiling the next test binary. A tight
        // deadline here buys nothing when the suite passes and produces an
        // unreproducible failure when it doesn't.
        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline {
            if TcpStream::connect(&self.addr).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "daemon did not start listening on {}. If libmpv is missing this is \
             where it shows up — the daemon exits immediately when it can't load it.",
            self.addr
        );
    }

    /// Send a command and return the parsed result. Panics on transport
    /// failure; a command that legitimately fails still returns a value with
    /// `success: false`, which is what the negative tests assert on.
    pub fn send(&self, command: &str, args: Value) -> Reply {
        let stream = TcpStream::connect(&self.addr).expect("connect to test daemon");
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .expect("set read timeout");
        let mut writer = stream.try_clone().expect("clone stream");
        let mut reader = BufReader::new(stream);

        writeln!(writer, "{}", json!({ "command": command, "args": args }))
            .expect("write command");

        let mut line = String::new();
        reader.read_line(&mut line).expect("read reply");
        let value: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("daemon returned invalid JSON: {e}: {line}"));
        Reply { command: command.to_string(), value }
    }

    /// Load a file and wait for playback to actually be underway. `play`
    /// returns as soon as mpv accepts `loadfile`; asserting on duration or
    /// chapters before the file is open is the main source of flakiness in
    /// a suite like this.
    pub fn play(&self, path: &Path) -> Reply {
        let reply = self.send("play", json!({ "file": path.to_string_lossy() }));
        reply.expect_ok();
        self.wait_for(|d| d.status()["duration"].as_f64().unwrap_or(0.0) > 0.0, "file to load");
        reply
    }

    pub fn status(&self) -> Value {
        self.send("status", json!({})).data()
    }

    pub fn position(&self) -> f64 {
        self.status()["position"].as_f64().unwrap_or(0.0)
    }

    /// Poll until `check` passes. Every wait in the suite goes through here
    /// rather than a bare sleep, so slow machines don't produce flakes.
    pub fn wait_for<F>(&self, check: F, what: &str)
    where
        F: Fn(&Daemon) -> bool,
    {
        // Same reasoning as `wait_until_listening`: the deadline is a
        // backstop against a hang, not a performance assertion.
        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline {
            if check(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("timed out waiting for {what}");
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
}

impl Daemon {
    /// Stop this daemon and start a fresh one on the same port, config dir
    /// and data dir.
    ///
    /// What a persistence test actually needs: "does this survive a restart"
    /// is only answered by a genuinely new process re-reading the same files.
    /// Consumes `self` so the old handle can't be used afterwards - its
    /// process is gone and its port belongs to the new one.
    pub fn restart(mut self) -> Self {
        let addr = self.addr.clone();
        let data_dir = self.data_dir.clone();

        let _ = self.child.kill();
        let _ = self.child.wait();
        // Drop would delete the data dir, which is the thing under test.
        self.keep_data_on_drop = true;

        let child = Command::new(env!("CARGO_BIN_EXE_unflick"))
            .arg("daemon")
            .env(unflick_lib::core::daemon::CONTROL_ADDR_ENV, &addr)
            .env(unflick_lib::db::DATA_DIR_ENV, &data_dir)
            .env(unflick_lib::core::settings::CONFIG_DIR_ENV, &data_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to respawn unflick daemon");

        let daemon = Self {
            child,
            addr,
            data_dir,
            keep_data_on_drop: false,
        };
        daemon.wait_until_listening();
        daemon
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep_data_on_drop {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}

/// One reply from the control server, with assertions that report the
/// command and the server's own message when they fail.
pub struct Reply {
    command: String,
    value: Value,
}

impl Reply {
    pub fn success(&self) -> bool {
        self.value["success"].as_bool().unwrap_or(false)
    }

    pub fn message(&self) -> &str {
        self.value["message"].as_str().unwrap_or("")
    }

    pub fn data(&self) -> Value {
        self.value.get("data").cloned().unwrap_or(Value::Null)
    }

    pub fn expect_ok(&self) -> &Self {
        assert!(
            self.success(),
            "`{}` failed: {}",
            self.command,
            self.message()
        );
        self
    }

    /// Assert the command failed, and that the message explains why in
    /// terms the user can act on.
    pub fn expect_err_containing(&self, needle: &str) -> &Self {
        assert!(
            !self.success(),
            "`{}` unexpectedly succeeded: {}",
            self.command,
            self.value
        );
        let msg = self.message().to_lowercase();
        assert!(
            msg.contains(&needle.to_lowercase()),
            "`{}` failed as expected but the message doesn't mention {:?}: {}",
            self.command,
            needle,
            self.message()
        );
        self
    }
}

// ─── MCP ──────────────────────────────────────────────────────────────────

/// Run a batch of JSON-RPC requests through `unflick --mcp` and return the
/// replies keyed by id. The MCP server is stdio, one process per exchange.
pub fn mcp_roundtrip(requests: &[Value], addr_from: &Daemon) -> std::collections::HashMap<i64, Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_unflick"))
        .arg("--mcp")
        .env(unflick_lib::core::daemon::CONTROL_ADDR_ENV, &addr_from.addr)
        .env(unflick_lib::db::DATA_DIR_ENV, addr_from.data_dir())
        .env(unflick_lib::core::settings::CONFIG_DIR_ENV, addr_from.data_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp server");

    {
        let stdin = child.stdin.as_mut().expect("mcp stdin");
        let init = json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "integration-test", "version": "1" }
            }
        });
        writeln!(stdin, "{init}").expect("write initialize");
        for req in requests {
            writeln!(stdin, "{req}").expect("write request");
        }
    }

    let output = child.wait_with_output().expect("mcp server exit");
    let mut replies = std::collections::HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if let Some(id) = v.get("id").and_then(|i| i.as_i64()) {
                replies.insert(id, v);
            }
        }
    }
    replies
}
