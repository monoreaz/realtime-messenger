; Inno Setup script for Messenger Desktop (per-user install, no admin rights needed).
; Build: iscc /DAppVersion=0.1.0 /DSourceExe=..\target\release\messenger.exe installer\messenger.iss
; Silent update (used by the in-app updater):
;   Messenger-Setup-X.Y.Z.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /UPDATE=1
; User data (%APPDATA%\Messenger, %LOCALAPPDATA%\Messenger) is never touched by (un)install.

#ifndef AppVersion
  #error AppVersion must be defined (/DAppVersion=x.y.z)
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\messenger.exe"
#endif

#define AppName "Messenger"
#define AppExe "messenger.exe"
#define AppUserModelID "monoreaz.Messenger"

[Setup]
; Never change AppId: it links upgrades and the uninstaller to earlier versions.
AppId={{45921240-2F5E-4DA8-AA21-B290287347C8}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=monoreaz
AppPublisherURL=https://github.com/monoreaz/realtime-messenger
AppSupportURL=https://github.com/monoreaz/realtime-messenger/issues
VersionInfoVersion={#AppVersion}
VersionInfoProductVersion={#AppVersion}
VersionInfoDescription={#AppName} installer
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
DisableReadyPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=Output
OutputBaseFilename=Messenger-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Close a running Messenger (Restart Manager) instead of failing on locked files.
CloseApplications=force
RestartApplications=no
SetupMutex=MessengerDesktopSetup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppUserModelID}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; AppUserModelID: "{#AppUserModelID}"; Tasks: desktopicon

[Run]
; Interactive install: offer to launch. Silent in-app update: always relaunch.
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: IsInAppUpdate

[Code]
function IsInAppUpdate: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:UPDATE|0}') = '1');
end;
