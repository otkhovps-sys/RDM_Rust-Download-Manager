// Notices video and audio a page loads (per tab, from response headers) and
// decides which of them is "the video" the user most likely wants.

import { isStreamHost, videoSite } from "./sites.js";

const MEDIA_EXT = /\.(m3u8|mpd|mp4|m4v|webm|mkv|mov|flv|avi|mp3|m4a|aac|ogg|oga|opus|flac|wav)$/i;
// Pieces of a stream (HLS / DASH fragments) are useless on their own.
const FRAGMENT = /\.(ts|m4s|m4f|cmfv|cmfa)$|\/(seg|segment|chunk|frag)[-_]?\d+/i;
const MIN_SIZE = 512 * 1024;
const MAX_SEEN = 40;
const MAX_OTHERS = 12;

const key = (tabId) => `media:${tabId}`;

/**
 * What a tab has loaded since its page was opened, oldest first:
 * `[{url, kind: "file"|"hls"|"dash", size, type, at}]`.
 */
async function seen(tabId) {
  const data = await chrome.storage.session.get(key(tabId));
  return data[key(tabId)] || [];
}

async function store(tabId, items) {
  await chrome.storage.session.set({ [key(tabId)]: items });
}

// ------------------------------------------------------------ presentation

function formatSize(bytes) {
  if (!bytes) return "";
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (bytes >= 1024 && i < units.length - 1) {
    bytes /= 1024;
    i++;
  }
  return `${bytes.toFixed(i ? 1 : 0)} ${units[i]}`;
}

function formatDuration(seconds) {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const s = Math.round(seconds);
  const pad = (n) => String(n).padStart(2, "0");
  return s >= 3600 ? `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}` : `${Math.floor(s / 60)}:${pad(s % 60)}`;
}

function formatAge(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  return s < 60 ? `${s} s` : `${Math.round(s / 60)} min`;
}

const hostOf = (url) => {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
};

const fileNameOf = (url) => {
  try {
    return decodeURIComponent(new URL(url).pathname.split("/").pop() || "");
  } catch {
    return "";
  }
};

/** Adds the texts the popup and the page button show: `title` and `detail`. */
function describe(item, media) {
  const facts = [hostOf(item.url)];
  let title;
  switch (item.kind) {
    case "page":
      title = item.site ? `${item.site} video` : "Video on this page";
      facts.push("choose the quality in RDM");
      break;
    case "hls":
    case "dash":
      title = "Video stream";
      facts.push(item.kind.toUpperCase());
      break;
    default:
      title = item.current ? "This video" : fileNameOf(item.url) || "Media file";
      facts.push(formatSize(item.size));
      // Tells apart the clips a feed loads one after another.
      if (!item.current && item.at) facts.push(`loaded ${formatAge(Date.now() - item.at)} ago`);
  }
  if (item.current && media) {
    if (media.width && media.height) facts.push(`${media.width}×${media.height}`);
    facts.push(formatDuration(media.duration));
  }
  return { ...item, title, detail: facts.filter(Boolean).join(" · ") };
}

// --------------------------------------------------------------- selection

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

/**
 * What RDM can download from a tab: `{primary, others}`.
 *
 * `context` describes what the user is looking at:
 *   pageUrl     address of the page (or frame)
 *   links       links around the video, nearest first (feeds such as TikTok
 *               link each video to its own page)
 *   currentSrc  address the video element plays, `media` its size/duration
 *   hasVideo    a video element is there, even if its stream is hidden
 *
 * Order of preference: the video's page on a known site, the file the video
 * element plays, a stream manifest, the page itself for yt-dlp to examine,
 * then anything else that was loaded (newest first).
 */
