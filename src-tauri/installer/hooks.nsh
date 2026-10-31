; Parzi installer: the app's graphite and plain copy.
; Included by the Tauri template BEFORE any MUI_PAGE_* macro, so every
; MUI_* define below takes effect. Single file on purpose: relative
; !includes from here would resolve against the bundle output dir, not
; src-tauri/installer, so the dark theme lives inline.

!include LogicLib.nsh

SetFont "Segoe UI" 9
SpaceTexts none

; ============ Colours: the same graphite as the app ============
!define MUI_BGCOLOR "161616"
!define MUI_TEXTCOLOR "ECECEC"
!define MUI_INSTFILESPAGE_COLORS "ECECEC 242424"
!define MUI_INSTFILESPAGE_PROGRESSBAR "smooth"
!define MUI_HEADERIMAGE_RIGHT

!define PARZI_INK "0xECECEC"
!define PARZI_BG "0x161616"
!define PARZI_FIELD "0x242424"

; Windows' own dark theme for a control.
!macro ParziDarkTheme HANDLE THEME
  System::Call 'uxtheme::SetWindowTheme(p ${HANDLE}, w "${THEME}", n)'
!macroend
; No theme: the only look where a check box or radio button takes its text
; colour from SetCtlColors, and a progress bar its colours from messages.
!macro ParziNoTheme HANDLE
  System::Call 'uxtheme::SetWindowTheme(p ${HANDLE}, w " ", w " ")'
!macroend

; Reads a control's rectangle on its parent: $R0 left, $R1 top, $R2 right, $R3 bottom.
!macro ParziRect HANDLE PARENT
  System::Call '*(i,i,i,i) p.R8'
  System::Call 'user32::GetWindowRect(p ${HANDLE}, p $R8)'
  System::Call 'user32::MapWindowPoints(p 0, p ${PARENT}, p $R8, i 2)'
  System::Call '*$R8(i.R0, i.R1, i.R2, i.R3)'
  System::Free $R8
!macroend
!macro ParziPlace HANDLE X Y W H
  System::Call 'user32::SetWindowPos(p ${HANDLE}, p 0, i ${X}, i ${Y}, i ${W}, i ${H}, i 0x14)'
!macroend

; One DWORD window attribute. 20 is the dark title bar (Windows 10 1809 and
; later), 35 and 36 are its colours (Windows 11).
!macro ParziWindowAttribute ID VALUE
  System::Alloc 4
  Pop $R8
  System::Call "*$R8(i ${VALUE})"
  System::Call "dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i ${ID}, p $R8, i 4)"
  System::Free $R8
!macroend

