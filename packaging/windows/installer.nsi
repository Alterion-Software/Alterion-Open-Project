; Alterion Open Project - Windows installer
;
; Built with NSIS, which also runs on Linux (makensis), so the installer can be
; produced from the same machine that cross-compiles the binary.
;
;   makensis -DVERSION=0.1.0 installer.nsi
;
; # Who it is installed for
;
; One account by default, under %LOCALAPPDATA%\Programs, which is where Chrome,
; VS Code and Discord put themselves and for the same reason: an application
; that updates itself has to live somewhere the account can write. Program
; Files does not, and an update that has to ask for administrator rights is an
; update that cannot happen while nobody is there to answer.
;
; Installing for everyone on the machine is still offered, because some people
; genuinely want that. Choosing it moves four things at once, and they have to
; move together or the uninstaller will not find what the installer wrote:
;
;   install directory   %LOCALAPPDATA%\Programs   ->  Program Files
;   registry hive       HKCU                      ->  HKLM
;   shortcuts           this account's            ->  every account's
;   rights              none                      ->  administrator
;
; SetShellVarContext moves three of those in one go: it decides what $SMPROGRAMS
; and $DESKTOP mean, and it decides which hive SHCTX writes to. Every registry
; write below goes through SHCTX for exactly that reason. Nothing writes HKLM or
; HKCU by name except the two probes that have to ask about a specific hive.
;
; # Replacing a copy that is running
;
; Windows will not let a running executable be deleted or written over. It will
; let it be renamed: the process keeps its handle to the file under the new
; name, and the old name is free for the new binary. So this waits for the
; running copy to let go, renames what is there aside, writes the new one into
; the freed name, and clears the old one afterwards. If the install fails
; between those steps, .onInstFailed puts the previous version back rather than
; leaving an installation with no program in it.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
!include "nsDialogs.nsh"

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
!define APPKEY     "Software\${COMPANY}\${APPNAME}"
!define REGKEY     "Software\Microsoft\Windows\CurrentVersion\Uninstall\AlterionOpenProject"
!define EXTENSION  ".aprj"
!define PROGID     "Alterion.Project.1"

; How long to wait for a copy that is running to close before giving up, in
; quarter seconds. An update starts this installer and then closes itself, so
; the wait is normally over before the first check; this is the allowance for a
; machine that is busy, not a timeout anybody should ever reach.
!define WAITTRIES  60

Name "${APPNAME} ${VERSION}"
OutFile "dist\AlterionOpenProject-${VERSION}-setup.exe"

; The default, and the whole point of this change: a directory this account
; owns. Machine wide installs move it in ApplyScope, not here.
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "${APPKEY}" "InstallDir"

; Nothing here needs administrator rights unless somebody asks for a machine
; wide install, and asking for rights that are not needed is how a background
; update becomes impossible.
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${FILEVERSION}"
VIAddVersionKey "ProductName"     "${APPNAME}"
VIAddVersionKey "CompanyName"     "${COMPANY}"
VIAddVersionKey "FileDescription" "${APPNAME} installer"
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "LegalCopyright"  "Apache 2.0 licensed"

; "user" or "machine". Everything that differs between the two reads this.
Var Scope
; Where an installation of each kind already is, or "" if there is not one.
Var HereForMe
Var HereForEveryone
; Set when this run is the elevated relaunch of an earlier one, so the choice
; is not put a second time to somebody who has already made it.
Var Relaunched
Var Dialog
Var RadioMe
Var RadioEveryone

!define MUI_ABORTWARNING
!define MUI_ICON   "app.ico"
!define MUI_UNICON "app.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME}"

!insertmacro MUI_PAGE_LICENSE "LICENSE.txt"
Page custom ScopePage ScopePageLeave
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ---------------------------------------------------------------- the scope

; Whether HKLM can actually be written, which is the only question that
; matters and is not the same question as whether this account is in the
; administrators group. With RequestExecutionLevel user an administrator's
; process is not elevated, group membership says yes, and the write still
; fails. So it is tried rather than inferred. Defined for the installer and
; the uninstaller from one body, because they need the same answer.
!macro MachineRights un
Function ${un}HasMachineRights
  ClearErrors
  WriteRegStr HKLM "Software\${COMPANY}" "WriteProbe" "1"
  ${If} ${Errors}
    StrCpy $0 "no"
  ${Else}
    DeleteRegValue HKLM "Software\${COMPANY}" "WriteProbe"
    ; Only if this probe is what created it.
    DeleteRegKey /ifempty HKLM "Software\${COMPANY}"
    StrCpy $0 "yes"
  ${EndIf}
