; Puts the bundled clab.exe on the per-user PATH (HKCU, no admin rights: the
; installer runs with installMode currentUser). PowerShell edits the value so
; it stays REG_EXPAND_SZ; the entry is removed first, so repeats never duplicate.
!macro ClabUserPath ADD
  Push $0
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLAB_BIN", t "$INSTDIR\clab-runtime\bin")'
  System::Call 'Kernel32::SetEnvironmentVariable(t "CLAB_PATH_ADD", t "${ADD}")'
  nsExec::ExecToLog `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "$$d=$$env:CLAB_BIN; $$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true); $$p=[string]$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); $$e=@($$p -split ';' | Where-Object { $$_ -and $$_ -ne $$d }); if ($$env:CLAB_PATH_ADD -eq '1') { $$e += $$d }; $$k.SetValue('Path',($$e -join ';'),[Microsoft.Win32.RegistryValueKind]::ExpandString); $$k.Close()"`
  Pop $0
  System::Call 'user32::SendMessageTimeout(i 0xffff, i 0x1A, i 0, t "Environment", i 2, i 5000, *i .r0)'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro ClabUserPath 1
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro ClabUserPath 0
!macroend
