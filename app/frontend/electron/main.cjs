const { app, BrowserWindow } = require("electron");
const path = require("path");
const { spawn } = require("child_process");
const http = require("http");

const isDev = !app.isPackaged;

let backendProcess = null;
const BACKEND_PORT = 8000;
const BACKEND_URL = `http://127.0.0.1:${BACKEND_PORT}`;

// ---------------------------------------------------------------------------
// Backend lifecycle
// ---------------------------------------------------------------------------

function getBackendExePath() {
  if (isDev) return null;
  return path.join(
    process.resourcesPath,
    "backend-dist",
    "flowpilot-backend",
    "flowpilot-backend.exe"
  );
}

function startBackend() {
  const exePath = getBackendExePath();
  if (!exePath) return;

  console.log("[main] Starting backend:", exePath);
  backendProcess = spawn(exePath, [], {
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, BACKEND_PORT: String(BACKEND_PORT) },
    windowsHide: true,
  });

  backendProcess.stdout.on("data", (data) => {
    console.log("[backend]", data.toString().trim());
  });
  backendProcess.stderr.on("data", (data) => {
    console.error("[backend]", data.toString().trim());
  });
  backendProcess.on("exit", (code) => {
    console.log("[main] Backend exited with code", code);
    backendProcess = null;
  });
}

function stopBackend() {
  if (!backendProcess) return;
  console.log("[main] Stopping backend...");
  try {
    backendProcess.kill("SIGTERM");
  } catch (_) {
    /* already dead */
  }
  backendProcess = null;
}

function waitForBackend(maxWaitMs = 30000) {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    function poll() {
      if (Date.now() - start > maxWaitMs) {
        return reject(new Error("Backend did not start in time"));
      }
      const req = http.get(`${BACKEND_URL}/health`, (res) => {
        if (res.statusCode === 200) return resolve();
        setTimeout(poll, 300);
      });
      req.on("error", () => setTimeout(poll, 300));
      req.end();
    }
    poll();
  });
}

// ---------------------------------------------------------------------------
// Window
// ---------------------------------------------------------------------------

function createWindow() {
  const win = new BrowserWindow({
    width: 1360,
    height: 860,
    minWidth: 1100,
    minHeight: 720,
    backgroundColor: "#0b1220",
    autoHideMenuBar: true,
    title: "FlowPilot",
    show: false,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  if (isDev) {
    win.loadURL(process.env.ELECTRON_START_URL || "http://localhost:5173");
    win.once("ready-to-show", () => win.show());
  } else {
    win.loadFile(path.join(__dirname, "..", "dist", "index.html"));
    win.once("ready-to-show", () => win.show());
  }

  return win;
}

// ---------------------------------------------------------------------------
// App lifecycle
// ---------------------------------------------------------------------------

app.whenReady().then(async () => {
  if (!isDev) {
    startBackend();
    try {
      await waitForBackend();
      console.log("[main] Backend is healthy");
    } catch (err) {
      console.error("[main]", err.message);
    }
  }

  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  stopBackend();
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  stopBackend();
});
