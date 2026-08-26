pub mod core;
pub mod db;
pub mod mpv;
pub mod cli;
pub mod mcp;
pub mod gui;
pub mod video;

use std::sync::Arc;

use core::i18n::{menu_strings, read_locale_from_settings};
use core::player::Player;
use core::render_loop::RenderLoop;
use gui::{commands, state::{GuiPlayer, PendingFile}};
use video::VideoSurface;
use tauri::menu::{Menu, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST come first so a second-launch is short-
        // circuited before any other plugin spends time on initialization.
        // The callback runs inside the *first* (already-running) process —
        // we focus its window and forward the second launch's file argument
        // (if any) to the frontend so it plays in the existing window.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::{Emitter, Manager};

            // Bring the existing window forward.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();

                // Forward a "open this file" arg, mirroring main.rs's logic.
                let file = args
                    .iter()
                    .skip(1)
                    .find(|a| !a.starts_with('-') && std::path::Path::new(a).is_file())
                    .cloned();
                if let Some(path) = file {
                    let _ = window.emit("open-file", path);
                }
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(GuiPlayer::new())
        .manage(PendingFile::from_env())
        .setup(|app| {
            let handle = app.handle();

            // Pull the user's saved locale once, here at startup. Tauri's
            // native menu can't re-render its labels at runtime, so
            // switching languages requires a restart (the Settings panel
            // tells the user this explicitly).
            let m = menu_strings(&read_locale_from_settings());

            // Build File menu
            let open_item = MenuItemBuilder::with_id("open", m.open_file)
                .accelerator("CmdOrCtrl+O")
                .build(handle)?;
            let open_url_item = MenuItemBuilder::with_id("open_url", m.open_url)
                .accelerator("CmdOrCtrl+U")
                .build(handle)?;
            let sep1 = PredefinedMenuItem::separator(handle)?;
            let quit_item = MenuItemBuilder::with_id("quit", m.quit)
                .accelerator("CmdOrCtrl+Q")
                .build(handle)?;
            let file_menu = SubmenuBuilder::new(handle, m.file)
                .items(&[&open_item, &open_url_item, &sep1, &quit_item])
                .build()?;

            // Build Playback menu
            let play_pause_item = MenuItemBuilder::with_id("play_pause", m.play_pause)
                .build(handle)?;
            let stop_item = MenuItemBuilder::with_id("stop", m.stop)
                .build(handle)?;
            let sep2 = PredefinedMenuItem::separator(handle)?;
            let vol_up_item = MenuItemBuilder::with_id("volume_up", m.volume_up)
                .build(handle)?;
            let vol_down_item = MenuItemBuilder::with_id("volume_down", m.volume_down)
                .build(handle)?;
            let playback_menu = SubmenuBuilder::new(handle, m.playback)
                .items(&[&play_pause_item, &stop_item, &sep2, &vol_up_item, &vol_down_item])
                .build()?;

            // Build View menu
            let fullscreen_item = MenuItemBuilder::with_id("fullscreen", m.fullscreen)
                .accelerator("F11")
                .build(handle)?;
            let pip_item = MenuItemBuilder::with_id("pip", m.pip)
                .build(handle)?;
            let library_item = MenuItemBuilder::with_id("library", m.library)
                .build(handle)?;
            let view_menu = SubmenuBuilder::new(handle, m.view)
                .items(&[&fullscreen_item, &pip_item, &library_item])
                .build()?;

            // Build Help menu
            let about_item = MenuItemBuilder::with_id("about", m.about)
                .build(handle)?;
            let check_updates_item = MenuItemBuilder::with_id("check_updates", m.check_updates)
                .build(handle)?;
            let sep_help = PredefinedMenuItem::separator(handle)?;
            let help_menu = SubmenuBuilder::new(handle, m.help)
                .items(&[&check_updates_item, &sep_help, &about_item])
                .build()?;

            let menu = Menu::with_items(handle, &[&file_menu, &playback_menu, &view_menu, &help_menu])?;
            app.set_menu(menu)?;

            // ── v0.8 video pipeline bring-up ───────────────────────────────
            // Build the embedded video surface beneath the WebView, spin up
            // a render thread that drives mpv → GL, and stash both in the
            // GuiPlayer state.
            //
            // Order is platform-specific:
            //   - Windows / macOS: HWND / NSView is valid as soon as the
            //     window is created in setup, even while still hidden, so
            //     we bring up the pipeline first and *then* show — that
            //     way the first frame the user sees is the final config
            //     (no white flash from native chrome on Windows, no
            //     pre-pipeline gap on macOS).
            //   - Linux (GTK): the underlying GdkWindow's X11 XID isn't
            //     allocated until the widget is realized, which happens
            //     on show(). Calling raw_window_handle before show() would
            //     return Unavailable and the X11 child window we need for
            //     mpv's GL context can't be created. So show first, then
            //     bring up the pipeline. The brief moment between show
            //     and the first rendered frame is harmless — the WebView
            //     paints its dark background underneath while mpv warms.
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                eprintln!("[unflick] bringing up video pipeline...");
                match bring_up_video_pipeline(&window, app.state::<GuiPlayer>()) {
                    Ok(()) => eprintln!("[unflick] video pipeline ready"),
                    Err(e) => eprintln!("[unflick] video pipeline init failed: {e}"),
                }
            }
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                eprintln!("[unflick] bringing up video pipeline...");
                match bring_up_video_pipeline(&window, app.state::<GuiPlayer>()) {
                    Ok(()) => eprintln!("[unflick] video pipeline ready"),
                    Err(e) => eprintln!("[unflick] video pipeline init failed: {e}"),
                }
                // Punch a hole through the WKWebView so the NSView with mpv
                // video (inserted *below* it via addSubview:positioned:Below)
                // is actually visible. The white-flash fix in index.html keeps
                // html/body opaque on Windows; main.tsx undoes that on macOS so
                // the page is transparent over the videoRegion. But the
                // WebView *itself* still defaults to opaque — without this
                // call, even a fully-transparent page paints over the
                // NSView. setOpaque:NO + drawsBackground=NO together make
                // WKWebView a true overlay.
                let _ = window.with_webview(|webview| unsafe {
                    use objc2::{class, msg_send, runtime::AnyObject};
                    use objc2_foundation::NSString;
                    let wk: *mut AnyObject = webview.inner() as *mut AnyObject;
                    if wk.is_null() {
                        return;
                    }
                    let _: () = msg_send![wk, setOpaque: false];
                    let key = NSString::from_str("drawsBackground");
                    let no_num: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: false];
                    let _: () = msg_send![wk, setValue: no_num, forKey: &*key];
                });
            }

            // Decorations: Windows wants `false` so our React TitleBar
            // renders alone (native Win11 chrome doesn't blend with our
            // dark theme). macOS keeps `decorations: true` from
            // tauri.conf.json so AppKit shows the native traffic-light
            // buttons via `titleBarStyle: "Overlay"`.
            //
            // Then show the window. tauri.conf.json sets `visible: false`
            // to avoid the white-flash where the user briefly sees the
            // native chrome before set_decorations + the WebView paints.
            // Showing here, after the chrome flip + the pipeline init,
            // means the first frame the user sees is the final
            // configuration (dark window with our React TitleBar on
            // Windows, dark window with overlaid traffic lights on mac).
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.set_decorations(false) {
                    eprintln!("[unflick] set_decorations(false) failed: {e}");
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = window.show() {
                    eprintln!("[unflick] window.show() failed: {e}");
                }
            }

            // Linux pipeline bring-up runs *after* show() so the GTK
            // widget is realized — see comment above. The X11 XID isn't
            // allocated until gtk realize, which fires asynchronously
            // on the GTK main loop. The setup hook runs *before* the
            // main loop starts, so a plain sleep-loop never lets the
            // realize event fire. Drive gtk_main_iteration_do(false)
            // ourselves between checks so the pending events drain.
            #[cfg(target_os = "linux")]
            if let Some(window) = app.get_webview_window("main") {
                eprintln!("[unflick] bringing up video pipeline...");
                let mut last_err = String::new();
                let mut ok = false;
                for attempt in 0..50 {
                    // Pump GTK events so gtk_widget_realize has a chance
                    // to actually fire after our show() call. main_iteration_do
                    // with `blocking=false` returns immediately if there's
                    // nothing to process — no risk of deadlock.
                    while gtk::events_pending() {
                        gtk::main_iteration_do(false);
                    }
                    match bring_up_video_pipeline(&window, app.state::<GuiPlayer>()) {
                        Ok(()) => {
                            eprintln!(
                                "[unflick] video pipeline ready (attempt {})",
                                attempt + 1
                            );
                            ok = true;
                            break;
                        }
                        Err(e) => {
                            last_err = e;
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                    }
                }
                if !ok {
                    eprintln!("[unflick] video pipeline init failed: {last_err}");
                }
            }

            // ── Windows file-association registration ──────────────────────
            // Tauri's NSIS bundling only writes basic OpenWithProgIDs entries.
            // We additionally register unflick under HKCU\Software\
            // RegisteredApplications so it appears in Settings → Default apps
            // for the user. Idempotent, runs every launch.
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = core::win_assoc::register_default_program() {
                    eprintln!("[unflick] file-assoc registration failed: {e}");
                }
            }

            // Linux equivalent — runs `xdg-mime default unflick.desktop
            // video/mp4` (and friends) so unflick becomes the default
            // for double-clicked video/audio files in nautilus / etc.
            // Best-effort: skipped silently on systems without xdg-utils.
            #[cfg(target_os = "linux")]
            {
                match core::linux_assoc::register_default_program() {
                    Ok(n) => eprintln!("[unflick] file-assoc: registered {n} MIME types"),
                    Err(e) => eprintln!("[unflick] file-assoc registration failed: {e}"),
                }
            }

            // The one owner of the window's shape. Managed here rather than
            // built in `new()` because it needs the AppHandle, and shared
            // with the control server so `unflick window mode music` reaches
            // the same window the buttons do.
            let window_host = Arc::new(gui::window::TauriWindowHost::new(app.handle().clone()));
            app.manage(Arc::clone(&window_host));

            // ── Embedded control server ────────────────────────────────────
            // Host the same TCP command surface the headless daemon serves,
            // but against *this* window's player. Without it, `unflick pause`
            // and MCP `pause` would spawn (or talk to) a separate vo=null mpv
            // the user can't see, while the video on screen kept playing.
            spawn_embedded_control_server(app.state::<GuiPlayer>(), window_host);

            // Timeline previews accumulate on disk as people watch things.
            // Trim once per launch, off the startup path — it walks the
            // cache directory and there's no reason to make the window
            // wait for it.
            std::thread::spawn(core::thumbnail::prune_cache);

            Ok(())
        })
        .on_menu_event(|app, event| {
            let window = app.get_webview_window("main");
            match event.id().as_ref() {
                "quit" => {
                    app.exit(0);
                }
                "open" | "open_url" | "play_pause" | "stop" | "fullscreen" | "pip" | "library"
                | "volume_up" | "volume_down" | "about" | "check_updates" => {
                    if let Some(win) = &window {
                        let id: String = event.id().as_ref().to_string();
                        let _ = win.emit("menu-event", id);
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::player_init,
            commands::consume_pending_file,
            commands::open_default_apps_settings,
            commands::video_surface_set_geometry,
            commands::video_surface_set_visible,
            commands::video_surface_set_alpha,
            #[cfg(target_os = "windows")]
            commands::show_native_context_menu,
            commands::set_always_on_top,
            commands::player_play,
            commands::player_pause,
            commands::player_resume,
            commands::player_stop,
            commands::player_seek,
            commands::player_set_volume,
            commands::player_set_speed,
            commands::player_status,
            commands::player_screenshot,
            commands::toggle_pip,
            commands::window_mode,
            commands::toggle_music_mode,
            commands::now_playing,
            commands::set_fullscreen,
            commands::exit_fullscreen,
            commands::open_file_dialog,
            commands::open_files_dialog,
            commands::open_folder_dialog,
            // Library
            commands::library_list,
            commands::library_search,
            commands::library_scan,
            commands::library_clear,
            // Subtitles
            commands::subtitle_load,
            commands::opensubtitles_configured,
            commands::equalizer_get,
            commands::equalizer_set,
            commands::equalizer_preset,
            commands::equalizer_presets,
            commands::equalizer_reset,
            commands::pitch_correction,
            commands::settings_set_key,
            commands::subtitle_search_online,
            commands::subtitle_download_online,
            commands::subtitle_auto_online,
            commands::subtitle_list,
            commands::subtitle_select,
            // Playlist
            commands::playlist_add,
            commands::playlist_list,
            commands::playlist_next,
            commands::playlist_prev,
            commands::playlist_remove,
            commands::playlist_play_index,
            commands::playlist_clear,
            commands::open_subtitle_dialog,
            // Clip extraction
            commands::extract_clip,
            commands::save_file_dialog,
            commands::write_file_bytes,
            commands::read_text_file,
            commands::find_sidecar_subtitles,
            commands::check_yt_dlp,
            commands::extract_stream_url,
            commands::cancel_url_extraction,
            commands::arm_post_play_hooks,
            commands::yt_dlp_info,
            commands::update_yt_dlp,
            commands::get_system_proxy,
            // Audio tracks
            commands::audio_list,
            commands::audio_select,
            // Playback position / history
            commands::save_position,
            commands::get_position,
            commands::clear_position,
            commands::record_play,
            // AI subtitle generation
            commands::generate_subtitles,
            commands::translate_subtitles,
            // Settings persistence
            commands::save_settings,
            commands::load_settings,
            commands::check_bundled_whisper,
            commands::check_for_updates,
            // Video filters
            commands::set_video_filter,
            commands::get_video_filters,
            commands::reset_video_filters,
            // Timing / chapters / A-B loop / frame stepping / playlist modes
            commands::subtitle_delay,
            commands::audio_delay,
            commands::subtitle_style_get,
            commands::subtitle_style_set,
            commands::chapter_list,
            commands::chapter_seek,
            commands::chapter_step,
            commands::ab_loop,
            commands::frame_step,
            commands::playlist_repeat,
            commands::playlist_shuffle,
            // Timeline previews
            commands::thumbnail_at,
            // Keyboard bindings
            commands::keybind_list,
            commands::keybind_set,
            commands::keybind_reset,
            commands::mouse_list,
            commands::mouse_set,
            commands::mouse_reset,
            // Recently played
            // Picture geometry
            commands::video_transform_get,
            commands::video_transform_set,
            commands::video_transform_reset,
            commands::set_incognito,
            commands::recent_list,
            commands::recent_clear,
            // Bookmarks
            commands::bookmark_add,
            commands::bookmark_list,
            commands::bookmark_rename,
            commands::bookmark_remove,
            commands::bookmark_clear,
        ])
        .on_window_event(|window, event| {
            // The video popup is a top-level WS_POPUP owned by this window.
            // Owner relationship gives us z-order tracking *within* an
            // app's window stack, but Windows still keeps the popup above
            // every other app while *this* app is the foreground process.
            // When the user alt-tabs to a different app, Win32 doesn't
            // automatically dismiss the popup — and because the popup is
            // a layered window with mpv painting at full opacity, the
            // other app gets occluded. We compensate by hiding the popup
            // on focus loss and re-showing it when focus returns.
            //
            // Geometry tracking on Move/Resize stays the same: re-apply
            // the cached client rect so the popup follows the owner.
            match event {
                tauri::WindowEvent::Moved(_) => {
                    if let Some(gp) = window.try_state::<GuiPlayer>() {
                        if let Some(rl) = gp.render_loop.get() {
                            let _ = rl.refresh_geometry();
                        }
                    }
                }
                tauri::WindowEvent::Resized(size) => {
                    if let Some(gp) = window.try_state::<GuiPlayer>() {
                        if let Some(rl) = gp.render_loop.get() {
                            // Tauri reports a (0, 0) inner size when the
                            // main window is minimized. The popup needs
                            // to vanish in that case — otherwise it stays
                            // floating on screen at its old screen-coords,
                            // covering whatever is now beneath. Frontend
                            // can't detect this either: ResizeObserver
                            // doesn't fire on minimize because the
                            // WebView itself stops painting.
                            //
                            // On restore (size goes from 0 back to a real
                            // value) the frontend's ResizeObserver fires
                            // again and re-pushes geometry, but it does
                            // *not* re-push visibility — it sees its own
                            // lastVisibleRef as already-true and skips.
                            // We emit a "main-restored" event so the
                            // frontend re-asserts visibility from its
                            // desired-state ref, bypassing the cache.
                            if size.width == 0 || size.height == 0 {
                                rl.set_visible(false);
                            } else {
                                let _ = rl.refresh_geometry();
                                let _ = window.emit("main-restored", ());
                            }
                        }
                    }
                }
                // Focus-based hiding has been removed entirely. The
                // earlier implementation hid the popup whenever Tauri
                // fired Focused(false), which triggered on every transient
                // focus blip on Win11 (notification toasts, IME panels,
                // hover-activated taskbar previews). The resulting
                // show/hide cycle was the user-visible "flicker." A 300 ms
                // debounce wasn't enough — some Win11 focus blips outlast
                // it. The popup is owned by the main window, so Win32's
                // owner z-order handles alt-tabs in normal cases. For the
                // "always-on-top" case the user explicitly asked for, the
                // popup *should* stay above other apps — that's the whole
                // feature.
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS Finder "Open With → unflick" routes through an
            // NSAppleEventDescriptor (kAEOpenDocuments), not argv. Tauri
            // surfaces it here as RunEvent::Opened. Forward each url's
            // file path to the frontend's existing "open-file" listener
            // (App.tsx, mirrors the single-instance plugin's path).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                    for url in urls {
                        if let Ok(path) = url.to_file_path() {
                            let path_str = path.to_string_lossy().into_owned();
                            let _ = window.emit("open-file", path_str);
                        }
                    }
                }
            }
            let _ = (app_handle, event);
        });
}

