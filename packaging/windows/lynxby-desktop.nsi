; NSIS installer for Lynxby, built by .github/workflows/build.yml.

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef PRODUCT_VERSION
  !define PRODUCT_VERSION "0.1.0.0"
!endif
!ifndef SOURCE
  !error "SOURCE must point at the built lynxby-desktop.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "lynxby-desktop-windows-x86_64-setup.exe"
!endif

Unicode true
Name "Lynxby"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Lynxby"
InstallDirRegKey HKCU "Software\Lynxby\Desktop" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma

VIProductVersion "${PRODUCT_VERSION}"
VIAddVersionKey "ProductName" "Lynxby"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "Lynxby"
VIAddVersionKey "FileDescription" "Lynxby desktop installer"

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Lynxby" SecInstall
  SetOutPath "$INSTDIR"
  File "${SOURCE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateShortcut "$SMPROGRAMS\Lynxby.lnk" "$INSTDIR\lynxby-desktop.exe"
  CreateShortcut "$DESKTOP\Lynxby.lnk" "$INSTDIR\lynxby-desktop.exe"

  WriteRegStr HKCU "Software\Lynxby\Desktop" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "DisplayName" "Lynxby"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "Publisher" "Lynxby"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "DisplayIcon" "$INSTDIR\lynxby-desktop.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\lynxby-desktop.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Lynxby.lnk"
  Delete "$DESKTOP\Lynxby.lnk"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Lynxby"
  DeleteRegKey HKCU "Software\Lynxby\Desktop"
SectionEnd
