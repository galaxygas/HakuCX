; This Source Code Form is subject to the terms of the Mozilla Public
; License, v. 2.0. If a copy of the MPL was not distributed with this
; file, You can obtain one at https://mozilla.org/MPL/2.0/.

; HakuCX installer for Windows (NSIS 3). Installs for the current user only, without
; administrator rights, into %LOCALAPPDATA%\Programs\HakuCX, and registers HakuCX as a browser
; Windows can make the default. Built by packaging/windows/package.ps1:
;
;   makensis /DVERSION=0.1.0 /DSTAGE=<staged files> /DOUTFILE=<setup.exe> /DICON=<hakucx.ico> installer.nsi
;
; The registry keys here must match crates/hakucx/src/os_integration.rs.
;
; Command line: /S installs silently; /RELAUNCH starts HakuCX afterwards (the updater uses
; both); /D=<folder> picks the folder.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !error "VERSION is not defined"
!endif
!ifndef STAGE
  !error "STAGE is not defined"
!endif
!ifndef OUTFILE
  !define OUTFILE "HakuCX-Setup-${VERSION}-x64.exe"
!endif

!define APP "HakuCX"
!define EXE "hakucx.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\HakuCX"
!define CLIENT_KEY "Software\Clients\StartMenuInternet\HakuCX"

Name "${APP}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ManifestDPIAware true
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} installer"
VIAddVersionKey "CompanyName" "HakuCX contributors"
VIAddVersionKey "LegalCopyright" "Mozilla Public License 2.0"

!ifdef ICON
  !define MUI_ICON "${ICON}"
  !define MUI_UNICON "${ICON}"
!endif
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APP}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSE.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; Set when HakuCX was already installed (an update).
Var Upgrade

Function .onInit
  ReadRegStr $Upgrade HKCU "${UNINSTALL_KEY}" "InstallLocation"
FunctionEnd

; Wait (up to 30 s) for a running HakuCX to close, so its files can be replaced. A running
; program can be renamed but not opened for writing, so that is the test.
!macro WaitForClose
  StrCpy $R0 0
  ${DoUntil} $R0 >= 60
    ${IfNot} ${FileExists} "$INSTDIR\${EXE}"
      ${Break}
    ${EndIf}
    ClearErrors
    FileOpen $R1 "$INSTDIR\${EXE}" a
    ${IfNot} ${Errors}
      FileClose $R1
      ${Break}
    ${EndIf}
    Sleep 500
    IntOp $R0 $R0 + 1
  ${Loop}
  ${If} $R0 >= 60
    IfSilent +2
    MessageBox MB_ICONSTOP "${APP} is still running. Close it and run the installer again."
    Abort
  ${EndIf}
  ; Left over by older installers.
  Delete "$INSTDIR\${EXE}.old"
!macroend

!define ICONREF '"$INSTDIR\${EXE}",0'
!define OPEN '"$INSTDIR\${EXE}" -- "%1"'

!macro RegisterFileType EXT
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities\FileAssociations" "${EXT}" "HakuCXHTML"
!macroend

