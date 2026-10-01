; Inno Setup script. Build: iscc /DAppVersion=1.0.0 packaging\windows\dboard.iss  (run from dboard-cross\)
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

[Setup]
AppId={{6F1B7A52-3C0D-4F7E-9B7B-D0A12DB0A4D1}
AppName=dboard
AppVersion={#AppVersion}
AppPublisher=alcolopa
AppPublisherURL=https://github.com/alcolopa/dboard
DefaultDirName={autopf}\dboard
DefaultGroupName=dboard
UninstallDisplayIcon={app}\dboard.exe
SetupIconFile=..\..\assets\icon.ico
OutputDir=..\..\dist
OutputBaseFilename=dboard-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\..\target\release\dboard.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\dboard"; Filename: "{app}\dboard.exe"
Name: "{autodesktop}\dboard"; Filename: "{app}\dboard.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\dboard.exe"; Description: "Launch dboard"; Flags: nowait postinstall skipifsilent
