const { app, BrowserWindow, ipcMain } = require('electron');
const fs = require('node:fs');
const output = process.env.VOICE_SCRATCH_OUTPUT;
if (!output) throw new Error('VOICE_SCRATCH_OUTPUT is required');
fs.writeFileSync(output, '');
app.setName('voice-electron-scratch');
app.whenReady().then(() => {
  const window = new BrowserWindow({ width: 640, height: 400, title: 'voice Electron scratch', webPreferences: { nodeIntegration: true, contextIsolation: false } });
  ipcMain.on('text', (_, text) => fs.writeFileSync(output, text));
  ipcMain.on('key', (_, key) => fs.appendFileSync(output.replace(/\.txt$/, '') + '.keys', key + '\n'));
  window.webContents.on('did-finish-load', () => fs.writeFileSync(output.replace(/\.txt$/, '') + '.ready', ''));
  window.loadURL('data:text/html,' + encodeURIComponent(`<textarea autofocus style="width:95%;height:90%"></textarea><script>
    const {ipcRenderer} = require('electron');
    const field = document.querySelector('textarea');
    field.addEventListener('input', () => ipcRenderer.send('text', field.value));
    field.addEventListener('keydown', event => ipcRenderer.send('key', event.key));
  </script>`));
});
app.on('window-all-closed', () => app.quit());
