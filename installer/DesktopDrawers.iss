; Inno Setup script for DesktopDrawers — a per-user, no-admin installer.
;
; Build (after `cargo build --release`):
;   "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\DesktopDrawers.iss
; Output: dist\DesktopDrawers-<version>-setup.exe
;
; This installer:
;   * installs to %LOCALAPPDATA%\Programs\DesktopDrawers (no administrator needed)
;   * creates a Start menu entry and an uninstaller
;   * offers an OPTIONAL desktop shortcut for the manager
;   * does NOT create a startup entry, service, scheduled task, or tray icon

#define AppName "DesktopDrawers"
#define AppVersion "0.1.0"
#define AppExe "DesktopDrawers.exe"
#define AppPublisher "DesktopDrawers"

[Setup]
; A stable AppId keeps upgrades/uninstall associated across versions.
AppId={{4B1E9C0A-2D7F-4C3A-9E2B-7A0D6F1E5C88}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=yes
UsePreviousAppDir=yes
; Per-user install: never request elevation.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename={#AppName}-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
; SetupIconFile=..\assets\DesktopDrawers.ico   ; uncomment once the .ico exists

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut for the manager"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Open the DesktopDrawers manager"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName} now"; Flags: nowait postinstall skipifsilent

; No [Registry] Run key: DesktopDrawers must never auto-start.

[UninstallDelete]
; Remove the disposable icon cache and logs on uninstall, but leave the user's
; drawer configuration in place unless they delete it manually.
Type: filesandordirs; Name: "{localappdata}\{#AppName}\Cache"
Type: filesandordirs; Name: "{localappdata}\{#AppName}\Logs"
