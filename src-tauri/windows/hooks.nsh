!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack '"$INSTDIR\odc.exe" __path add'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "Command registration failed. You can run odc.exe from the installation folder."
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToStack '"$INSTDIR\odc.exe" __path remove'
  Pop $0
  Pop $1
  ${If} $0 != 0
    MessageBox MB_ICONEXCLAMATION "Could not remove the command PATH entry. Remove only this application's folder from your user PATH."
  ${EndIf}
  Delete "$INSTDIR\.path-added"
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
