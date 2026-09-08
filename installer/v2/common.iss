; Shared Inno Setup body for the v2 agent. Included by the 4 per-(product,channel) wrappers.
;
; Each wrapper defines, before including this file:
;   MyProduct       "Luminosa" | "Solira"
;   MyProductKey    "luminosa" | "solira"
;   MyChannel       "stable"   | "beta"
;   MyBinary        source path to the built exe (pquploader-<product>[-beta].exe)
;
; Version comes from the repo VERSION file, passed on the ISCC command line:
;   ISCC /DMyAppVersion=<x.y.z> installer\v2\luminosa.iss
; (falls back to 0.0.0 for a local smoke build).

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

#define MyChannelSuffixSpace (MyChannel == "beta" ? " Beta" : "")
#define MyChannelSuffixDash  (MyChannel == "beta" ? "-beta" : "")
#define MyServiceName "PQUploader" + MyProduct
#define MyDisplayName "PicoQuant " + MyProduct + " Log Uploader"
#define MyExeName "pquploader.exe"
; keeps the "...Setup.exe" token the v1 updater's *Setup*.exe glob matches (Constitution VI)
#define MyOutputBase MyProduct + " Log Uploader" + MyChannelSuffixSpace + " Setup"
; v1 fielded task for Luminosa is exactly \PicoQuant\LuminosaLogUploader\AutoUpdate
#define MyTaskName "\PicoQuant\" + MyProduct + "LogUploader\AutoUpdate"
#define MyDataDir "{commonappdata}\PicoQuant\" + MyProduct

[Setup]
AppId={{EC5738FF-E229-4BB2-9438-ACD2BD11AAC8}
AppName={#MyDisplayName}
AppVersion={#MyAppVersion}
AppPublisher=PicoQuant Support
AppPublisherURL=https://support.picoquant.com
AppSupportURL=https://support.picoquant.com
AppUpdatesURL=https://support.picoquant.com
DefaultDirName={autopf}\{#MyDisplayName}
DefaultGroupName={#MyDisplayName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
OutputBaseFilename={#MyOutputBase}
; repo-root\Output (overridable with ISCC /O); {#SourcePath} = this script's dir
OutputDir={#SourcePath}..\..\Output
Compression=lzma
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#MyBinary}";            DestDir: "{app}"; DestName: "{#MyExeName}"; Flags: ignoreversion
Source: "..\..\VERSION";          DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\config.toml.example"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\updater\update.ps1";  DestDir: "{app}\updater"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyDisplayName}"; Filename: "{app}\{#MyExeName}"

[Run]
Filename: "{app}\{#MyExeName}"; Parameters: "install"; Flags: runhidden waituntilterminated
Filename: "{sys}\sc.exe"; Parameters: "start {#MyServiceName}"; Flags: runhidden; StatusMsg: "Starting service..."

[UninstallRun]
Filename: "{app}\{#MyExeName}"; Parameters: "uninstall"; Flags: runhidden; RunOnceId: "RemoveService"

[Code]
function EnsureDir(const Dir: string): Boolean;
begin
  if DirExists(Dir) then Result := True else Result := ForceDirectories(Dir);
end;

procedure WriteTaskLog(const S: string);
var LogDir, LogPath: string;
begin
  LogDir := ExpandConstant('{#MyDataDir}\update');
  EnsureDir(LogDir);
  LogPath := LogDir + '\installer_task.log';
  SaveStringToFile(LogPath, S + #13#10, True);
end;

function CreateAutoUpdateTask(): Boolean;
var ResultCode: Integer; Tr, Params: string; Started: Boolean;
begin
  Tr := '\"powershell.exe\" -NoProfile -ExecutionPolicy Bypass -File \"' + ExpandConstant('{app}\updater\update.ps1') + '\"';
  Params := '/Create /F /RL HIGHEST /RU SYSTEM /SC ONSTART /DELAY 0000:30 /TN "{#MyTaskName}" /TR "' + Tr + '"';
  WriteTaskLog('Creating task: ' + Params);
  Started := Exec(ExpandConstant('{sys}\schtasks.exe'), Params, '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  WriteTaskLog('schtasks started=' + IntToStr(Ord(Started)) + ' exit=' + IntToStr(ResultCode));
  Result := Started and (ResultCode = 0);
end;

function DeleteAutoUpdateTask(): Boolean;
var ResultCode: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\schtasks.exe'), '/Delete /F /TN "{#MyTaskName}"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := Result and (ResultCode = 0);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    CreateAutoUpdateTask();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    DeleteAutoUpdateTask();
end;
