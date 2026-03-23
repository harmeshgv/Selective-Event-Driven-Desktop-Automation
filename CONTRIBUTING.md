# Contributing

Thanks for contributing to SEDA.

## Requirements

- Windows 10/11.
- Python 3.10+.

## Setup

```powershell
cd .\seda-agent-py
python -m venv .venv
.\.venv\Scripts\Activate.ps1
pip install -U pip
pip install -e .
```

## Local Development Workflow

1. Create a branch from `main`.
2. Make focused changes.
3. Run checks:

```powershell
cd .\seda-agent-py
.\.venv\Scripts\python -m pip install -e .
.\.venv\Scripts\python -m compileall .\src
```

4. Update docs when behavior or APIs change.
5. Open a pull request.
6. Include what changed, why it changed, and how it was tested.

## Scope and Safety Expectations

- Keep processing local-first.
- Avoid introducing raw sensitive data storage unless explicitly required.
- Preserve localhost-only serving for local APIs unless a change is intentional and documented.
- Keep safety validation for automation endpoints.

## Coding Notes

- Keep modules small and purpose-specific.
- Prefer explicit error messages over silent failures.
- Add tests for behavior changes when practical.
