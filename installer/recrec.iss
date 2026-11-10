; Inno Setup script for recrec. Built by .github/workflows/release.yml:
;   ISCC.exe /DAppVersion=0.1.0 installer/recrec.iss

#ifndef AppVersion
  #error AppVersion must be defined (/DAppVersion=x.y.z)
#endif

[Setup]
AppId={{6F1E3C52-8A4B-4B8E-9C1D-2E7A5B3F9D10}
AppName=recrec
AppVersion={#AppVersion}
AppVerName=recrec {#AppVersion}
AppPublisher=recrec contributors
AppPublisherURL=https://github.com/liasvistreamgamemusic-sketch/recrec
AppSupportURL=https://github.com/liasvistreamgamemusic-sketch/recrec/issues
DefaultDirName={autopf}\recrec
DefaultGroupName=recrec
DisableProgramGroupPage=yes
; Per-user install by default: no administrator rights needed.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=recrec-{#AppVersion}-setup
SetupIconFile=..\assets\recrec.ico
UninstallDisplayIcon={app}\recrec.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\recrec.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\recrec"; Filename: "{app}\recrec.exe"
Name: "{group}\{cm:UninstallProgram,recrec}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\recrec"; Filename: "{app}\recrec.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\recrec.exe"; Description: "{cm:LaunchProgram,recrec}"; Flags: nowait postinstall skipifsilent
