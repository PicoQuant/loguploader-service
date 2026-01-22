from cx_Freeze import setup, Executable


def _read_version() -> str:
    with open("VERSION", "r", encoding="utf-8") as f:
        return f.read().strip()

bdist_msi_options = {
    #'upgrade_code': '{66620F3A-DC3A-11E2-B341-002219E9B01E}',
    'add_to_path': False,
    'all_users' : True,
    'initial_target_dir': r'[ProgramFilesFolder]\%s\%s' % ("PicoQuant", "Luminosa Log Uploader"),
    'upgrade_code': '{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}'
    }

setup(name = "LumiLogUploaderService" ,
      version = _read_version() ,
      description = "Luminosa Log File Uploader Service" ,
      executables = [Executable("loguploaderservice.py")])