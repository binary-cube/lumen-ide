; The Windows installer (Inno Setup 6), made by scripts/build.ps1 -Installer: the app for
; the user who installs it (no administrator: %LOCALAPPDATA%\Programs\<Name>, where the app
; can replace itself when it updates), its Start menu entry (a desktop one if chosen), and
; its entry in Settings › Apps to remove it. The names and the version come from
; brand.toml and Cargo.toml (given with /D by the script).
;
;   ISCC.exe /DAppName=Lumen /DAppId=lumen /DAppVersion=1.0.0-beta.5 /DNumericVersion=1.0.0
;            /DPublisher="Binary Cube" /DUrl=https://github.com/... /DSourceExe=...\lumen.exe
;            /DIconFile=...\app-icon.ico /DOutputDir=...\dist /DOutputName=lumen-...-setup
;            packaging\windows\installer.iss

#ifndef AppName
  #error AppName is given by scripts/build.ps1
#endif

[Setup]
; The app's identity for Windows, fixed for good: a later version installs over it, and
; Settings › Apps knows it as the same one.
AppId={{9A5FDBF2-2130-4CF3-9EDD-28C481073381}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#Url}
AppSupportURL={#Url}/issues
AppUpdatesURL={#Url}/releases
; Windows' version fields take numbers only: the whole version (a beta's too) as text.
VersionInfoVersion={#NumericVersion}
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoTextVersion={#AppVersion}
; For this user only: no administrator, a folder the app can write to (its updates).
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#AppId}.exe
UninstallDisplayName={#AppName}
SetupIconFile={#IconFile}
OutputDir={#OutputDir}
OutputBaseFilename={#OutputName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The app running while it's installed over or removed: asked to close first.
CloseApplications=yes
RestartApplications=no
#ifdef SignTool
SignTool={#SignTool}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppId}.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppId}.exe"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppId}.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppId}.exe"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; Run by the app to update (/VERYSILENT /REOPEN=<session>): the app started again, its
; windows reopened.
Filename: "{app}\{#AppId}.exe"; Parameters: "--reopen-session {code:ReopenId}"; Flags: nowait; Check: ShouldReopen

[UninstallDelete]
; What an update left beside the program (the old one set aside, not removable then).
Type: files; Name: "{app}\{#AppId}.exe.old*"

[Code]
// Run by the app to update: /WAITPID=<the app's process> (waited for: it's closing) and
// /REOPEN=<its session> (the app started after with it).

function ReopenId(Param: String): String;
begin
  Result := ExpandConstant('{param:REOPEN|}');
end;

function ShouldReopen(): Boolean;
begin
  Result := ReopenId('') <> '';
end;

function InitializeSetup(): Boolean;
var
  Pid: Integer;
  Code: Integer;
begin
  Pid := StrToIntDef(ExpandConstant('{param:WAITPID|}'), 0);
  if Pid > 0 then
    Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      '-NoProfile -NonInteractive -Command "Wait-Process -Id ' + IntToStr(Pid) +
      ' -Timeout 30 -ErrorAction SilentlyContinue"',
      '', SW_HIDE, ewWaitUntilTerminated, Code);
  Result := True;
end;
