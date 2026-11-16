// Requests from the content script (video button) and the popup.

import { captureEnabled, getSettings, ping, sendToApp, setSettings } from "./app.js";
import { listFor } from "./media.js";

const handlers = {
  /** Downloadable items of a tab. */
  list: (msg, sender) => listFor(msg.tabId ?? sender.tab?.id, msg.pageUrl || sender.tab?.url, Boolean(msg.hasVideo)),

  download: (msg, sender) =>
    sendToApp({
      url: msg.url,
      page: Boolean(msg.page),
      referer: msg.referer || sender.tab?.url,
      fileName: msg.fileName,
    }),

  /** A magnet link was clicked: take it only while RDM handles downloads. */
  magnet: async (msg, sender) =>
    (await captureEnabled()) && sendToApp({ url: msg.url, referer: sender.tab?.url }),

  status: async () => ({ app: await ping(true), settings: await getSettings() }),

  set: async (msg) => {
    await setSettings(msg.values);
    return true;
  },
};

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  const handler = handlers[msg?.type];
  if (!handler) return false;
  Promise.resolve(handler(msg, sender)).then(reply, () => reply(null));
  return true; // the reply is sent asynchronously
});
