; Windows installer for ApexShot, built by scripts/package-windows.sh.
;
; The caller passes the staged app tree, the version, and the output folder as
; /D defines so this file carries no absolute paths and works from a checkout,
; a CI runner, and a release machine alike.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#ifndef SourceDir
  #define SourceDir "..\..\dist\windows\ApexShot"
#endif

#ifndef OutputDir
  #define OutputDir "..\..\dist\windows"
#endif

#ifndef IconFile
  #define IconFile "apexshot.ico"
#endif

#define AppName "ApexShot"
#define AppPublisher "ApexShot"
#define AppUrl "https://apexshot.org"
#define AppIssuesUrl "https://github.com/apex-shot/apexshot/issues"
#define AppReleasesUrl "https://github.com/apex-shot/apexshot/releases"
#define AppGuid "7E3C8F1B-2B4A-4E4C-9C1E-6A5D24A3F9B7"

[Setup]
; A fixed AppId keeps an upgrade in place instead of stacking a second install.
AppId={{{#AppGuid}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppIssuesUrl}
AppUpdatesURL={#AppReleasesUrl}
; The first page of the wizard is the licence, the way a Windows installer for
; GPL software opens.
LicenseFile=..\..\LICENSE
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
; Per-user, like any other Windows app: no elevation prompt, and the uninstall
; entry lands in the user's own Add/Remove Programs.
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename={#AppName}-{#AppVersion}-setup
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\bin\apexshot-capture.exe
UninstallDisplayName={#AppName} {#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Ask a running copy to close so no DLL is replaced under it. The tray shell
; holds the hotkeys; Restart Manager finds it through its windows. No AppMutex:
; its check runs before Restart Manager and stops the upgrade with a "close
; ApexShot first" prompt, which a silent install answers with Cancel.
CloseApplications=yes
RestartApplications=no
MinVersion=10.0

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\bin\apexshot-settings.exe"; WorkingDir: "{app}\bin"; IconFilename: "{app}\bin\apexshot-capture.exe"; Comment: "ApexShot settings - the tray icon starts a capture"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\bin\apexshot-settings.exe"; WorkingDir: "{app}\bin"; IconFilename: "{app}\bin\apexshot-capture.exe"; Tasks: desktopicon

[Run]
; The settings binary owns the first-run flow and hands over to the tray shell
; when setup finishes, so it is the right entry point for a fresh install.
Filename: "{app}\bin\apexshot-settings.exe"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent; Check: not IsUpgrade
; An upgrade closed the tray shell to replace its files. Start it again in the
; background, silent or not, so the hotkeys come back without the user opening
; Settings; onboarding already ran on the first install.
Filename: "{app}\bin\apexshot-capture.exe"; Parameters: "--background"; WorkingDir: "{app}\bin"; Flags: nowait; Check: IsUpgrade

[Code]
var
  Upgrading: Boolean;

// Read before the files are copied: the uninstall key the previous install
// left behind is what tells an upgrade from a fresh install.
function InitializeSetup(): Boolean;
begin
  Upgrading := RegKeyExists(HKA,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#AppGuid}' + '}_is1');
  Result := True;
end;

function IsUpgrade(): Boolean;
begin
  Result := Upgrading;
end;