; Paints one page: the window, its title bar and every control. $R9 is the page,
; $R7 the control at hand, $R8 scratch for the macros above. The template keeps
; values in $R0 to $R4 across its pages, so everything is saved and restored.
!macro ParziPaintFunction UN
Function ${UN}ParziPaint
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  Push $R6
  Push $R7
  Push $R8
  Push $R9

  !insertmacro ParziWindowAttribute 20 1
  !insertmacro ParziWindowAttribute 35 ${PARZI_BG}
  !insertmacro ParziWindowAttribute 36 ${PARZI_INK}

  SetCtlColors $HWNDPARENT ${PARZI_INK} ${PARZI_BG}

  ; The frame's etched lines and copyright line are drawn light whatever we set.
  GetDlgItem $R7 $HWNDPARENT 1035
  ShowWindow $R7 ${SW_HIDE}
  GetDlgItem $R7 $HWNDPARENT 1036
  ShowWindow $R7 ${SW_HIDE}
  GetDlgItem $R7 $HWNDPARENT 1045
  ShowWindow $R7 ${SW_HIDE}
  GetDlgItem $R7 $HWNDPARENT 1028
  ShowWindow $R7 ${SW_HIDE}
  GetDlgItem $R7 $HWNDPARENT 1256
  ShowWindow $R7 ${SW_HIDE}

  ; Back, Next and Cancel.
  GetDlgItem $R7 $HWNDPARENT 1
  !insertmacro ParziDarkTheme $R7 "DarkMode_Explorer"
  GetDlgItem $R7 $HWNDPARENT 2
  !insertmacro ParziDarkTheme $R7 "DarkMode_Explorer"
  GetDlgItem $R7 $HWNDPARENT 3
  !insertmacro ParziDarkTheme $R7 "DarkMode_Explorer"

  ; Back keeps the same gap to Next as Next keeps to Cancel.
  GetDlgItem $R7 $HWNDPARENT 2
  !insertmacro ParziRect $R7 $HWNDPARENT
  StrCpy $R5 $R0
  GetDlgItem $R7 $HWNDPARENT 1
  !insertmacro ParziRect $R7 $HWNDPARENT
  IntOp $R5 $R5 - $R2
  StrCpy $R6 $R0
  GetDlgItem $R7 $HWNDPARENT 3
  !insertmacro ParziRect $R7 $HWNDPARENT
  IntOp $R2 $R2 - $R0
  IntOp $R3 $R3 - $R1
  IntOp $R6 $R6 - $R5
  IntOp $R6 $R6 - $R2
  !insertmacro ParziPlace $R7 $R6 $R1 $R2 $R3

  ; The header's title starts on the same edge as the line under it.
  GetDlgItem $R7 $HWNDPARENT 1038
  !insertmacro ParziRect $R7 $HWNDPARENT
  StrCpy $R5 $R0
  GetDlgItem $R7 $HWNDPARENT 1037
  !insertmacro ParziRect $R7 $HWNDPARENT
  IntOp $R2 $R2 - $R5
  IntOp $R3 $R3 - $R1
  !insertmacro ParziPlace $R7 $R5 $R1 $R2 $R3

  ; A page made with nsDialogs still has the previous page behind it at this
  ; point, so go over every page there is. The old one is about to close.
  FindWindow $R9 "#32770" "" $HWNDPARENT
  ${While} $R9 <> 0
    SetCtlColors $R9 ${PARZI_INK} ${PARZI_BG}

    !if "${UN}" == ""
      ; The folder page: its group box goes below, so Browse moves up to the
      ; page's right edge, two of its heights below the text, and the field
      ; fills the row from the text's left edge, centred on the button.
      GetDlgItem $R7 $R9 1020
      ${If} $R7 <> 0
        GetDlgItem $R7 $R9 1006
        !insertmacro ParziRect $R7 $R9
        StrCpy $R5 $R0
        StrCpy $R6 $R1
        StrCpy $R4 $R2
        GetDlgItem $R7 $R9 1001
        !insertmacro ParziRect $R7 $R9
        IntOp $R2 $R2 - $R0
        IntOp $R3 $R3 - $R1
        IntOp $R4 $R4 - $R2
        IntOp $R1 $R3 * 2
        ; The text's box is tall enough to cover them otherwise.
        GetDlgItem $R8 $R9 1006
        IntOp $R0 $R4 + $R2
        IntOp $R0 $R0 - $R5
        !insertmacro ParziPlace $R8 $R5 $R6 $R0 $R1
        IntOp $R6 $R6 + $R1
        !insertmacro ParziPlace $R7 $R4 $R6 $R2 $R3
        IntOp $R3 $R3 / 2
        IntOp $R6 $R6 + $R3
        IntOp $R4 $R4 - $R3
        GetDlgItem $R7 $R9 1019
        !insertmacro ParziRect $R7 $R9
        IntOp $R3 $R3 - $R1
        IntOp $R2 $R4 - $R5
        IntOp $R1 $R3 / 2
        IntOp $R1 $R6 - $R1
        !insertmacro ParziPlace $R7 $R5 $R1 $R2 $R3
      ${EndIf}
    !endif

    FindWindow $R7 "" "" $R9
    ${While} $R7 <> 0
      System::Call 'user32::GetClassName(p $R7, t .R6, i 64)'
      ${If} $R6 == "Button"
        System::Call 'user32::GetWindowLong(p $R7, i -16) i .R5'
        IntOp $R5 $R5 & 15
        ${If} $R5 <= 1
          !insertmacro ParziDarkTheme $R7 "DarkMode_Explorer"
        ${ElseIf} $R5 = 7
          ; A group box's frame stays light in every theme.
          ShowWindow $R7 ${SW_HIDE}
        ${Else}
          !insertmacro ParziNoTheme $R7
          SetCtlColors $R7 ${PARZI_INK} ${PARZI_BG}
        ${EndIf}
      ${ElseIf} $R6 == "Edit"
        ; Flat: a field's sunken edge is drawn light in every theme.
        System::Call 'user32::GetWindowLong(p $R7, i -20) i .R5'
        IntOp $R5 $R5 & 0xFFFFFDFF
        System::Call 'user32::SetWindowLong(p $R7, i -20, i $R5)'
        System::Call 'user32::SetWindowPos(p $R7, p 0, i 0, i 0, i 0, i 0, i 0x37)'
        SetCtlColors $R7 ${PARZI_INK} ${PARZI_FIELD}
      ${ElseIf} $R6 == "msctls_progress32"
        !insertmacro ParziNoTheme $R7
        SendMessage $R7 0x0409 0 ${PARZI_INK}
        SendMessage $R7 0x2001 0 ${PARZI_FIELD}
      ${ElseIf} $R6 == "SysListView32"
        !insertmacro ParziDarkTheme $R7 "DarkMode_Explorer"
      ${Else}
        SetCtlColors $R7 ${PARZI_INK} ${PARZI_BG}
      ${EndIf}
      FindWindow $R7 "" "" $R9 $R7
    ${EndWhile}
    FindWindow $R9 "#32770" "" $HWNDPARENT $R9
  ${EndWhile}

  Pop $R9
  Pop $R8
  Pop $R7
  Pop $R6
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
FunctionEnd
!macroend
!insertmacro ParziPaintFunction ""
!insertmacro ParziPaintFunction "un."

