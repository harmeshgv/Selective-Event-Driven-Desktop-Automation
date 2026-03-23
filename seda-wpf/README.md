## SEDA WPF UI (Windows-native frontend)

This folder contains a **Windows-native WPF desktop UI** that talks to the existing Python FastAPI backend at `http://127.0.0.1:9315`.

### Prereqs

- **Windows 10/11**
- **.NET SDK** (this repo currently targets `net10.0-windows` to match machines with .NET 10 installed)
- Python backend set up in `seda-agent-py/` (venv recommended)

### Run (development)

1) Set up the Python backend once:

```powershell
cd ..\seda-agent-py
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -U pip
pip install -e .
```

2) Build/run the WPF UI (from repo root):

```powershell
cd .\seda-wpf\SEDA.Wpf
dotnet build
dotnet run
```

The WPF app will auto-start the backend if it isn't running. It tries:

- `seda-agent-py\.venv\Scripts\python.exe` (repo-local venv)
- else `python` from PATH
- or set `SEDA_PYTHON` to a full python path.

You can also override the backend URL (if you run it on a different port):

```powershell
$env:SEDA_BACKEND_URL = "http://127.0.0.1:9315"
```

### Logs (backend)

If the backend fails to start or crashes, the WPF app writes backend stdout/stderr to:

- `%LOCALAPPDATA%\\SEDA\\logs\\backend.log`

### Packaging approach (recommended)

Start with **dev-run** until the UI/UX is finalized. After that, ship a single-click Windows installer:

- **MSIX** (best Windows-native install/update experience)
  - Create a packaging project (Windows Application Packaging Project) and produce an `.msix`.
  - Bundle the WPF app as the primary entrypoint.
  - Decide backend distribution:
    - **Option A**: bundle an embeddable Python + wheels + your backend module (single install, offline).
    - **Option B**: require Python installed + `pip install -e .` (simpler, dev-focused).

If your machine uses application control policies (AppLocker/WDAC), MSIX is typically easier to allowlist than unsigned EXEs.

