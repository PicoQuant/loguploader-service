; Luminosa / stable — see common.iss for the shared body.
#define MyProduct "Luminosa"
#define MyProductKey "luminosa"
#define MyChannel "stable"
#ifndef MyBinary
  #define MyBinary "..\..\pquploader-luminosa.exe"
#endif
; v1 fielded Windows service being retired by this migration (spec 001 D1/D2) —
; Solira has no fielded v1, so its wrappers do not define this.
#define MyLegacyServiceName "LumiLogUploadService"
#include "common.iss"
