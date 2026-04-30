//! Native menu translations.
//!
//! Strings here mirror the `menu` namespace in `src/i18n/<locale>.json`.
//! Tauri's menu items are constructed at startup so we can't hot-swap labels
//! when the user changes locale — a restart is required (the README and
//! Settings panel both call this out).

#[derive(Clone, Copy)]
pub struct MenuStrings {
    pub file: &'static str,
    pub open_file: &'static str,
    pub open_url: &'static str,
    pub quit: &'static str,
    pub playback: &'static str,
    pub play_pause: &'static str,
    pub stop: &'static str,
    pub volume_up: &'static str,
    pub volume_down: &'static str,
    pub view: &'static str,
    pub fullscreen: &'static str,
    pub pip: &'static str,
    pub library: &'static str,
    pub help: &'static str,
    pub about: &'static str,
    pub check_updates: &'static str,
}

const EN: MenuStrings = MenuStrings {
    file: "File",
    open_file: "Open File...",
    open_url: "Open URL...",
    quit: "Quit",
    playback: "Playback",
    play_pause: "Play / Pause",
    stop: "Stop",
    volume_up: "Volume Up",
    volume_down: "Volume Down",
    view: "View",
    fullscreen: "Toggle Fullscreen",
    pip: "Picture in Picture",
    library: "Toggle Library",
    help: "Help",
    about: "About unflick",
    check_updates: "Check for Updates...",
};

const ZH_CN: MenuStrings = MenuStrings {
    file: "文件",
    open_file: "打开文件…",
    open_url: "打开 URL…",
    quit: "退出",
    playback: "播放",
    play_pause: "播放 / 暂停",
    stop: "停止",
    volume_up: "增大音量",
    volume_down: "减小音量",
    view: "视图",
    fullscreen: "切换全屏",
    pip: "画中画",
    library: "切换媒体库",
    help: "帮助",
    about: "关于 unflick",
    check_updates: "检查更新…",
};

const ZH_TW: MenuStrings = MenuStrings {
    file: "檔案",
    open_file: "開啟檔案…",
    open_url: "開啟 URL…",
    quit: "結束",
    playback: "播放",
    play_pause: "播放 / 暫停",
    stop: "停止",
    volume_up: "增大音量",
    volume_down: "減小音量",
    view: "檢視",
    fullscreen: "切換全螢幕",
    pip: "子母畫面",
    library: "切換媒體庫",
    help: "說明",
    about: "關於 unflick",
    check_updates: "檢查更新…",
};

const JA: MenuStrings = MenuStrings {
    file: "ファイル",
    open_file: "ファイルを開く…",
    open_url: "URL を開く…",
    quit: "終了",
    playback: "再生",
    play_pause: "再生 / 一時停止",
    stop: "停止",
    volume_up: "音量を上げる",
    volume_down: "音量を下げる",
    view: "表示",
    fullscreen: "フルスクリーン切り替え",
    pip: "ピクチャ・イン・ピクチャ",
    library: "ライブラリの表示",
    help: "ヘルプ",
    about: "unflick について",
    check_updates: "アップデートを確認…",
};

const KO: MenuStrings = MenuStrings {
    file: "파일",
    open_file: "파일 열기…",
    open_url: "URL 열기…",
    quit: "종료",
    playback: "재생",
    play_pause: "재생 / 일시정지",
    stop: "정지",
    volume_up: "음량 높이기",
    volume_down: "음량 낮추기",
    view: "보기",
    fullscreen: "전체화면 전환",
    pip: "PIP 모드",
    library: "라이브러리 토글",
    help: "도움말",
    about: "unflick 정보",
    check_updates: "업데이트 확인…",
};

const DE: MenuStrings = MenuStrings {
    file: "Datei",
    open_file: "Datei öffnen…",
    open_url: "URL öffnen…",
    quit: "Beenden",
    playback: "Wiedergabe",
    play_pause: "Wiedergabe / Pause",
    stop: "Stopp",
    volume_up: "Lauter",
    volume_down: "Leiser",
    view: "Ansicht",
    fullscreen: "Vollbild umschalten",
    pip: "Bild-in-Bild",
    library: "Bibliothek umschalten",
    help: "Hilfe",
    about: "Über unflick",
    check_updates: "Nach Updates suchen…",
};

const FR: MenuStrings = MenuStrings {
    file: "Fichier",
    open_file: "Ouvrir un fichier…",
    open_url: "Ouvrir une URL…",
    quit: "Quitter",
    playback: "Lecture",
    play_pause: "Lecture / Pause",
    stop: "Arrêter",
    volume_up: "Volume +",
    volume_down: "Volume −",
    view: "Affichage",
    fullscreen: "Basculer en plein écran",
    pip: "Picture in Picture",
    library: "Basculer la bibliothèque",
    help: "Aide",
    about: "À propos d'unflick",
    check_updates: "Vérifier les mises à jour…",
};

const ES: MenuStrings = MenuStrings {
    file: "Archivo",
    open_file: "Abrir archivo…",
    open_url: "Abrir URL…",
    quit: "Salir",
    playback: "Reproducción",
    play_pause: "Reproducir / Pausar",
    stop: "Detener",
    volume_up: "Subir volumen",
    volume_down: "Bajar volumen",
    view: "Ver",
    fullscreen: "Pantalla completa",
    pip: "Picture in Picture",
    library: "Mostrar biblioteca",
    help: "Ayuda",
    about: "Acerca de unflick",
    check_updates: "Buscar actualizaciones…",
};

/// Look up the menu translation bundle for a locale string. Falls back to
/// English on unknown / unsupported codes.
pub fn menu_strings(locale: &str) -> MenuStrings {
    match locale {
        "zh-CN" => ZH_CN,
        "zh-TW" => ZH_TW,
        "ja" => JA,
        "ko" => KO,
        "de" => DE,
        "fr" => FR,
        "es" => ES,
        _ => EN,
    }
}

/// Read the persisted UI locale out of `settings.json`. We don't want to
/// pull serde_json's Value type in here just for one field, so this stays
/// hand-rolled and best-effort: any failure (file missing, malformed,
/// unknown code) returns "en".
pub fn read_locale_from_settings() -> String {
    use crate::core::settings;

    if let Ok(Some(value)) = settings::get("locale") {
        if let Some(s) = value.as_str() {
            return s.to_string();
        }
    }
    "en".to_string()
}
