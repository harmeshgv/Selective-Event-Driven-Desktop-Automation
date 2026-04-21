# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for the FlowPilot backend sidecar.

Build:
    cd app/backend
    pip install pyinstaller
    pyinstaller flowpilot-backend.spec

Output lands in  app/frontend/backend-dist/  so electron-builder can bundle it.
"""
import os
from pathlib import Path

BACKEND_DIR = os.path.abspath(SPECPATH)
APP_DIR = os.path.dirname(BACKEND_DIR)
REPO_ROOT = os.path.dirname(APP_DIR)

block_cipher = None

a = Analysis(
    [os.path.join(BACKEND_DIR, 'main.py')],
    pathex=[
        os.path.join(BACKEND_DIR, 'src'),
        os.path.join(APP_DIR, 'ai', 'src'),
        os.path.join(APP_DIR, 'automation', 'src'),
        os.path.join(APP_DIR, 'observer', 'src'),
    ],
    binaries=[],
    datas=[
        (os.path.join(REPO_ROOT, '.env.example'), '.'),
    ],
    hiddenimports=[
        # FastAPI / Uvicorn
        'uvicorn',
        'uvicorn.logging',
        'uvicorn.loops',
        'uvicorn.loops.auto',
        'uvicorn.protocols',
        'uvicorn.protocols.http',
        'uvicorn.protocols.http.auto',
        'uvicorn.protocols.websockets',
        'uvicorn.protocols.websockets.auto',
        'uvicorn.lifespan',
        'uvicorn.lifespan.on',
        'uvicorn.lifespan.off',
        # SQLAlchemy dialects
        'sqlalchemy.dialects.sqlite',
        'sqlalchemy.dialects.postgresql',
        'sqlalchemy.dialects.postgresql.psycopg2',
        # Pydantic
        'pydantic',
        'pydantic_settings',
        # App packages
        'backend',
        'backend.main',
        'backend.core.config',
        'backend.db.models',
        'backend.db.session',
        'backend.api.routes.logs',
        'backend.api.routes.tasks',
        'backend.api.routes.automations',
        'backend.api.routes.run',
        'backend.api.routes.observer_settings',
        'backend.api.routes.user_config',
        'backend.services.task_discovery',
        'backend.services.task_explanations',
        'backend.services.automation_planning',
        'backend.services.sample_seed',
        'ai',
        'ai.llm_executor',
        'ai.task_explainer',
        'ai.planner',
        'ai.models',
        'automation',
        'automation.engine',
        # Observer (runs in-process in packaged exe)
        'observer',
        'observer.main',
        'observer.collector',
        'observer.config',
        'observer.http_client',
        'observer.models',
        # Observer dependencies
        'pynput',
        'pynput.keyboard',
        'pynput.mouse',
        'mss',
        'mss.tools',
        'httpx',
        'pyautogui',
        # Stdlib used dynamically
        'multiprocessing',
        'email.mime.text',
        'ctypes',
        'ctypes.wintypes',
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=['tkinter', 'matplotlib', 'numpy', 'PIL', 'cv2'],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name='flowpilot-backend',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,
)

DIST_DIR = os.path.join(APP_DIR, 'frontend', 'backend-dist')

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name='flowpilot-backend',
    distpath=DIST_DIR,
)
