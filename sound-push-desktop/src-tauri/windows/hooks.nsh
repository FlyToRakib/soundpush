; SoundPush NSIS installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).

; Firewall rule. The installer runs per user without administrator rights, so it cannot add a
; firewall rule, and SoundPush never needs one for outgoing connections. When Windows Firewall
; blocks phones from connecting, the app offers "Allow SoundPush", which adds one inbound UDP
; rule named "SoundPush" for SoundPush.exe through a UAC prompt (src-tauri/src/network.rs).
;
; Removing that rule also needs administrator rights. On a real uninstall (not an update, not
; silent) the uninstaller checks for the rule first (`netsh ... show rule` works without
; administrator rights and fails when there is no such rule) and only then asks once through
; UAC. Declining leaves the rule behind; it names the removed SoundPush.exe, so it allows nothing.
;
; The same fix can move a network from the Public to the Private profile when the user says it is
; their own. That is a Windows setting every app on the computer relies on, not SoundPush's to take
; back, so the uninstaller deliberately leaves it alone.
!macro SOUNDPUSH_REMOVE_FIREWALL_RULE
  ${If} $UpdateMode <> 1
  ${AndIfNot} ${Silent}
    Push $0
    nsExec::Exec 'netsh.exe advfirewall firewall show rule name="SoundPush"'
    Pop $0
    ${If} $0 = 0
      ExecShellWait "runas" "netsh.exe" 'advfirewall firewall delete rule name="SoundPush" program="$INSTDIR\${MAINBINARYNAME}.exe"' SW_HIDE
    ${EndIf}
    Pop $0
  ${EndIf}
!macroend

; SoundPush keeps its data in %LOCALAPPDATA%\SoundPush (hooks.rs -> data_dir), not in the
; %LOCALAPPDATA%\net.soundpush.desktop folder Tauri removes. When the user ticks
; "Delete the application data" (and this is not an update), remove SoundPush's own files
; too. The folder itself is removed only if nothing else is left in it.
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro SOUNDPUSH_REMOVE_FIREWALL_RULE
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    Delete "$LOCALAPPDATA\SoundPush\identity.bin"
    Delete "$LOCALAPPDATA\SoundPush\trust.bin"
    Delete "$LOCALAPPDATA\SoundPush\settings.json"
    RMDir /r "$LOCALAPPDATA\SoundPush\logs"
    RMDir /r "$LOCALAPPDATA\SoundPush\diagnostics"
    RMDir "$LOCALAPPDATA\SoundPush"
  ${EndIf}
!macroend