export async function listFor(tabId, context = {}) {
  const requests = await seen(tabId);
  const pageUrl = /^https?:/i.test(context.pageUrl || "") ? context.pageUrl : "";
  const candidates = [];

  const siteUrl = videoSite(pageUrl) ? pageUrl : (context.links || []).find(videoSite);
  if (siteUrl) candidates.push({ kind: "page", url: siteUrl, site: videoSite(siteUrl) });

  const playing = /^https?:/i.test(context.currentSrc || "") ? normalize(context.currentSrc) : "";
  if (playing && !isStreamHost(playing)) {
    const known = requests.find((r) => r.url === playing);
    candidates.push({ ...(known || { kind: "file", url: playing, size: 0 }), current: true });
  }

  // The first manifest a player loads is the master one; later ones from the
  // same host are its quality variants.
  const streamHosts = new Set();
  const streams = requests.filter((r) => r.kind !== "file" && !streamHosts.has(hostOf(r.url)) && streamHosts.add(hostOf(r.url)));
  candidates.push(...streams);

  // A player that hides its stream: let yt-dlp examine the page. Without a
  // video in sight (toolbar popup) loaded files are the better guess.
  const page = pageUrl && !siteUrl ? { kind: "page", url: pageUrl, site: "" } : null;
  if (page && context.hasVideo) candidates.push(page);

  const listed = new Set(candidates.map((c) => c.url));
  const rest = requests.filter((r) => !listed.has(r.url)).sort((a, b) => b.at - a.at);
  candidates.push(...rest);
  if (page && !context.hasVideo && context.offerPage) candidates.push(page);

  const [primary, ...others] = candidates.map((c) => describe(c, context.media));
  return { primary: primary || null, others: others.slice(0, MAX_OTHERS) };
}

// ---------------------------------------------------------------- sniffing

function header(headers, name) {
  const found = headers.find((h) => h.name.toLowerCase() === name);
  return found ? found.value : "";
}

/** Total size from Content-Range ("bytes 0-99/1234") or Content-Length. */
function sizeOf(headers) {
  const total = header(headers, "content-range").split("/")[1];
  return Number(total) || Number(header(headers, "content-length")) || 0;
}

function classify(url, contentType) {
  const path = new URL(url).pathname;
  const type = contentType.split(";")[0].trim().toLowerCase();
  if (type.includes("mpegurl") || /\.m3u8$/i.test(path)) return "hls";
  if (type.includes("dash+xml") || /\.mpd$/i.test(path)) return "dash";
  if (FRAGMENT.test(path) || type === "video/mp2t" || type.includes("iso.segment")) return null;
  if (type.startsWith("video/") || type.startsWith("audio/")) return "file";
  if (MEDIA_EXT.test(path) && (type === "" || type === "application/octet-stream")) return "file";
  return null;
}

async function updateBadge(tabId, pageUrl) {
  const { primary, others } = await listFor(tabId, { pageUrl });
  const count = (primary ? 1 : 0) + others.length;
  chrome.action.setBadgeText({ tabId, text: count ? (count > 9 ? "9+" : String(count)) : "" }).catch(() => {});
  chrome.action.setBadgeBackgroundColor({ tabId, color: "#1e88e5" }).catch(() => {});
}

// Changes are queued per tab, in the order the browser reported them, so
// parallel requests do not overwrite each other.
const queues = new Map();
function enqueue(tabId, change) {
  const run = async () => {
    await store(tabId, change(await seen(tabId)).slice(-MAX_SEEN));
    const tab = await chrome.tabs.get(tabId).catch(() => null);
    if (tab) await updateBadge(tabId, tab.url);
  };
  queues.set(tabId, (queues.get(tabId) || Promise.resolve()).then(run, run));
}

function remember(tabId, item) {
  enqueue(tabId, (items) => (items.some((x) => x.url === item.url) ? items : [...items, item]));
}

const forget = (tabId) => enqueue(tabId, () => []);

chrome.webRequest.onHeadersReceived.addListener(
  (details) => {
    if (details.tabId < 0 || details.statusCode >= 400 || isStreamHost(details.url)) return;
    const headers = details.responseHeaders || [];
    const type = header(headers, "content-type");
    const kind = classify(details.url, type);
    if (!kind) return;
    const size = sizeOf(headers);
    if (kind === "file" && size && size < MIN_SIZE) return;
    remember(details.tabId, { url: normalize(details.url), kind, size, type: type.split(";")[0], at: Date.now() });
  },
  { urls: ["http://*/*", "https://*/*"], types: ["media", "xmlhttprequest", "other", "object"] },
  ["responseHeaders"]
);

// A new page starts a fresh list. The request for the page itself arrives
// through the same event stream as its media, so the order is reliable;
// single-page apps (YouTube, TikTok) change the address without a request.
chrome.webRequest.onBeforeRequest.addListener(
  (details) => {
    if (details.tabId >= 0) forget(details.tabId);
  },
  { urls: ["http://*/*", "https://*/*"], types: ["main_frame"] }
);
chrome.webNavigation.onHistoryStateUpdated.addListener((details) => {
  if (details.frameId === 0) forget(details.tabId);
});
chrome.tabs.onRemoved.addListener((tabId) => {
  queues.delete(tabId);
  chrome.storage.session.remove(key(tabId));
});
