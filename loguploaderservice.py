import time
import win32serviceutil  # ServiceFramework and commandline helper
import win32service  # Events
import servicemanager  # Simple setup and logging
import loguploader
import sys
import win32timezone
try:
    import settings  # type: ignore
except Exception:
    from types import SimpleNamespace

    settings = SimpleNamespace()
import pywintypes


class LumiLogUploadService:
    """Luminosa Log Upload Service"""

    def stop(self):
        """Stop the service"""
        self.running = False

    def run(self):
        """Main service loop. This is where work is done!"""
        self.running = True
        interval = getattr(settings, "service_interval_seconds", 300)
        while self.running:
            try:
                servicemanager.LogInfoMsg("Service running...")
                [defaultDir, serialnumber, currentMachineID] = loguploader.init()
                servicemanager.LogInfoMsg(f"Log Directory: {defaultDir}")
                servicemanager.LogInfoMsg(f"System Serial Number: {serialnumber}")
                servicemanager.LogInfoMsg(f"ID: {currentMachineID}")

                rtn = loguploader.copyDB(basepath=defaultDir)
                servicemanager.LogInfoMsg(rtn)

                rtn = loguploader.uploadSettings(
                    basepath=defaultDir,
                    serialnumber=serialnumber,
                    current_machine_id=currentMachineID,
                )
                servicemanager.LogInfoMsg(rtn)
                any_uploaded = loguploader.did_any_upload_succeed(rtn)

                rtn = loguploader.uploadUserSettings(
                    basepath=defaultDir,
                    serialnumber=serialnumber,
                    current_machine_id=currentMachineID,
                )
                servicemanager.LogInfoMsg(rtn)
                any_uploaded = any_uploaded or loguploader.did_any_upload_succeed(rtn)

                rtn = loguploader.uploadLaserPowerLog(
                    basepath=defaultDir,
                    serialnumber=serialnumber,
                    current_machine_id=currentMachineID,
                )
                servicemanager.LogInfoMsg(rtn)
                any_uploaded = any_uploaded or loguploader.did_any_upload_succeed(rtn)

                rtn = loguploader.uploadlog(
                    basepath=defaultDir,
                    serialnumber=serialnumber,
                    current_machine_id=currentMachineID,
                )
                servicemanager.LogInfoMsg(rtn)
                any_uploaded = any_uploaded or loguploader.did_any_upload_succeed(rtn)

                if any_uploaded:
                    rtn = loguploader.upload_client_version_if_needed(
                        serialnumber=serialnumber,
                        current_machine_id=currentMachineID,
                    )
                    servicemanager.LogInfoMsg(rtn)
            except Exception as e:
                # Never crash the service loop; log and continue next cycle
                try:
                    servicemanager.LogErrorMsg(f"Service loop error: {e}")
                except Exception:
                    pass

            # Sleep in small steps so stop() is responsive
            slept = 0
            while self.running and slept < interval:
                time.sleep(1)
                slept += 1


class LumiLogUploadServiceFramework(win32serviceutil.ServiceFramework):
    _svc_name_ = "LumiLogUploadService"
    _svc_display_name_ = "Luminosa Log Upload Service"
    _svc_description_ = (
        "Uploads Luminosa logs and settings to Nextcloud periodically with retry and size checks."
    )

    def SvcStop(self):
        """Stop the service"""
        self.ReportServiceStatus(win32service.SERVICE_STOP_PENDING)
        self.service_impl.stop()
        self.ReportServiceStatus(win32service.SERVICE_STOPPED)

    def SvcDoRun(self):
        """Start the service; does not return until stopped"""
        self.ReportServiceStatus(win32service.SERVICE_START_PENDING)
        self.service_impl = LumiLogUploadService()
        self.ReportServiceStatus(win32service.SERVICE_RUNNING)
        # Run the service
        self.service_impl.run()


def init():
    if len(sys.argv) == 1:
        servicemanager.Initialize()
        servicemanager.PrepareToHostSingle(LumiLogUploadServiceFramework)
        try:
            servicemanager.StartServiceCtrlDispatcher()
            servicemanager.LogInfoMsg("Loguploader Service started")
        except pywintypes.error as e:
            # (1063, 'StartServiceCtrlDispatcher', ...) happens when started from console
            # instead of by the Windows Service Control Manager.
            if e.args and e.args[0] == 1063:
                sys.stderr.write(
                    "This executable is a Windows Service and must be started by the Service Control Manager.\n"
                    "Use one of these (as Administrator):\n"
                    "  loguploaderservice.exe install\n"
                    "  loguploaderservice.exe start\n"
                    "Or for console debug:\n"
                    "  loguploaderservice.exe debug\n"
                )
                return
            raise
    else:
        win32serviceutil.HandleCommandLine(LumiLogUploadServiceFramework)


if __name__ == "__main__":
    init()
