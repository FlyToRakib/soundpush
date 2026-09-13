; SoundPush NSIS installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).

; SoundPush keeps its data in %LOCALAPPDATA%\SoundPush (hooks.rs -> data_dir), not in the
; %LOCALAPPDATA%\net.soundpush.desktop folder Tauri removes. When the user ticks
; "Delete the application data" (and this is not an update), remove SoundPush's own files
; too. The folder itself is removed only if nothing else is left in it.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    Delete "$LOCALAPPDATA\SoundPush\identity.bin"
    Delete "$LOCALAPPDATA\SoundPush\trust.bin"
    Delete "$LOCALAPPDATA\SoundPush\settings.json"
    RMDir /r "$LOCALAPPDATA\SoundPush\logs"
    RMDir "$LOCALAPPDATA\SoundPush"
  ${EndIf}
!macroend
