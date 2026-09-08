const { app, BrowserWindow } = require('electron');
const path = require('node:path');
app.setName('NaN GUI Spike Electron');
app.commandLine.appendSwitch('force-renderer-accessibility');
app.whenReady().then(() => {
  const window = new BrowserWindow({ width: 600, height: 300 });
  window.loadFile(path.join(__dirname, 'electron.html'));
});
app.on('window-all-closed', () => app.quit());
