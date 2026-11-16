// Connection to the RDM app on this computer (http://127.0.0.1, local only).
//
// RDM listens on the first free port of a small fixed range; the extension
// finds it by asking each of them, so there is nothing to configure.

const PORTS = [9614, 9615, 9616, 9617, 9618, 9619, 9620, 9621, 9622, 9623];
const DEFAULTS = { capture: true };
const CACHE_MS = 60_000;
const VERSION = chrome.runtime.getManifest().version;

/** Where RDM was last found: `{at, port, info}`. */
let found = { at: 0, port: null, info: null };

/** @returns {Promise<{capture: boolean}>} */
export async function getSettings() {
  return { ...DEFAULTS, ...(await chrome.storage.local.get(DEFAULTS)) };
}

export async function setSettings(values) {
  await chrome.storage.local.set(values);
}

async function request(port, path, body, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: body ? "POST" : "GET",
      headers: body ? { "Content-Type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    return await response.json();
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

/** Says hello on one port; resolves to RDM's info or rejects. */
async function hello(port) {
  const info = await request(port, `/ping?ext=${VERSION}`, null, 800);
  if (info?.app !== "rdm") throw new Error("not RDM");
  return { port, info };
}

/**
 * App info (`{version, enabled, captureTypes}`) or `null` when RDM is not
 * running. Cached for a minute so every download does not wait for it.
 */
export async function ping(force = false) {
  if (!force && found.info && Date.now() - found.at < CACHE_MS) return found.info;
  // The port used last time first, then the whole range at once.
  const candidates = found.port ? [[found.port], PORTS] : [PORTS];
  for (const ports of candidates) {
    try {
      const { port, info } = await Promise.any(ports.map(hello));
      found = { at: Date.now(), port, info };
      return info;
    } catch {
      // not on these ports
    }
  }
  found = { at: Date.now(), port: null, info: null };
  return null;
}

/** RDM is running and both sides want downloads handed over. */
export async function captureEnabled() {
  const app = await ping();
  return Boolean(app && app.enabled && (await getSettings()).capture);
}

/**
 * The browser's cookies for a page as Netscape `cookies.txt` text. RDM hands
 * it to yt-dlp only if the page cannot be fetched anonymously (private or
 * age-restricted videos, "sign in to continue").
 */
async function cookieFileFor(url) {
  try {
    const cookies = await chrome.cookies.getAll({ url });
    if (!cookies.length) return "";
    const lines = cookies.map((c) => {
      const domain = c.hostOnly || c.domain.startsWith(".") ? c.domain : `.${c.domain}`;
      return [
        (c.httpOnly ? "#HttpOnly_" : "") + domain,
        c.hostOnly ? "FALSE" : "TRUE",
        c.path,
        c.secure ? "TRUE" : "FALSE",
        Math.floor(c.expirationDate || 0),
        c.name,
        c.value,
      ].join("\t");
    });
    return ["# Netscape HTTP Cookie File", ...lines, ""].join("\n");
  } catch {
    return "";
  }
}

async function cookiesFor(url) {
  if (!/^https?:/i.test(url)) return "";
  try {
    const cookies = await chrome.cookies.getAll({ url });
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

/**
 * Opens RDM's "Download File Info" dialog for a URL.
 * @param {{url: string, referer?: string, fileName?: string, page?: boolean}} item
 *   `page: true` means `url` is a web page showing a video (YouTube, ...).
 * @returns {Promise<boolean>} whether RDM accepted it
 */
export async function sendToApp({ url, referer, fileName, page = false }) {
  if (!found.port && !(await ping(true))) return false;
  const body = {
    url,
    page,
    referer: referer || undefined,
    fileName: fileName || undefined,
    // Files and streams get the cookies as a header; video pages get them
    // as a cookie file that is used only when the site demands signing in.
    cookies: page ? undefined : (await cookiesFor(url)) || undefined,
    cookieFile: page ? (await cookieFileFor(url)) || undefined : undefined,
    userAgent: navigator.userAgent,
  };
  let reply = await request(found.port, "/download", body, 1500);
  if (!reply && (await ping(true))) {
    reply = await request(found.port, "/download", body, 1500); // RDM was restarted on another port
  }
  return Boolean(reply && reply.ok);
}

// Say hello as soon as the browser starts or the extension is added, so RDM
// can show "connected" right away.
chrome.runtime.onInstalled.addListener(() => ping(true));
chrome.runtime.onStartup.addListener(() => ping(true));
