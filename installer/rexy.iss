; Rexy's installer, built by scripts/package.ps1, which passes the version and
; the folder the release was staged in. It installs for the user alone, with
; no administrator.
#ifndef AppVersion
  #error Build this with scripts/package.ps1, which gives the version.
#endif
#ifndef Stage
  #error Build this with scripts/package.ps1, which gives the staged release.
#endif

[Setup]
; Never changed: Windows knows the installed program by it.
AppId={{31E364CC-776A-4D97-ABB8-643D35467740}
AppName=Rexy
AppVersion={#AppVersion}
AppVerName=Rexy {#AppVersion}
AppComments=A minimal native client for Rex
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\Rexy
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\assets\rexy.ico
UninstallDisplayIcon={app}\rexy.exe
UninstallDisplayName=Rexy
OutputDir=..\dist
OutputBaseFilename=rexy-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; A running Rexy is closed to be replaced, and not started again by the installer.
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

; Windows shows a program's toasts only by the name its shortcut gives it.
[Icons]
Name: "{autoprograms}\Rexy"; Filename: "{app}\rexy.exe"; AppUserModelID: "dev.rexy.client"
Name: "{autodesktop}\Rexy"; Filename: "{app}\rexy.exe"; AppUserModelID: "dev.rexy.client"; Tasks: desktopicon

; Rexy writes its name and icon here for its toasts. Taken out with the program.
[Registry]
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\dev.rexy.client"; ValueType: none; Flags: uninsdeletekey

[Run]
Filename: "{app}\rexy.exe"; Description: "{cm:LaunchProgram,Rexy}"; Flags: nowait postinstall skipifsilent

; The icon its toasts are drawn with. What else is there is the user's own, and is kept.
[UninstallDelete]
Type: files; Name: "{localappdata}\rexy\icon.png"
Type: dirifempty; Name: "{localappdata}\rexy"
