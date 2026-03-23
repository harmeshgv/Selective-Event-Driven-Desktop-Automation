## SEDA Agent (Python port)

This folder contains a Python implementation of SEDA as a **native Windows desktop app** with:

- Start Session
- Stop Session
- Repeated-task suggestions

### Run (PowerShell) – Qt desktop app (recommended)

```powershell
cd .\seda-agent-py
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -U pip
pip install -e .
python .\seda_qt_app.py
```

You can also launch it via the console script after installing in editable mode:

```powershell
.\.venv\Scripts\seda-qt.exe
```

### Build an installable EXE (PyInstaller, Qt UI)

This produces a **double-clickable** `SEDA.exe` Windows desktop app (no browser UI).

```powershell
cd .\seda-agent-py
.\build.ps1
.\dist\SEDA.exe
```

> Note: The legacy Tkinter UI (`seda_ui.py`) is still available as a fallback during development, but the Qt-based UI is the primary experience going forward.
