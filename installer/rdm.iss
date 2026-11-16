; Windows installer for RDM (Inno Setup 6).
;
; Built by the release workflow:
;   set RDM_VERSION=1.2.0
;   set RDM_SOURCE=<folder with rdm.exe, extension\, README.md, LICENSE, CREDITS.md>
;   ISCC installer\rdm.iss        -> installer\Output\rdm-1.2.0-setup.exe

#define AppVersion GetEnv("RDM_VERSION")
#define Source GetEnv("RDM_SOURCE")

[Setup]
AppId={{B6E2F7A4-6C1D-4E0B-9E55-2D8C5B7A91F3}
AppName=RDM - Rust Download Manager
AppVersion={#AppVersion}
AppPublisher=RDM contributors
AppPublisherURL=https://github.com/otkhovps-sys/rdm
DefaultDirName={autopf}\RDM
DefaultGroupName=RDM
DisableProgramGroupPage=yes
; Installs for the current user without an administrator prompt; the first
; page lets the user choose "for all users" instead.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
SetupIconFile=..\crates\rdm-gui\assets\rdm.ico
UninstallDisplayIcon={app}\rdm.exe
OutputDir=Output
OutputBaseFilename=rdm-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"
Name: "autostart"; Description: "&Start RDM when I sign in to Windows (hidden in the system tray)"; GroupDescription: "Start-up:"; Flags: unchecked
Name: "torrent"; Description: "Open .&torrent files and magnet links with RDM"; GroupDescription: "File types:"; Flags: unchecked

[Files]
Source: "{#Source}\rdm.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Source}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\CREDITS.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\RDM"; Filename: "{app}\rdm.exe"
Name: "{group}\Browser extension (folder)"; Filename: "{app}\extension"
Name: "{group}\Uninstall RDM"; Filename: "{uninstallexe}"
Name: "{autodesktop}\RDM"; Filename: "{app}\rdm.exe"; Tasks: desktopicon

[Registry]
; HKA = current user, or all users when installed with administrator rights.
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "RDM"; ValueData: """{app}\rdm.exe"" --minimized"; Flags: uninsdeletevalue; Tasks: autostart
Root: HKA; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueData: "RDM.torrent"; Flags: uninsdeletevalue; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\RDM.torrent"; ValueType: string; ValueData: "Torrent file"; Flags: uninsdeletekey; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\RDM.torrent\DefaultIcon"; ValueType: string; ValueData: "{app}\rdm.exe,0"; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\RDM.torrent\shell\open\command"; ValueType: string; ValueData: """{app}\rdm.exe"" ""%1"""; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueData: "URL:Magnet link"; Flags: uninsdeletekey; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: torrent
Root: HKA; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueData: """{app}\rdm.exe"" ""%1"""; Tasks: torrent

[Run]
Filename: "{app}\rdm.exe"; Description: "Start RDM"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; RDM keeps running in the tray, which would lock rdm.exe.
Filename: "{cmd}"; Parameters: "/C taskkill /IM rdm.exe /F"; Flags: runhidden; RunOnceId: "StopRDM"

[Code]
// Same when upgrading: stop the running RDM before its files are replaced.
// Unfinished downloads keep their progress and can be resumed afterwards.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{cmd}'), '/C taskkill /IM rdm.exe /F', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := '';
end;
