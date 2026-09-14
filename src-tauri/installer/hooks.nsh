; Parzi installer voice: page titles and copy for the NSIS wizard.
; Included by the Tauri template BEFORE any MUI_PAGE_* macro, so every
; MUI_*PAGE define below takes effect. Keep lines short: the wizard is 497px.

; --- Welcome ---
!define MUI_WELCOMEPAGE_TITLE "Welcome to Parzi"
!define MUI_WELCOMEPAGE_TEXT "Parzi is a lean agent harness: Claude, Codex, Antigravity, OpenCode and Grok through one elegant window.$\r$\n$\r$\nThis installs Parzi for your user account only, no admin needed. Your threads and settings live in your user folder and are never touched by updates."

; --- License (MIT, from bundle.licenseFile) ---
!define MUI_LICENSEPAGE_TEXT_TOP "Parzi is MIT-licensed. Short version: do what you want, keep the notice."
!define MUI_LICENSEPAGE_TEXT_BOTTOM "Press Next to accept and continue."

; --- Destination ---
!define MUI_DIRECTORYPAGE_TEXT_TOP "Choose where Parzi should live. Your data stays in your user folder either way."
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "Parzi folder"

; --- Start Menu ---
!define MUI_STARTMENUPAGE_TEXT_TOP "Choose a Start Menu folder for the Parzi shortcut."

; --- Progress tail ---
!define MUI_INSTFILESPAGE_FINISHHEADER_TEXT "Files are in place"
!define MUI_INSTFILESPAGE_FINISHHEADER_SUBTEXT "Press Next to wrap up."

; --- Finish ---
!define MUI_FINISHPAGE_TITLE "Parzi is installed"
!define MUI_FINISHPAGE_TEXT "You are set. Launch Parzi and ask it something."
!define MUI_FINISHPAGE_RUN_TEXT "Launch Parzi now"

; --- Uninstall confirm ---
!define MUI_UNCONFIRMPAGE_TEXT_TOP "This removes Parzi from your machine. Your threads and settings stay unless you tick the box below."

; --- Abort guard ---
!define MUI_ABORTWARNING
!define MUI_ABORTWARNING_TEXT "Quit the Parzi installer?"
!define MUI_ABORTWARNING_CANCELDEFAULT
