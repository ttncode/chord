; Inno Setup 6 script for chord. Build (from the repository root, after `cargo build --release`):
;   iscc /DVersion=0.2.0 installer\chord.iss
; Output: target\installer\chord-setup-<Version>.exe

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
; Never change AppId: upgrades and the uninstaller find the installed copy by it.
AppId={{6B2E9C41-3F7A-4D8B-9E51-0C7A2D9F4E13}
AppName=chord
AppVersion={#Version}
AppPublisher=ttncode
AppPublisherURL=https://github.com/ttncode/chord
AppSupportURL=https://github.com/ttncode/chord/issues
DefaultDirName={localappdata}\Programs\chord
DisableProgramGroupPage=yes
; Per-user install: no admin prompt, and it can only touch this user's files and registry.
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=chord-setup-{#Version}
SetupIconFile=..\assets\chord.ico
UninstallDisplayIcon={app}\chord.exe
UninstallDisplayName=chord
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
; chord is closed by PrepareToInstall below; the Restart Manager can't close a tray-only app.
CloseApplications=no

[Tasks]
Name: startup; Description: "Start chord when I sign in to Windows"

[Files]
Source: "..\target\release\chord.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\chord"; Filename: "{app}\chord.exe"; WorkingDir: "{app}"

[Registry]
; Same value the tray's "Start with Windows" writes, so the two stay in sync.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "chord"; ValueData: """{app}\chord.exe"" --background"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\chord.exe"; Description: "Launch chord now"; Flags: nowait postinstall skipifsilent
; Updates from the tray run the installer with /SILENT: start chord again afterwards.
Filename: "{app}\chord.exe"; Parameters: "--background"; Flags: nowait; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM chord.exe"; Flags: runhidden; RunOnceId: "StopChord"

[UninstallDelete]
; A --debug-hints dump chord may have written next to itself. Chord files in %APPDATA%\chord are kept.
Type: files; Name: "{app}\hints-debug.txt"

[Code]
// A running chord holds chord.exe open; stop it so the new version can replace it.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM chord.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := '';
end;

// The tray's "Start with Windows" can write the Run value even when the startup task was unticked.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'chord');
end;
