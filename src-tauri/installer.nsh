!macro WinGlanceMigrateShortcut shortcut phase
  ${If} ${FileExists} "${shortcut}"
    !insertmacro WinGlanceReadShortcut "${shortcut}" "${phase}"
  ${EndIf}
!macroend

!macro WinGlanceReadShortcut shortcut phase
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  StrCpy $R4 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("${shortcut}",${STGM_READWRITE}).r3'
      ${If} $3 = 0
        ${IShellLink::GetPath} $0 '(.r2,${MAX_PATH},0,${SLGP_RAWPATH}).r3'
        ${If} $3 = 0
          ExpandEnvStrings $2 "$2"
          ${GetFileName} "$2" $3
          ${If} $3 == "${MAINBINARYNAME}.exe"
          ${AndIf} $2 != "$INSTDIR\${MAINBINARYNAME}.exe"
            ${GetFileVersion} "$2" $3
            ${IfNot} ${Errors}
              ${VersionCompare} "${VERSIONWITHBUILD}" "$3" $3
              ${If} $3 = 1
                !if "${phase}" == "before"
                  StrCpy $R4 "$2"
                !else
                  ${IShellLink::SetPath} $0 '(w "$INSTDIR\${MAINBINARYNAME}.exe").r3'
                  ${If} $3 = 0
                    ${IShellLink::SetWorkingDirectory} $0 '(w "$INSTDIR")'
                    ${IPersistFile::Save} $1 '("${shortcut}",1)'
                  ${EndIf}
                !endif
              ${EndIf}
            ${EndIf}
          ${EndIf}
        ${EndIf}
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  !if "${phase}" == "before"
    ${If} $R4 != ""
      !insertmacro CheckIfAppIsRunning "$R4" "${PRODUCTNAME}"
    ${EndIf}
  !endif
  ClearErrors
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro WinGlanceMigrateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "before"
  !insertmacro WinGlanceMigrateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "before"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro WinGlanceMigrateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "after"
  !insertmacro WinGlanceMigrateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "after"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "WinGlance"
!macroend