/// Host the CLI/MCP control protocol inside the GUI process, bound to the
/// on-screen render player.
///
/// unflick has always shipped a headless daemon for `unflick play` and the
/// MCP server. That daemon builds its own `vo=null` mpv, which is invisible
/// by construction — so an AI agent driving unflick got audio and a status
/// JSON, but never the picture, and never the window the user was looking
/// at. Hosting the same server here makes the three interfaces share one
/// player instead of merely one codebase.
///
/// The GUI wins ties: any headless daemon already on the port is asked to
/// exit first. If it declines (another GUI holds it) or the bind still
/// fails, we log and carry on — the window stays fully usable, it just
/// isn't the one answering CLI commands.
fn spawn_embedded_control_server(
    state: tauri::State<'_, GuiPlayer>,
    window_host: Arc<gui::window::TauriWindowHost>,
) {
    let Some(player) = state.render_player.get().cloned() else {
        eprintln!("[unflick] control server: no render player, skipping");
        return;
    };
    let playlist = Arc::clone(&state.playlist);
    let incognito = Arc::clone(&state.incognito);

    // A second SQLite connection to the same library.db. The GUI's own
    // handle stays in GuiPlayer; rusqlite serialises writes per connection
    // and our write volume (position saves, scan upserts) is far below
    // anything that would contend.
    let db = match db::Database::open() {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("[unflick] control server: database open failed: {e}");
            return;
        }
    };

    std::thread::spawn(move || {
        if !core::daemon::request_port_handover() {
            eprintln!("[unflick] control server: port busy, CLI/MCP will use the existing host");
            return;
        }
        let ctx = Arc::new(core::daemon::ControlContext {
            player,
            playlist,
            db,
            embedded: true,
            incognito,
            window: Some(window_host),
        });
        if let Err(e) = core::daemon::serve_control(ctx) {
            eprintln!("[unflick] control server: bind failed: {e}");
        }
    });
}