FunctionEnd
!macroend
!insertmacro MachineRights ""
!insertmacro MachineRights "un."

; Point everything at the chosen scope at once: the shell folders, the hive
; SHCTX resolves to, and the directory. Called from .onInit and again whenever
; the choice changes, so no page can be shown a directory the scope has since
; moved.
Function ApplyScope
  ${If} $Scope == "machine"
    SetShellVarContext all
    ${If} $HereForEveryone != ""
      StrCpy $INSTDIR "$HereForEveryone"
    ${Else}
      StrCpy $INSTDIR "$PROGRAMFILES64\${COMPANY}\${APPNAME}"
    ${EndIf}
  ${Else}
    SetShellVarContext current
    ${If} $HereForMe != ""
      StrCpy $INSTDIR "$HereForMe"
    ${Else}
      StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${APPNAME}"
    ${EndIf}
  ${EndIf}
FunctionEnd

; Machine wide needs administrator rights. Getting them means starting this
; installer again through the shell, which is the only thing that can raise the
; prompt, and letting this copy stop.
Function RequireMachineRights
  ${If} $Scope != "machine"
    Return
  ${EndIf}
  Call HasMachineRights
  ${If} $0 == "yes"
    Return
  ${EndIf}
  ${If} ${Silent}
    ; A silent run is an update happening by itself. There is nobody there to
    ; answer a prompt for administrator rights, so it stops without having
    ; changed anything and says so through its exit code.
    SetErrorLevel 2
    Quit
  ${EndIf}
  ClearErrors
  ExecShell "runas" "$EXEPATH" "/machine"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP|MB_OK \
      "Installing ${APPNAME} for everyone on this machine needs administrator \
       rights, and they were not granted.$\r$\n$\r$\n\
       Nothing has been changed. Running this installer again and choosing \
       $\"Just me$\" needs no such rights."
  ${EndIf}
  Quit
FunctionEnd

Function ScopePage
  ; Skipped when there is nothing to ask: a silent run has nobody to ask, and
  ; an elevated relaunch is carrying out a choice already made.
  ${If} $Relaunched == "yes"
    Abort
  ${EndIf}

  !insertmacro MUI_HEADER_TEXT "Install for" "Choose who this copy of ${APPNAME} is for."
  nsDialogs::Create 1018
  Pop $Dialog
  ${If} $Dialog == error
    Abort
  ${EndIf}

  ${NSD_CreateRadioButton} 0 0u 100% 12u "Just me"
  Pop $RadioMe
  ${NSD_CreateLabel} 14u 14u 96% 26u \
    "Installed under this account's own files. Needs no administrator rights, \
     now or later, which is what lets ${APPNAME} update itself."
  Pop $0

  ${NSD_CreateRadioButton} 0 46u 100% 12u "Everyone on this machine"
  Pop $RadioEveryone
  ${NSD_CreateLabel} 14u 60u 96% 26u \
    "Installed under Program Files, for every account. Needs administrator \
     rights to install, and again for every update."
  Pop $0

  ${If} $HereForEveryone != ""
    ${NSD_CreateLabel} 0 94u 100% 26u \
      "There is already an installation for everyone on this machine, at \
       $HereForEveryone. Updating that one is what happens by default, and it \
       needs administrator rights."
    Pop $0
  ${EndIf}

  ${If} $Scope == "machine"
    ${NSD_Check} $RadioEveryone
  ${Else}
    ${NSD_Check} $RadioMe
  ${EndIf}
  nsDialogs::Show
FunctionEnd

Function ScopePageLeave
  ${NSD_GetState} $RadioEveryone $0
  ${If} $0 == ${BST_CHECKED}
    StrCpy $Scope "machine"
  ${Else}
    StrCpy $Scope "user"
  ${EndIf}
  Call ApplyScope
  Call RequireMachineRights
FunctionEnd

; --------------------------------------------------------------- installing

; Wait for a copy that is running to let go of its own executable.
;
; Opening the file for writing is the probe: Windows shares a running image for
; reading only, so this fails exactly while the application is running and
; succeeds the moment it is not. Nothing has been changed yet when this gives
; up, which is what makes giving up safe.
Function WaitForTheRunningCopy
  IfFileExists "$INSTDIR\${EXENAME}" 0 gone

  StrCpy $R9 0
  again:
    ClearErrors
    FileOpen $R8 "$INSTDIR\${EXENAME}" a
    ${IfNot} ${Errors}
      FileClose $R8
      Goto gone
    ${EndIf}
    Sleep 250
    IntOp $R9 $R9 + 1
    ${If} $R9 < ${WAITTRIES}
      Goto again
    ${EndIf}

  ${If} ${Silent}
    SetErrorLevel 3
    Quit
  ${EndIf}
  MessageBox MB_ICONSTOP|MB_OK \
    "${APPNAME} is still running, and Windows will not let a program that is \
     running be replaced.$\r$\n$\r$\n\
     Nothing has been changed. Close it and run this installer again."
  Abort

  gone:
