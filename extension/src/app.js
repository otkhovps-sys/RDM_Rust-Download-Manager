// Connection to the RDM app (http://127.0.0.1:<port>, local machine only).

const DEFAULTS = { port: 9614, capture: true };
const PING_CACHE_MS = 60_000;

let pingCache = { at: 0, value: null };

/** @returns {Promise<{port: number, capture: boolean}>} */
export async function getSettings() {
  return { ...DEFAULTS, ...(await chrome.storage.local.get(DEFAULTS)) };
}

export async function setSettings(values) {
  await chrome.storage.local.set(values);
  pingCache = { at: 0, value: null };
}

async function call(path, body) {
  const { port } = await getSettings();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 1500);
  try {
    const response = await fetch(`http://127.0.0.1:${port}${path}`, {
      method: body ? "POST" : "GET",
      headers: body ? { "Content-Type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    return await response.json();
  } catch {
    return null; // RDM is not running
  } finally {
    clearTimeout(timer);
  }
}

/**
 * App info (`{version, enabled, captureTypes}`) or `null` when RDM is not
 * running. Cached for a minute so every download does not wait for it.
 */
export async function ping(force = false) {
  if (!force && pingCache.value && Date.now() - pingCache.at < PING_CACHE_MS) return pingCache.value;
  const value = await call("/ping");
  pingCache = { at: Date.now(), value };
  return value;
}

/** RDM is running and both sides want downloads handed over. */
export async function captureEnabled() {
  const app = await ping();
  return Boolean(app && app.enabled && (await getSettings()).capture);
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
  const reply = await call("/download", {
    url,
    page,
    referer: referer || undefined,
    fileName: fileName || undefined,
    // Page downloads are fetched by yt-dlp, which does not use these.
    cookies: page ? undefined : (await cookiesFor(url)) || undefined,
    userAgent: navigator.userAgent,
  });
  return Boolean(reply && reply.ok);
}