/// Bring up the v0.8 video pipeline on the main window.
///
/// Order matters: surface must be created before the render thread starts
/// (the thread immediately calls `make_current` on it), and the player needs
/// to exist before render-context construction (the context binds to mpv).
/// We tolerate any error here — the GUI will simply fall back to the
/// HTML5 path that's still wired up. The surface starts hidden.
// TODO(v0.9 follow-up): wire `core::url_post_play::after_play_url_hooks` into
// the daemon's `play` arm (`core::daemon.rs`) so SponsorBlock skip + auto-
// subtitle download fire whenever a URL is played via CLI / MCP. Today the
// hook is callable but unwired:
//   - GUI path (`gui::commands::player_play`) has access to `Arc<Player>` via
//     `GuiPlayer::render_player` and could call the hook directly.
//   - CLI/MCP path (`core::daemon::dispatch_command`) currently takes
//     `&Player`; needs a small refactor to pass `Arc<Player>` so the hook
//     can store handles on the live player instance for the polling task
//     spun up below in `bring_up_video_pipeline` to consume.
// Function signature (in `core::url_post_play`):
//   `Arc<Player>`, `String` (page URL), `Option<PathBuf>` (yt-dlp),
//   `UrlPostPlaySettings`.
#[cfg(target_os = "windows")]
fn bring_up_video_pipeline(
    window: &tauri::WebviewWindow,
    state: tauri::State<'_, GuiPlayer>,
) -> Result<(), String> {
    use windows_sys::Win32::Foundation::HWND;

    // Tauri's `hwnd()` returns the `windows` crate's HWND newtype; our
    // video surface wants windows-sys's HWND alias (`*mut c_void`). Both
    // wrap the same kernel handle — extract the raw pointer through the
    // newtype's `.0` field.
    let tauri_hwnd = window
        .hwnd()
        .map_err(|e| format!("get_hwnd: {e}"))?;
    let hwnd: HWND = tauri_hwnd.0 as HWND;
    let size = window
        .inner_size()
        .map_err(|e| format!("inner_size: {e}"))?;

    // Hidden child window beneath WebView. Geometry is bogus until the
    // frontend calls video_surface_set_geometry once it's mounted.
    let surface = video::windows::WindowsVideoSurface::new(
        hwnd,
        size.width as i32,
        size.height as i32,
    )
    .map_err(|e| format!("create video surface: {e}"))?;
    let surface: Arc<dyn VideoSurface> = Arc::new(surface);

    let player = Arc::new(Player::new_for_render().map_err(|e| format!("create render player: {e}"))?);

    let render_loop = RenderLoop::start(Arc::clone(&player), Arc::clone(&surface))
        .map_err(|e| format!("start render loop: {e}"))?;

    // SponsorBlock auto-skip polling task. Cheap (250 ms tick, just reads
    // a property and runs an in-memory check) so it stays running for the
    // process lifetime regardless of whether the user ever plays a URL.
    core::url_post_play::spawn_sponsor_skip_task(Arc::clone(&player));

    state
        .render_player
        .set(player)
        .map_err(|_| "render_player already initialised".to_string())?;
    state
        .render_loop
        .set(render_loop)
        .map_err(|_| "render_loop already initialised".to_string())?;

    Ok(())
}

