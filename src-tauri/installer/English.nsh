; Parzi English overrides for the Tauri NSIS template strings.
; Included AFTER the stock English.nsh, so these win. Keep $vars intact.

LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already here. Pick what to do and press Next to continue."
LangString appRunning ${LANG_ENGLISH} "${PRODUCTNAME} is running. Close it first, then continue."
LangString appRunningOkKill ${LANG_ENGLISH} "${PRODUCTNAME} is running.$\nPress OK and the installer will close it for you."
LangString createDesktop ${LANG_ENGLISH} "Put Parzi on my desktop"
LangString deleteAppData ${LANG_ENGLISH} "Also delete my threads, settings and local data"
LangString failedToKillApp ${LANG_ENGLISH} "Could not close ${PRODUCTNAME}. Close it yourself, then try again."
LangString unableToUninstall ${LANG_ENGLISH} "Could not uninstall - is Parzi still running?"
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall first, then install fresh"
