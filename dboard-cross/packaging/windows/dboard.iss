; Inno Setup script. Build: iscc /DAppVersion=2.0.0 /DArch=x86_64|arm64 packaging\windows\dboard.iss  (run from dboard-cross\)
#ifndef Arch
  #define Arch "x86_64"
#endif
#if Arch == "arm64"
  #define ArchAllowed "arm64"
#else
  #define ArchAllowed "x64compatible"
#endif
#ifndef AppVersion
  #define AppVersion "2.0.0"
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
OutputBaseFilename=dboard-{#AppVersion}-windows-{#Arch}-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed={#ArchAllowed}
ArchitecturesInstallIn64BitMode={#ArchAllowed}
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