/// macOS counterpart to the Windows pipeline bring-up. Same shape:
/// surface → mpv player → render loop, all stashed in GuiPlayer state.
/// The architecture differs in that the surface is a *subview* of the
/// Tauri main window's contentView (not a top-level popup), so there's no
/// owner / z-order plumbing — the WKWebView naturally sits on top.
#[cfg(target_os = "macos")]
fn bring_up_video_pipeline(
    window: &tauri::WebviewWindow,
    state: tauri::State<'_, GuiPlayer>,
) -> Result<(), String> {
    let ns_view_ptr = window.ns_view().map_err(|e| format!("ns_view: {e}"))?;
    let size = window
        .inner_size()
        .map_err(|e| format!("inner_size: {e}"))?;

    let surface = video::macos::MacosVideoSurface::new(
        ns_view_ptr,
        size.width as i32,
        size.height as i32,
    )
    .map_err(|e| format!("create video surface: {e}"))?;
    let surface: Arc<dyn VideoSurface> = Arc::new(surface);

    let player = Arc::new(Player::new_for_render().map_err(|e| format!("create render player: {e}"))?);

    let render_loop = RenderLoop::start(Arc::clone(&player), Arc::clone(&surface))
        .map_err(|e| format!("start render loop: {e}"))?;

    core::url_post_play::spawn_sponsor_skip_task(Arc::clone(&player));

    state
        .render_player
        .set(player)
        .map_err(|_| "render_player already initialised".to_string())?;
    state
        .render_loop
        .set(render_loop)
        .map_err(|_| "render_loop already initialised".to_string())?;

    Ok(())
}

