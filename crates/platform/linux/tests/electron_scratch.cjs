const { app, BrowserWindow, ipcMain } = require("electron");
const fs = require("node:fs");

const output = process.env.SAID_SCRATCH_OUTPUT;

if (output === undefined || output === "") {
  throw new Error("SAID_SCRATCH_OUTPUT is required");
}

const sibling = (extension) => `${output.replace(/\.txt$/u, "")}${extension}`;

fs.writeFileSync(output, "");

app.setName("said-electron-scratch");

app.on("ready", () => {
  const window = new BrowserWindow({
    width: 640,
    height: 400,
    title: "said Electron scratch",
    webPreferences: { nodeIntegration: true, contextIsolation: false },
  });

  ipcMain.on("text", (_, text) => {
    fs.writeFileSync(output, text);
  });
  ipcMain.on("key", (_, key) => {
    fs.appendFileSync(sibling(".keys"), `${key}\n`);
  });
  window.webContents.on("did-finish-load", () => {
    fs.writeFileSync(sibling(".ready"), "");
  });
  window.loadURL(
    `data:text/html,${encodeURIComponent(`<textarea autofocus style="width:95%;height:90%"></textarea><script>
    const {ipcRenderer} = require('electron');
    const field = document.querySelector('textarea');
    field.addEventListener('input', () => ipcRenderer.send('text', field.value));
    field.addEventListener('keydown', event => ipcRenderer.send('key', event.key));
  </script>`)}`,
  );
});

app.on("window-all-closed", () => {
  app.quit();
});
