; ForgeDock installer (Inno Setup 6). Per-user: no administrator rights needed.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{4D9A2C67-E1B5-4A83-9F20-6C3B8D17A5E4}
AppName=ForgeDock
AppVersion={#AppVersion}
AppVerName=ForgeDock {#AppVersion}
AppPublisher=Zheer Khalid
AppPublisherURL=https://zheer.dev
AppSupportURL=https://github.com/zheerkhalid/forgedock
AppUpdatesURL=https://github.com/zheerkhalid/forgedock/releases
DefaultDirName={autopf}\ForgeDock
DefaultGroupName=ForgeDock
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=ForgeDock-Setup
SetupIconFile=..\target\forgedock.ico
UninstallDisplayIcon={app}\ForgeDock.exe
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
LicenseFile=..\LICENSE
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Zheer Khalid (zheer.dev)
VersionInfoDescription=ForgeDock Setup

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\target\release\ForgeDock.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\ForgeDock"; Filename: "{app}\ForgeDock.exe"
Name: "{group}\Uninstall ForgeDock"; Filename: "{uninstallexe}"
Name: "{autodesktop}\ForgeDock"; Filename: "{app}\ForgeDock.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ForgeDock.exe"; Description: "{cm:LaunchProgram,ForgeDock}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\ForgeDock\cache"
Type: filesandordirs; Name: "{localappdata}\ForgeDock\downloads"
