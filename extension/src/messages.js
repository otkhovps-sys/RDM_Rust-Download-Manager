// Requests from the content script (video button) and the popup.

import { captureEnabled, getSettings, ping, sendToApp, setSettings } from "./app.js";
import { listFor } from "./media.js";

const handlers = {
  /** What a tab offers: `{primary, others}` (see media.js). */
  list: (msg, sender) =>
    listFor(msg.tabId ?? sender.tab?.id, { pageUrl: sender.tab?.url, ...msg.context }),

  /** Sends one entry of that list to RDM. */
  download: (msg, sender) =>
    sendToApp({
      url: msg.item.url,
      page: msg.item.kind === "page",
      referer: msg.referer || sender.tab?.url,
      fileName: msg.title,
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
