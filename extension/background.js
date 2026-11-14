// RDM browser integration: hands downloads and detected media to the RDM app
// running on this computer (http://127.0.0.1:<port>).

const DEFAULTS = { port: 9614, capture: true };
const MEDIA_EXT = /\.(m3u8|mp4|m4v|webm|mkv|mov|flv|avi|mp3|m4a|aac|ogg|oga|opus|flac|wav)$/i;
// Pieces of a stream (HLS / DASH fragments) are not useful on their own.
const FRAGMENT = /\.(ts|m4s|m4f|cmfv|cmfa)$|\/(seg|segment|chunk|frag)[-_]?\d+/i;
const MIN_MEDIA_SIZE = 512 * 1024;

// ---------------------------------------------------------------- app link

async function settings() {
  return { ...DEFAULTS, ...(await chrome.storage.local.get(DEFAULTS)) };
}

async function callApp(path, body) {
  const { port } = await settings();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1500);
  try {
    const res = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: body ? "POST" : "GET",
      headers: body ? { "Content-Type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    return await res.json();
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

/** App info, cached for a minute so every download does not wait for it. */
let pingCache = { at: 0, value: null };
async function ping(force = false) {
  if (!force && Date.now() - pingCache.at < 60_000 && pingCache.value) return pingCache.value;
  const value = await callApp("/ping");
  pingCache = { at: Date.now(), value };
  return value;
}

async function cookiesFor(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

async function sendToApp({ url, referer, fileName }) {
  const ok = await callApp("/download", {
    url,
    referer: referer || undefined,
    cookies: (await cookiesFor(url)) || undefined,
    userAgent: navigator.userAgent,
    fileName: fileName || undefined,
  });
  return Boolean(ok && ok.ok);
}

// ------------------------------------------------------ download capture

function extensionOf(name) {
  const clean = (name || "").split(/[?#]/)[0];
  const base = clean.split(/[\\/]/).pop() || "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
}

function baseName(path) {
  return (path || "").split(/[\\/]/).pop();
}

// Downloads we gave back to the browser after RDM failed; never capture them again.
const giveBack = new Set();

chrome.downloads.onCreated.addListener(async (item) => {
  const { capture } = await settings();
  const url = item.finalUrl || item.url;
  if (giveBack.delete(item.url) || giveBack.delete(url)) return;
  if (!capture || item.state !== "in_progress" || !/^https?:/i.test(url)) return;

  const app = await ping();
  if (!app || !app.enabled) return; // RDM not running: the browser keeps the download

  const ext = extensionOf(item.filename) || extensionOf(url);
  if (!app.captureTypes.includes(ext)) return;

  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch {
    return;
  }
  const sent = await sendToApp({ url, referer: item.referrer, fileName: baseName(item.filename) });
  if (!sent) {
    giveBack.add(url);
    chrome.downloads.download({ url });
  }
});

// --------------------------------------------------------- media sniffing

const key = (tabId) => `media:${tabId}`;

async function mediaOf(tabId) {
  const data = await chrome.storage.session.get(key(tabId));
  return data[key(tabId)] || [];
}

async function setMedia(tabId, items) {
  await chrome.storage.session.set({ [key(tabId)]: items });
  const text = items.length ? String(items.length) : "";
  chrome.action.setBadgeText({ tabId, text }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#1e88e5" }).catch(() => {});
}

function header(headers, name) {
  const h = headers.find((x) => x.name.toLowerCase() === name);
  return h ? h.value : "";
}

/** Total size from Content-Range ("bytes 0-99/1234") or Content-Length. */
function sizeOf(headers) {
  const range = header(headers, "content-range").split("/")[1];
  return Number(range) || Number(header(headers, "content-length")) || 0;
}

/** Drops byte-range query parameters players add, so one file = one entry. */
function normalize(url) {
  try {
    const u = new URL(url);
    for (const p of ["range", "rn", "rbuf", "bytestart", "byteend"]) u.searchParams.delete(p);
    return u.toString();
  } catch {
    return url;
  }
}

function classify(url, type) {
  const path = new URL(url).pathname;
  type = type.split(";")[0].trim().toLowerCase();
  if (type.includes("mpegurl") || /\.m3u8$/i.test(path)) return "hls";
  if (FRAGMENT.test(path) || type === "video/mp2t" || type.includes("iso.segment")) return null;
  if (type.startsWith("video/") || type.startsWith("audio/")) return "file";
  if (MEDIA_EXT.test(path) && (type === "" || type === "application/octet-stream")) return "file";
  return null;
}

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode >= 400) return;
    const headers = details.responseHeaders || [];
    const kind = classify(details.url, header(headers, "content-type"));
    if (!kind) return;
    const size = sizeOf(headers);
    if (kind === "file" && size && size < MIN_MEDIA_SIZE) return;
    remember(details.tabId, {
      url: normalize(details.url),
      kind,
      size,
      type: header(headers, "content-type").split(";")[0],
    });
  },
  { urls: ["http://*/*", "https://*/*"], types: ["media", "xmlhttprequest", "other", "object"] },
  ["responseHeaders"]
);

// Serialize updates per tab so concurrent requests do not overwrite each other.
const queues = new Map();
function remember(tabId, item) {
  const run = async () => {
    const items = await mediaOf(tabId);
    if (items.some((x) => x.url === item.url)) return;
    items.push(item);
    await setMedia(tabId, items.slice(-30));
    chrome.tabs.sendMessage(tabId, { type: "media-found" }).catch(() => {});
  };
  const next = (queues.get(tabId) || Promise.resolve()).then(run, run);
  queues.set(tabId, next);
}

chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.url) setMedia(tabId, []);
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(key(tabId)));

// ----------------------------------------------------------- context menu

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "rdm-download",
    title: "Download with RDM",
    contexts: ["link", "video", "audio", "image"],
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl;
  if (url && /^https?:/i.test(url)) sendToApp({ url, referer: info.pageUrl || tab?.url });
});

// --------------------------------------------- messages (content / popup)

chrome.runtime.onMessage.addListener((msg, sender, reply) => {
  const tabId = msg.tabId ?? sender.tab?.id;
  (async () => {
    switch (msg.type) {
      case "list":
        return mediaOf(tabId);
      case "download":
        return sendToApp({ url: msg.url, referer: msg.referer || sender.tab?.url, fileName: msg.fileName });
      case "status":
        return { app: await ping(true), settings: await settings() };
      case "set":
        await chrome.storage.local.set(msg.values);
        pingCache = { at: 0, value: null };
        return true;
    }
  })().then(reply);
  return true; // async reply
});
