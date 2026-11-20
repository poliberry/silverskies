; Silver Skies Pro Windows installer (Inno Setup 6).
; Built by .github/workflows/release.yml:
;   ISCC.exe /DAppVersion=<version> /O"<repo-root>" /F"SilverSkiesPro-Setup-<version>" apps\pro\dist\windows\installer.iss
; Override the version with /DAppVersion; the default below is only a fallback.
#define AppVersion "0.0.0"

[Setup]
AppName=Silver Skies Pro
AppVerName=Silver Skies Pro {#AppVersion}
AppVersion={#AppVersion}
; Same AppId as the pre-split single app, so existing installs upgrade into Pro.
AppId=com.silverskies.desktop
DefaultDirName={autopf}\Silver Skies Pro
DefaultGroupName=Silver Skies Pro
OutputDir=..\..\..\..
OutputBaseFilename=SilverSkiesPro-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
WizardStyle=modern
; Close a running copy (in-app updates) so its files can be replaced.
CloseApplications=force

[Files]
Source: "..\..\..\..\stage\silverskies-pro.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\..\..\stage\silverskies-globe.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\..\..\..\stage\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Silver Skies Pro"; Filename: "{app}\silverskies-pro.exe"
Name: "{autodesktop}\Silver Skies Pro"; Filename: "{app}\silverskies-pro.exe"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "Create a &desktop shortcut"; Flags: unchecked

[Run]
Filename: "{app}\silverskies-pro.exe"; Description: "Launch Silver Skies Pro"; Flags: nowait postinstall skipifsilent
; In-app updates install silently (/SILENT); start the new version afterwards.
Filename: "{app}\silverskies-pro.exe"; Flags: nowait; Check: WizardSilent
