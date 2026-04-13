const { contextBridge } = require("electron");

contextBridge.exposeInMainWorld("flowpilotDesktop", {
  platform: process.platform,
  electron: true,
});