; Every Modern UI page calls this macro as it shows, and MUI_PAGE_CUSTOMFUNCTION_SHOW
; only reaches the next page, so this is the one place that covers them all.
!macroundef MUI_PAGE_FUNCTION_CUSTOM
!macro MUI_PAGE_FUNCTION_CUSTOM TYPE
  !ifdef MUI_PAGE_CUSTOMFUNCTION_${TYPE}
    Call "${MUI_PAGE_CUSTOMFUNCTION_${TYPE}}"
    !undef MUI_PAGE_CUSTOMFUNCTION_${TYPE}
  !endif
  !if "${TYPE}" == "SHOW"
    Call "${MUI_PAGE_UNINSTALLER_FUNCPREFIX}ParziPaint"
  !endif
!macroend

; The template's "already installed" page is not a Modern UI page. Setting the
; focus is the last thing it does before showing, so it is painted from there.
!macro ParziSetFocus HWND
  !insertmacro __NSD_SetFocus ${HWND}
  Call ParziPaint
!macroend
!undef NSD_SetFocus
!define NSD_SetFocus `!insertmacro ParziSetFocus `

; ============ Copy: what a person needs, nothing else ============

; --- Welcome ---
!define MUI_WELCOMEPAGE_TITLE "Install Parzi"
!define MUI_WELCOMEPAGE_TEXT "Your coding agents and a browser, in one window.$\r$\n$\r$\nIt takes a few seconds."

; --- Destination ---
!define MUI_DIRECTORYPAGE_TEXT_TOP "Your sessions and settings stay in your user folder wherever Parzi goes."
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "Folder"

; --- Progress tail ---
!define MUI_INSTFILESPAGE_FINISHHEADER_TEXT "Installed"
!define MUI_INSTFILESPAGE_FINISHHEADER_SUBTEXT "Press Next to finish."

; --- Finish ---
!define MUI_FINISHPAGE_TITLE "Parzi is ready"
!define MUI_FINISHPAGE_TEXT "Open it, pick a folder and start a session."
!define MUI_FINISHPAGE_RUN_TEXT "Open Parzi"

; --- Uninstall confirm ---
!define MUI_UNCONFIRMPAGE_TEXT_TOP "This removes Parzi. Your sessions and settings stay unless you tick the box below."

; --- Abort guard ---
!define MUI_ABORTWARNING
!define MUI_ABORTWARNING_TEXT "Stop installing Parzi?"
!define MUI_ABORTWARNING_CANCELDEFAULT

; ============ Install: one Start Menu shortcut ============
; Installs up to 1.1 put the shortcut in a Parzi folder. An update does not
; recreate shortcuts, so the old one only goes once the new one exists.
!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    Delete "$SMPROGRAMS\${PRODUCTNAME}\${PRODUCTNAME}.lnk"
    RMDir "$SMPROGRAMS\${PRODUCTNAME}"
  ${EndIf}
!macroend

; ============ Uninstall: "delete my threads, settings and local data" ============
; The template only clears the bundle-id folders under AppData; Parzi keeps
; everything in the profile's .parzi folder. Never on an update: updates
; rerun this uninstaller with /UPDATE, and passive runs skip the confirm
; page, so the box stays unticked there.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIf} $PROFILE != ""
    RmDir /r "$PROFILE\.parzi"
  ${EndIf}
!macroend
