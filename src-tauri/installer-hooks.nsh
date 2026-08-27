; Installer hooks for the Windows (NSIS) package.
;
; Between v0.9 and v0.10 the per-user install directory moved from
; $LOCALAPPDATA\unflick to $LOCALAPPDATA\Programs\unflick, because that is
; what the packaging default became. Both installs register the *same*
; uninstall key, so upgrading overwrote the old uninstaller's registration
; and left about half a gigabyte of files behind with nothing pointing at
; them — no entry in Apps & Features, no shortcut, no trace anyone would
; find without going looking.
;
; This removes the old payload during an upgrade, before the new files are
; written. It cannot help a machine that already upgraded — `unflick
; cleanup` is for those — but it stops the next one.
;
; What it deliberately does NOT touch: `thumbs` and `covers` inside that
; same folder are the *live* caches of the version being installed, because
; on Windows the cache root and the old install directory are the same path.
; And it does not run the old uninstall.exe: that would take the registry
; key with it, and by then the key belongs to us.

!macro NSIS_HOOK_PREINSTALL
  StrCpy $R0 "$LOCALAPPDATA\unflick"

  ; Only act on something that was demonstrably an install, and never on
  ; the directory we are installing into.
  ${If} $R0 != $INSTDIR
  ${AndIf} ${FileExists} "$R0\uninstall.exe"
    DetailPrint "Removing files left by an earlier unflick in $R0"

    Delete "$R0\unflick.exe"
    Delete "$R0\uninstall.exe"
    RMDir /r "$R0\bin"
    RMDir /r "$R0\ffmpeg"
    RMDir /r "$R0\mpv-dev"
    RMDir /r "$R0\whisper"
    RMDir /r "$R0\yt-dlp"
    RMDir /r "$R0\legal"

    ; Non-recursive: succeeds only if the caches were not there, which is
    ; the one case where the folder itself is genuinely finished with.
    RMDir "$R0"
  ${EndIf}
!macroend
