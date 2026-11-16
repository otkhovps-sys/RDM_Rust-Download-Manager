// Notices video and audio a page loads, per tab, by watching response headers.

import { isStreamHost, videoSite } from "./sites.js";

const MEDIA_EXT = /\.(m3u8|mp4|m4v|webm|mkv|mov|flv|avi|mp3|m4a|aac|ogg|oga|opus|flac|wav)$/i;
// Pieces of a stream (HLS / DASH fragments) are useless on their own.
const FRAGMENT = /\.(ts|m4s|m4f|cmfv|cmfa)$|\/(seg|segment|chunk|frag)[-_]?\d+/i;
const MIN_SIZE = 512 * 1024;
const MAX_ITEMS = 30;

const key = (tabId) => `media:${tabId}`;

/** Files and streams seen in a tab: `[{url, kind: "file"|"hls", size, type}]`. */
async function seen(tabId) {
  const data = await chrome.storage.session.get(key(tabId));
  return data[key(tabId)] || [];
}

async function store(tabId, items) {
  await chrome.storage.session.set({ [key(tabId)]: items });
}

async function updateBadge(tabId, url) {
  const count = (await listFor(tabId, url, false)).length;
  chrome.action.setBadgeText({ tabId, text: count ? String(count) : "" }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#1e88e5" }).catch(() => {});
}

/**
 * Everything RDM can download from a tab, best choice first.
 * On video sites (and for players that hide their stream, when `hasVideo`)
 * the first entry is the page itself: `{kind: "page", url, site}`.
 */
export async function listFor(tabId, pageUrl, hasVideo) {
  const items = (await seen(tabId)).slice().reverse();
  const site = videoSite(pageUrl || "");
  if (site || (hasVideo && items.length === 0 && /^https?:/i.test(pageUrl || ""))) {
    items.unshift({ kind: "page", url: pageUrl, site: site || "" });
  }
  return items;
}

function header(headers, name) {
  const found = headers.find((h) => h.name.toLowerCase() === name);
  return found ? found.value : "";
}

/** Total size from Content-Range ("bytes 0-99/1234") or Content-Length. */
function sizeOf(headers) {
  const total = header(headers, "content-range").split("/")[1];
  return Number(total) || Number(header(headers, "content-length")) || 0;
}

/** Drops byte-range parameters players add, so one file is one entry. */
function normalize(url) {
  try {
    const u = new URL(url);
    for (const p of ["range", "rn", "rbuf", "bytestart", "byteend"]) u.searchParams.delete(p);
    return u.toString();
  } catch {
    return url;
  }
}

function classify(url, contentType) {
  const path = new URL(url).pathname;
  const type = contentType.split(";")[0].trim().toLowerCase();
  if (type.includes("mpegurl") || /\.m3u8$/i.test(path)) return "hls";
  if (FRAGMENT.test(path) || type === "video/mp2t" || type.includes("iso.segment")) return null;
  if (type.startsWith("video/") || type.startsWith("audio/")) return "file";
  if (MEDIA_EXT.test(path) && (type === "" || type === "application/octet-stream")) return "file";
  return null;
}

// Updates are queued per tab so parallel requests do not overwrite each other.
const queues = new Map();
function remember(tabId, item) {
  const run = async () => {
    const items = await seen(tabId);
    if (items.some((x) => x.url === item.url)) return;
    await store(tabId, [...items, item].slice(-MAX_ITEMS));
    const tab = await chrome.tabs.get(tabId).catch(() => null);
    await updateBadge(tabId, tab?.url);
  };
  queues.set(tabId, (queues.get(tabId) || Promise.resolve()).then(run, run));
}

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode >= 400 || isStreamHost(details.url)) return;
    const headers = details.responseHeaders || [];
    const type = header(headers, "content-type");
    const kind = classify(details.url, type);
    if (!kind) return;
    const size = sizeOf(headers);
    if (kind === "file" && size && size < MIN_SIZE) return;
    remember(details.tabId, { url: normalize(details.url), kind, size, type: type.split(";")[0] });
  },
  { urls: ["http://*/*", "https://*/*"], types: ["media", "xmlhttprequest", "other", "object"] },
  ["responseHeaders"]
);

// A new address in the tab (also inside single-page apps such as YouTube)
// starts a fresh list.
chrome.tabs.onUpdated.addListener(async (tabId, change) => {
  if (!change.url) return;
  await store(tabId, []);
  await updateBadge(tabId, change.url);
});
chrome.tabs.onRemoved.addListener((tabId) => chrome.storage.session.remove(key(tabId)));