FunctionEnd

Section "${APPNAME} (required)" SecCore
  SectionIn RO

  Call WaitForTheRunningCopy
  SetOutPath "$INSTDIR"

  ; The rename that frees the name. Nothing has to be deleted first, which is
  ; the point: a file being replaced can still be held open by something, and
  ; renaming it is allowed where deleting it is not. If this install goes wrong
  ; from here on, .onInstFailed renames it back.
  Delete "$INSTDIR\${EXENAME}.old"
  ClearErrors
  Rename "$INSTDIR\${EXENAME}" "$INSTDIR\${EXENAME}.old"
  ClearErrors

  File "${EXENAME}"
  File "LICENSE.txt"
  File "README.md"
  File "app.ico"
  File "document.ico"

  ; Present only when the build script found them. A MinGW built binary can
  ; need a few runtime libraries beside it, and the WebView2 bootstrapper is
  ; fetched at build time; both are optional so the installer still builds
  ; without a network connection.
  File /nonfatal "*.dll"
  File /nonfatal "MicrosoftEdgeWebview2Setup.exe"

  ; The new one is in place, so the old one has served its purpose. Best
  ; effort: something may still be holding it, and the application sweeps it
  ; the next time it starts.
  Delete "$INSTDIR\${EXENAME}.old"

  WriteRegStr SHCTX "${APPKEY}" "InstallDir" "$INSTDIR"
  WriteRegStr SHCTX "${APPKEY}" "Version" "${VERSION}"
  ; Which of the two this is, recorded where the uninstaller can read it.
  WriteRegStr SHCTX "${APPKEY}" "Scope" "$Scope"

  ; What was chosen, so an update repeats it rather than starting from the
  ; defaults again.
  Call RecordChoices

  ; Add or Remove Programs, in the same hive as everything else: an entry in
  ; HKLM pointing at an uninstaller under a user's own files would outlive the
  ; account it belongs to.
  WriteRegStr   SHCTX "${REGKEY}" "DisplayName"     "${APPNAME}"
  WriteRegStr   SHCTX "${REGKEY}" "DisplayVersion"  "${VERSION}"
  WriteRegStr   SHCTX "${REGKEY}" "Publisher"       "${COMPANY}"
  WriteRegStr   SHCTX "${REGKEY}" "DisplayIcon"     "$INSTDIR\${EXENAME}"
  WriteRegStr   SHCTX "${REGKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr   SHCTX "${REGKEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD SHCTX "${REGKEY}" "NoModify" 1
  WriteRegDWORD SHCTX "${REGKEY}" "NoRepair" 1

  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD SHCTX "${REGKEY}" "EstimatedSize" "$0"

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
  ${AndIfNot} ${Silent}
    MessageBox MB_ICONEXCLAMATION|MB_OK \
      "The WebView2 runtime could not be installed (code $1).$\r$\n$\r$\n\
       ${APPNAME} is installed, but it will not open a window until the runtime \
       is present. It can be installed by hand from:$\r$\n\
       https://developer.microsoft.com/microsoft-edge/webview2/"
  ${EndIf}
  Return

  NoBootstrapper:
  ${IfNot} ${Silent}
    MessageBox MB_ICONEXCLAMATION|MB_OK \
      "${APPNAME} needs the Microsoft WebView2 runtime, which is not installed \
       on this machine.$\r$\n$\r$\n\
       The application is installed, but it will not open a window until the \
       runtime is present. It can be installed from:$\r$\n\
       https://developer.microsoft.com/microsoft-edge/webview2/"
  ${EndIf}
FunctionEnd

Section "Start Menu shortcut" SecStartMenu
  CreateDirectory "$SMPROGRAMS\${COMPANY}"
  CreateShortcut "$SMPROGRAMS\${COMPANY}\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\app.ico"
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\app.ico"
SectionEnd

Section "Open aop:// links with ${APPNAME}" SecScheme
  ; A shared plan is passed around as an aop:// link. Without this the link
  ; opens in a browser, which cannot do anything with it.
  ;
  ; Software\Classes under SHCTX rather than HKCR, which is a merged view of
  ; both hives and cannot be written to on one account's behalf. Under HKCU it
  ; registers the scheme for this account and needs no rights to do it.
  ;
  ; The empty "URL Protocol" value is the marker Windows looks for; a key
  ; without it is treated as a file type and the scheme is never routed here.
  WriteRegStr SHCTX "Software\Classes\aop" "" "URL:Alterion Open Project"
  WriteRegStr SHCTX "Software\Classes\aop" "URL Protocol" ""
  WriteRegStr SHCTX "Software\Classes\aop\DefaultIcon" "" "$INSTDIR\app.ico,0"
  WriteRegStr SHCTX "Software\Classes\aop\shell\open\command" "" '"$INSTDIR\${EXENAME}" "%1"'
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd

Section "Open .aprj files with ${APPNAME}" SecAssoc
  WriteRegStr SHCTX "Software\Classes\${EXTENSION}" "" "${PROGID}"
  WriteRegStr SHCTX "Software\Classes\${PROGID}" "" "Alterion Project"
  WriteRegStr SHCTX "Software\Classes\${PROGID}\DefaultIcon" "" "$INSTDIR\document.ico,0"
  WriteRegStr SHCTX "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${EXENAME}" "%1"'
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd

; Below the sections on purpose: a section's index is a define the
; Section statement creates, so nothing above them can name one.
Function RecordChoices
  ${If} ${SectionIsSelected} ${SecStartMenu}
    WriteRegDWORD SHCTX "${APPKEY}" "Shortcut.StartMenu" 1
  ${Else}
    WriteRegDWORD SHCTX "${APPKEY}" "Shortcut.StartMenu" 0
  ${EndIf}
  ${If} ${SectionIsSelected} ${SecDesktop}
    WriteRegDWORD SHCTX "${APPKEY}" "Shortcut.Desktop" 1
  ${Else}
    WriteRegDWORD SHCTX "${APPKEY}" "Shortcut.Desktop" 0
  ${EndIf}
  ${If} ${SectionIsSelected} ${SecScheme}
    WriteRegDWORD SHCTX "${APPKEY}" "Register.Scheme" 1
  ${Else}
    WriteRegDWORD SHCTX "${APPKEY}" "Register.Scheme" 0
  ${EndIf}
  ${If} ${SectionIsSelected} ${SecAssoc}
    WriteRegDWORD SHCTX "${APPKEY}" "Register.Files" 1
  ${Else}
    WriteRegDWORD SHCTX "${APPKEY}" "Register.Files" 0
  ${EndIf}
FunctionEnd

; Put back what was chosen last time, so an update does not quietly bring back
; a shortcut somebody removed or a file association they said no to. A value
; that was never written leaves the section on its default, which is what a
; first install wants.
!macro RestoreChoice value section
  ClearErrors
  ReadRegDWORD $0 SHCTX "${APPKEY}" "${value}"
  ${IfNot} ${Errors}
  ${AndIf} $0 == 0
    !insertmacro UnselectSection ${section}
  ${EndIf}
!macroend

Function .onInit
  ; What is already here, asked of both hives, because either can hold an
  ; installation and they mean different things.
  ReadRegStr $HereForMe HKCU "${APPKEY}" "InstallDir"
  ReadRegStr $HereForEveryone HKLM "${APPKEY}" "InstallDir"

  StrCpy $Scope "user"
  StrCpy $Relaunched "no"

  ; An installation that is already here is updated where it is. Installing the
  ; other way round beside it would leave two copies, one of them stale, and
  ; nothing to say which one a shortcut or a .aprj file opens.
  ${If} $HereForMe == ""
  ${AndIf} $HereForEveryone != ""
    StrCpy $Scope "machine"
  ${EndIf}

  ; The switch the elevated relaunch carries, so the second run installs what
  ; the first one was asked for.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/machine" $R1
  ${IfNot} ${Errors}
    StrCpy $Scope "machine"
    StrCpy $Relaunched "yes"
  ${EndIf}
  ClearErrors
  ${GetOptions} $R0 "/user" $R1
  ${IfNot} ${Errors}
    StrCpy $Scope "user"
    StrCpy $Relaunched "yes"
  ${EndIf}

  Call ApplyScope

  !insertmacro RestoreChoice "Shortcut.StartMenu" ${SecStartMenu}
  !insertmacro RestoreChoice "Shortcut.Desktop" ${SecDesktop}
  !insertmacro RestoreChoice "Register.Scheme" ${SecScheme}
  !insertmacro RestoreChoice "Register.Files" ${SecAssoc}

  ; Both of the ways the page that would otherwise ask this is not shown: a
  ; silent run has nobody to ask, and a relaunch is carrying out an answer
  ; already given. Neither may end up writing to a hive it cannot write to.
  ${If} ${Silent}
  ${OrIf} $Relaunched == "yes"
    Call RequireMachineRights
  ${EndIf}
FunctionEnd

; The previous version goes back under its own name if the new one never
; landed. Anything else leaves an installation with no application in it.
Function .onInstFailed
  IfFileExists "$INSTDIR\${EXENAME}" done
  IfFileExists "$INSTDIR\${EXENAME}.old" 0 done
  Rename "$INSTDIR\${EXENAME}.old" "$INSTDIR\${EXENAME}"
  done:
FunctionEnd

Function .onInstSuccess
  ; A silent install is an update the application asked for and then closed
  ; itself to let happen, so it puts the application back rather than leaving
  ; somebody looking at a window that went away. Silent means user scope, so
  ; this process is not elevated and neither is what it starts.
  ${If} ${Silent}
    Exec '"$INSTDIR\${EXENAME}"'
  ${EndIf}
FunctionEnd

LangString DESC_SecCore      ${LANG_ENGLISH} "The application itself."
LangString DESC_SecStartMenu ${LANG_ENGLISH} "Add ${APPNAME} to the Start menu."
LangString DESC_SecDesktop   ${LANG_ENGLISH} "Put a shortcut on the desktop."
LangString DESC_SecAssoc     ${LANG_ENGLISH} "Double-clicking a .aprj plan opens it here."
LangString DESC_SecScheme    ${LANG_ENGLISH} "Clicking a shared aop:// plan link opens it here."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecCore}      $(DESC_SecCore)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} $(DESC_SecStartMenu)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop}   $(DESC_SecDesktop)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecAssoc}     $(DESC_SecAssoc)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecScheme}    $(DESC_SecScheme)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

