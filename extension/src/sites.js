// Video sites: there the page address itself is what RDM downloads (with
// yt-dlp), because the player streams in small encrypted or split pieces.

const SITES = [
  { name: "YouTube", host: /(^|\.)youtube(-nocookie)?\.com$/, path: /^\/(watch|shorts\/|live\/|embed\/|clip\/)/ },
  { name: "YouTube", host: /^youtu\.be$/, path: /^\/[\w-]+/ },
  { name: "Vimeo", host: /(^|\.)vimeo\.com$/, path: /^\/(\d+|video\/\d+|channels\/[^/]+\/\d+)/ },
  { name: "Dailymotion", host: /(^|\.)dailymotion\.com$/, path: /^\/(video|embed\/video)\// },
  { name: "Twitch", host: /(^|\.)twitch\.tv$/, path: /^\/(videos\/\d+|[^/]+\/clip\/)/ },
  { name: "TikTok", host: /(^|\.)tiktok\.com$/, path: /\/video\/\d+/ },
  { name: "Facebook", host: /(^|\.)facebook\.com$/, path: /^\/(watch|reel\/|[^/]+\/videos\/)/ },
  { name: "Instagram", host: /(^|\.)instagram\.com$/, path: /^\/(reel|reels|p|tv)\// },
  { name: "X", host: /(^|\.)(x|twitter)\.com$/, path: /^\/[^/]+\/status\/\d+/ },
  { name: "Bilibili", host: /(^|\.)bilibili\.com$/, path: /^\/video\// },
  { name: "SoundCloud", host: /(^|\.)soundcloud\.com$/, path: /^\/[^/]+\/[^/]+/ },
];

// Hosts that only ever serve stream pieces; never list their requests.
const STREAM_HOSTS = /(^|\.)googlevideo\.com$/;

/** Name of the video site if `url` is one of its video pages, else `null`. */
export function videoSite(url) {
  try {
    const { hostname, pathname } = new URL(url);
    const site = SITES.find((s) => s.host.test(hostname) && s.path.test(pathname));
    return site ? site.name : null;
  } catch {
    return null;
  }
}

export function isStreamHost(url) {
  try {
    return STREAM_HOSTS.test(new URL(url).hostname);
  } catch {
    return false;
  }
}