Section "${APP}" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  !insertmacro WaitForClose
  File /r "${STAGE}\*.*"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Programs and Features
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "HakuCX contributors"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/galaxygas/HakuCX"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" '"$INSTDIR\${EXE}",0'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"

  ; Register as a browser (Windows asks the user to choose the default in Settings).
  WriteRegStr HKCU "Software\Classes\HakuCXURL" "" "HakuCX URL"
  WriteRegStr HKCU "Software\Classes\HakuCXURL" "FriendlyTypeName" "HakuCX URL"
  WriteRegStr HKCU "Software\Classes\HakuCXURL\DefaultIcon" "" '${ICONREF}'
  WriteRegStr HKCU "Software\Classes\HakuCXURL\shell\open\command" "" '${OPEN}'
  WriteRegStr HKCU "Software\Classes\HakuCXURL\Application" "ApplicationName" "${APP}"
  WriteRegStr HKCU "Software\Classes\HakuCXURL\Application" "ApplicationIcon" '${ICONREF}'
  WriteRegStr HKCU "Software\Classes\HakuCXURL\Application" "ApplicationCompany" "${APP}"
  WriteRegStr HKCU "Software\Classes\HakuCXHTML" "" "HakuCX HTML Document"
  WriteRegStr HKCU "Software\Classes\HakuCXHTML" "FriendlyTypeName" "HakuCX HTML Document"
  WriteRegStr HKCU "Software\Classes\HakuCXHTML\DefaultIcon" "" '${ICONREF}'
  WriteRegStr HKCU "Software\Classes\HakuCXHTML\shell\open\command" "" '${OPEN}'
  WriteRegStr HKCU "Software\Classes\HakuCXHTML\Application" "ApplicationName" "${APP}"
  WriteRegStr HKCU "Software\Classes\HakuCXHTML\Application" "ApplicationIcon" '${ICONREF}'
  WriteRegStr HKCU "Software\Classes\HakuCXHTML\Application" "ApplicationCompany" "${APP}"
  WriteRegStr HKCU "${CLIENT_KEY}" "" "${APP}"
  WriteRegStr HKCU "${CLIENT_KEY}\DefaultIcon" "" '${ICONREF}'
  WriteRegStr HKCU "${CLIENT_KEY}\shell\open\command" "" '"$INSTDIR\${EXE}"'
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities" "ApplicationName" "${APP}"
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities" "ApplicationIcon" '${ICONREF}'
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities" "ApplicationDescription" "An independent GX-style browser built on Servo"
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities\StartMenu" "StartMenuInternet" "${APP}"
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities\URLAssociations" "http" "HakuCXURL"
  WriteRegStr HKCU "${CLIENT_KEY}\Capabilities\URLAssociations" "https" "HakuCXURL"
  !insertmacro RegisterFileType ".htm"
  !insertmacro RegisterFileType ".html"
  !insertmacro RegisterFileType ".shtml"
  !insertmacro RegisterFileType ".xht"
  !insertmacro RegisterFileType ".xhtml"
  !insertmacro RegisterFileType ".svg"
  !insertmacro RegisterFileType ".webp"
  WriteRegStr HKCU "Software\RegisteredApplications" "${APP}" "${CLIENT_KEY}\Capabilities"
  ; SHCNE_ASSOCCHANGED: tell Explorer.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'

  CreateShortCut "$SMPROGRAMS\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0

  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/RELAUNCH" $1
  ${IfNot} ${Errors}
    Exec '"$INSTDIR\${EXE}"'
  ${EndIf}
SectionEnd

Section "Desktop shortcut" SecDesktop
  ; Silent updates keep the user's choice: no new shortcut if HakuCX was already installed.
  ${If} ${Silent}
  ${AndIf} $Upgrade != ""
    Return
  ${EndIf}
  CreateShortCut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "The browser. It is installed for you only and needs no administrator rights."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "A shortcut on your desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  !insertmacro WaitForClose
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\*.dll"
  Delete "$INSTDIR\LICENSE.txt"
  RMDir /r "$INSTDIR\licenses"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}.lnk"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\Classes\HakuCXURL"
  DeleteRegKey HKCU "Software\Classes\HakuCXHTML"
  DeleteRegKey HKCU "${CLIENT_KEY}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${APP}"
  ; Only remove "start at sign-in" if it points at this copy.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP}"
  ${If} $0 == '"$INSTDIR\${EXE}"'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP}"
  ${EndIf}
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  ; Profiles (bookmarks, history, saved passwords) stay unless the user asks.
  IfSilent done
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete your HakuCX data (profiles, bookmarks, history, saved passwords) from this computer?" IDNO done
  RMDir /r "$APPDATA\${APP}"
  done:
SectionEnd