/// Linux bring-up: pull the X11 XID + Display* from Tauri's
/// raw_window_handle and hand them to LinuxVideoSurface. X11 only —
/// Wayland users currently fall back to Xwayland (Tauri's GTK stack
/// runs through it by default), which is enough for first-frame.
#[cfg(target_os = "linux")]
fn bring_up_video_pipeline(
    window: &tauri::WebviewWindow,
    state: tauri::State<'_, GuiPlayer>,
) -> Result<(), String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};

    let win_handle = window.window_handle().map_err(|e| format!("window_handle: {e}"))?;
    let display_handle = window
        .display_handle()
        .map_err(|e| format!("display_handle: {e}"))?;

    let xid: u64 = match win_handle.as_raw() {
        RawWindowHandle::Xlib(h) => h.window,
        other => return Err(format!("expected Xlib window handle, got {:?}", other)),
    };
    if xid == 0 {
        return Err("got X11 XID 0 — running on pure Wayland?".into());
    }

    let display_ptr: *mut std::ffi::c_void = match display_handle.as_raw() {
        RawDisplayHandle::Xlib(h) => h
            .display
            .map(|nn| nn.as_ptr() as *mut std::ffi::c_void)
            .unwrap_or(std::ptr::null_mut()),
        other => return Err(format!("expected Xlib display handle, got {:?}", other)),
    };
    if display_ptr.is_null() {
        return Err("X11 Display* is null".into());
    }

    let size = window.inner_size().map_err(|e| format!("inner_size: {e}"))?;

    let surface_struct = video::linux::LinuxVideoSurface::new(
        display_ptr,
        xid,
        size.width as i32,
        size.height as i32,
    )
    .map_err(|e| format!("create video surface: {e}"))?;
    let child_xid = surface_struct.window_id();
    let surface: Arc<dyn VideoSurface> = Arc::new(surface_struct);

    // Create mpv with vo=x11 + wid pointing at the X11 child window we
    // just made. mpv handles all rendering internally via XPutImage, so
    // we don't need our GL render thread on Linux. See video/linux.rs
    // for why we abandon the glutin path here.
    let player = Arc::new(
        Player::new_with_wid_x11(child_xid as i64)
            .map_err(|e| format!("create render player: {e}"))?,
    );
    let render_loop = RenderLoop::start_passive(Arc::clone(&player), Arc::clone(&surface));

    core::url_post_play::spawn_sponsor_skip_task(Arc::clone(&player));

    state
        .render_player
        .set(player)
        .map_err(|_| "render_player already initialised".to_string())?;
    state
        .render_loop
        .set(render_loop)
        .map_err(|_| "render_loop already initialised".to_string())?;
    Ok(())
}
