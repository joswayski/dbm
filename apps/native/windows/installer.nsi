; Windows installer for Anybase: puts dbm.exe in %LOCALAPPDATA%\DBM, adds a
; Start menu shortcut, and registers an uninstaller. No administrator rights
; needed. The folder, executable, and uninstall key keep their DBM names so
; installs from before the Anybase rename update in place.
;
;   makensis -DVERSION=2026.9.2802 -DEXE=path\to\dbm-workbench.exe \
;            -DICON=path\to\icon.ico -DOUTFILE=DBM-Windows-x64-setup.exe installer.nsi
;
; The former Tauri app installed the same way (same folder, executable name,
; and uninstall key), and its updater runs this installer with
; "/P /R /UPDATE /ARGS ...": /P means no questions, /R means reopen the app after.
; Both are honoured; anything else is ignored.

Unicode true
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "Pass -DVERSION=MAJOR.MINOR.PATCH"
!endif

Name "Anybase"
OutFile "${OUTFILE}"
Icon "${ICON}"
UninstallIcon "${ICON}"
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\DBM"
SetCompressor /SOLID lzma
ShowInstDetails nevershow
AutoCloseWindow true

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\DBM"

Var Reopen

Function .onInit
  ; Install over an existing copy wherever it is (a Tauri install may have
  ; picked another folder); its uninstaller sits beside dbm.exe.
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "UninstallString"
  ${If} $0 != ""
    StrCpy $1 $0 1
    ${If} $1 == '"'
      StrCpy $0 $0 "" 1
      StrCpy $0 $0 -1
    ${EndIf}
    ${GetParent} $0 $1
    ${If} ${FileExists} "$1\dbm.exe"
      StrCpy $INSTDIR $1
    ${EndIf}
  ${EndIf}
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/P" $1
  ${IfNot} ${Errors}
    SetSilent silent
  ${EndIf}
  ClearErrors
  ${GetOptions} $0 "/R" $1
  ${IfNot} ${Errors}
    StrCpy $Reopen "1"
  ${EndIf}
FunctionEnd

Section "Anybase"
  SetOutPath "$INSTDIR"
  ; An updating copy may still be closing; retry while dbm.exe is in use.
  StrCpy $2 0
  retry:
  ClearErrors
  File "/oname=dbm.exe" "${EXE}"
  ${If} ${Errors}
    IntOp $2 $2 + 1
    ${If} $2 < 50
      Sleep 200
      Goto retry
    ${EndIf}
    MessageBox MB_ICONSTOP "Anybase is still running. Close it and run the installer again."
    Abort
  ${EndIf}
  ; The Tauri app's WebView files are no longer used.
  Delete "$INSTDIR\WebView2Loader.dll"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  ; Installs from before the rename have a DBM shortcut; replace it.
  Delete "$SMPROGRAMS\DBM.lnk"
  CreateShortCut "$SMPROGRAMS\Anybase.lnk" "$INSTDIR\dbm.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Anybase"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\dbm.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Anybase"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Function .onInstSuccess
  ; Reopen after an update, and after an interactive first install.
  ${If} $Reopen == "1"
  ${OrIfNot} ${Silent}
    Exec '"$INSTDIR\dbm.exe"'
  ${EndIf}
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\dbm.exe"
  Delete "$INSTDIR\dbm.previous.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Anybase.lnk"
  Delete "$SMPROGRAMS\DBM.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