; ------------------------------------------------------------- uninstalling

; Which installation this uninstaller belongs to, worked out from where it is
; rather than from a value it might disagree with. Everything below then reads
; the same hive and the same shell folders the installer wrote.
Function un.onInit
  StrCpy $Scope "user"
  SetShellVarContext current

  ReadRegStr $0 HKLM "${APPKEY}" "InstallDir"
  ${If} $0 != ""
  ${AndIf} $0 == "$INSTDIR"
    StrCpy $Scope "machine"
    SetShellVarContext all
    Call un.RequireMachineRights
  ${EndIf}
FunctionEnd

Function un.RequireMachineRights
  Call un.HasMachineRights
  ${If} $0 == "yes"
    Return
  ${EndIf}
  ClearErrors
  ExecShell "runas" "$INSTDIR\uninstall.exe"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP|MB_OK \
      "Removing an installation that is for everyone on this machine needs \
       administrator rights, and they were not granted.$\r$\n$\r$\n\
       Nothing has been removed."
  ${EndIf}
  Quit
FunctionEnd

Section "Uninstall"
  Delete "$INSTDIR\${EXENAME}"
  ; A previous version an update left behind, if one is still here.
  Delete "$INSTDIR\${EXENAME}.old"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\app.ico"
  Delete "$INSTDIR\document.ico"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$INSTDIR\MicrosoftEdgeWebview2Setup.exe"
  Delete "$INSTDIR\*.dll"
  RMDir "$INSTDIR"
  ; The company directory a machine wide install adds above its own, and only
  ; if nothing else is left in it. A per user install has no such level: its
  ; parent is Programs, which belongs to every application there.
  ${If} $Scope == "machine"
    RMDir "$PROGRAMFILES64\${COMPANY}"
  ${EndIf}

  Delete "$SMPROGRAMS\${COMPANY}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${COMPANY}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  DeleteRegKey SHCTX "Software\Classes\aop"

  DeleteRegKey SHCTX "${REGKEY}"
  DeleteRegKey SHCTX "${APPKEY}"
  DeleteRegKey /ifempty SHCTX "Software\${COMPANY}"

  ; Only give the extension back if it still points at this app.
  ReadRegStr $0 SHCTX "Software\Classes\${EXTENSION}" ""
  ${If} $0 == "${PROGID}"
    DeleteRegKey SHCTX "Software\Classes\${EXTENSION}"
  ${EndIf}
  DeleteRegKey SHCTX "Software\Classes\${PROGID}"
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
