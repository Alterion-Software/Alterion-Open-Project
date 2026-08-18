; Alterion Open Project - Windows installer
;
; Built with NSIS, which also runs on Linux (makensis), so the installer can be
; produced from the same machine that cross-compiles the binary.
;
;   makensis -DVERSION=0.1.0 installer.nsi

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif

; The display version can carry a pre-release suffix; this one cannot, because
; a Windows version resource is four numbers and nothing else.
!ifndef FILEVERSION
  !define FILEVERSION "0.1.0.0"
!endif

!define APPNAME    "Alterion Open Project"
!define COMPANY    "Alterion"
!define EXENAME    "alterion-open-project.exe"
!define REGKEY     "Software\Microsoft\Windows\CurrentVersion\Uninstall\AlterionOpenProject"
!define EXTENSION  ".aprj"
!define PROGID     "Alterion.Project.1"

Name "${APPNAME} ${VERSION}"
OutFile "dist\AlterionOpenProject-${VERSION}-setup.exe"
InstallDir "$PROGRAMFILES64\${COMPANY}\${APPNAME}"
InstallDirRegKey HKLM "Software\${COMPANY}\${APPNAME}" "InstallDir"
RequestExecutionLevel admin
SetCompressor /SOLID lzma

VIProductVersion "${FILEVERSION}"
VIAddVersionKey "ProductName"     "${APPNAME}"
VIAddVersionKey "CompanyName"     "${COMPANY}"
VIAddVersionKey "FileDescription" "${APPNAME} installer"
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "LegalCopyright"  "MIT licensed"

!define MUI_ABORTWARNING
!define MUI_ICON   "app.ico"
!define MUI_UNICON "app.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME}"

!insertmacro MUI_PAGE_LICENSE "LICENSE.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "${APPNAME} (required)" SecCore
  SectionIn RO
  SetOutPath "$INSTDIR"

  File "${EXENAME}"
  File "LICENSE.txt"
  File "README.md"
  File "app.ico"

  ; Present only when the build script found them. A MinGW built binary can
  ; need a few runtime libraries beside it, and the WebView2 bootstrapper is
  ; fetched at build time; both are optional so the installer still builds
  ; without a network connection.
  File /nonfatal "*.dll"
  File /nonfatal "MicrosoftEdgeWebview2Setup.exe"

  WriteRegStr HKLM "Software\${COMPANY}\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\${COMPANY}\${APPNAME}" "Version" "${VERSION}"

  ; Add or Remove Programs
  WriteRegStr   HKLM "${REGKEY}" "DisplayName"     "${APPNAME}"
  WriteRegStr   HKLM "${REGKEY}" "DisplayVersion"  "${VERSION}"
  WriteRegStr   HKLM "${REGKEY}" "Publisher"       "${COMPANY}"
  WriteRegStr   HKLM "${REGKEY}" "DisplayIcon"     "$INSTDIR\${EXENAME}"
  WriteRegStr   HKLM "${REGKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr   HKLM "${REGKEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKLM "${REGKEY}" "NoModify" 1
  WriteRegDWORD HKLM "${REGKEY}" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${REGKEY}" "EstimatedSize" "$0"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; The window is a WebView2 control, so without the runtime the application
  ; starts and then shows nothing at all. Windows 11 ships it and most updated
  ; Windows 10 machines have it through Edge, but neither is a guarantee, so it
  ; is checked rather than assumed.
  ;
  ; The version string is written under one of three keys depending on whether
  ; the runtime was installed per machine, per user, or on a 32 bit view of the
  ; registry, and all three have to be tried before concluding it is missing.
  Call EnsureWebView2
SectionEnd

Function EnsureWebView2
  ; The Evergreen runtime's product code. Fixed by Microsoft, not by us.
  !define WV2KEY "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WV2KEY}" "pv"
  ${If} $0 != ""
  ${AndIf} $0 != "0.0.0.0"
    Return
  ${EndIf}

  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WV2KEY}" "pv"
  ${If} $0 != ""
  ${AndIf} $0 != "0.0.0.0"
    Return
  ${EndIf}

  ReadRegStr $0 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WV2KEY}" "pv"
  ${If} $0 != ""
  ${AndIf} $0 != "0.0.0.0"
    Return
  ${EndIf}

  ; Missing. The bootstrapper is about two megabytes and fetches the runtime
  ; itself, which is why it is bundled rather than the full redistributable.
  IfFileExists "$INSTDIR\MicrosoftEdgeWebview2Setup.exe" 0 NoBootstrapper

  DetailPrint "Installing the WebView2 runtime, which ${APPNAME} needs to draw its window."
  ExecWait '"$INSTDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
  Delete "$INSTDIR\MicrosoftEdgeWebview2Setup.exe"
  ${If} $1 != 0
    MessageBox MB_ICONEXCLAMATION|MB_OK \
      "The WebView2 runtime could not be installed (code $1).$\r$\n$\r$\n\
       ${APPNAME} is installed, but it will not open a window until the runtime \
       is present. It can be installed by hand from:$\r$\n\
       https://developer.microsoft.com/microsoft-edge/webview2/"
  ${EndIf}
  Return

  NoBootstrapper:
  MessageBox MB_ICONEXCLAMATION|MB_OK \
    "${APPNAME} needs the Microsoft WebView2 runtime, which is not installed \
     on this machine.$\r$\n$\r$\n\
     The application is installed, but it will not open a window until the \
     runtime is present. It can be installed from:$\r$\n\
     https://developer.microsoft.com/microsoft-edge/webview2/"
FunctionEnd

Section "Start Menu shortcut" SecStartMenu
  CreateDirectory "$SMPROGRAMS\${COMPANY}"
  CreateShortcut "$SMPROGRAMS\${COMPANY}\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\app.ico"
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\app.ico"
SectionEnd

Section "Open .aprj files with ${APPNAME}" SecAssoc
  WriteRegStr HKCR "${EXTENSION}" "" "${PROGID}"
  WriteRegStr HKCR "${PROGID}" "" "Alterion Project"
  WriteRegStr HKCR "${PROGID}\DefaultIcon" "" "$INSTDIR\app.ico,0"
  WriteRegStr HKCR "${PROGID}\shell\open\command" "" '"$INSTDIR\${EXENAME}" "%1"'
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd

LangString DESC_SecCore      ${LANG_ENGLISH} "The application itself."
LangString DESC_SecStartMenu ${LANG_ENGLISH} "Add ${APPNAME} to the Start menu."
LangString DESC_SecDesktop   ${LANG_ENGLISH} "Put a shortcut on the desktop."
LangString DESC_SecAssoc     ${LANG_ENGLISH} "Double-clicking a .aprj plan opens it here."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore}      $(DESC_SecCore)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} $(DESC_SecStartMenu)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop}   $(DESC_SecDesktop)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAssoc}     $(DESC_SecAssoc)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\${EXENAME}"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\app.ico"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\MicrosoftEdgeWebview2Setup.exe"
  Delete "$INSTDIR\*.dll"
  RMDir "$INSTDIR"
  RMDir "$PROGRAMFILES64\${COMPANY}"

  Delete "$SMPROGRAMS\${COMPANY}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${COMPANY}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  DeleteRegKey HKLM "${REGKEY}"
  DeleteRegKey HKLM "Software\${COMPANY}\${APPNAME}"

  ; Only give the extension back if it still points at this app.
  ReadRegStr $0 HKCR "${EXTENSION}" ""
  ${If} $0 == "${PROGID}"
    DeleteRegKey HKCR "${EXTENSION}"
  ${EndIf}
  DeleteRegKey HKCR "${PROGID}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
