// Hands browser downloads of the chosen file types over to RDM.

import { captureEnabled, ping, sendToApp } from "./app.js";

function extensionOf(name) {
  const base = (name || "").split(/[?#]/)[0].split(/[\\/]/).pop() || "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

const baseName = (path) => (path || "").split(/[\\/]/).pop();

// Downloads given back to the browser after RDM failed; never taken again.
const givenBack = new Set();

chrome.downloads.onCreated.addListener(async (item) => {
  const url = item.finalUrl || item.url;
  if (givenBack.delete(item.url) || givenBack.delete(url)) return;
  if (item.state !== "in_progress" || !/^https?:/i.test(url)) return;
  if (!(await captureEnabled())) return; // RDM not running: the browser keeps it

  const { captureTypes } = await ping();
  const ext = extensionOf(item.filename) || extensionOf(url);
  if (!captureTypes.includes(ext)) return;

  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    return;
  }
  const sent = await sendToApp({ url, referer: item.referrer, fileName: baseName(item.filename) });
  if (!sent) {
    givenBack.add(url);
    chrome.downloads.download({ url });
  }
});
