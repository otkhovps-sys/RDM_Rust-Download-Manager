// Video sites: there the address of the video's page is what RDM downloads
// (with yt-dlp), because the player streams in small or protected pieces.

const SITES = [
  { name: "YouTube", host: /(^|\.)youtube(-nocookie)?\.com$/, path: /^\/(watch|shorts\/|live\/|embed\/|clip\/)/ },
  { name: "YouTube", host: /^youtu\.be$/, path: /^\/[\w-]+/ },
  { name: "TikTok", host: /(^|\.)tiktok\.com$/, path: /\/(video|photo)\/\d+/ },
  { name: "Douyin", host: /(^|\.)douyin\.com$/, path: /^\/video\/\d+/ },
  { name: "Facebook", host: /(^|\.)facebook\.com$/, path: /^\/(watch|reel\/|share\/[rv]\/|[^/]+\/videos\/)/ },
  { name: "Facebook", host: /^fb\.watch$/, path: /^\/.+/ },
  { name: "Instagram", host: /(^|\.)instagram\.com$/, path: /^\/([^/]+\/)?(reel|reels|p|tv)\// },
  { name: "X", host: /(^|\.)(x|twitter)\.com$/, path: /^\/[^/]+\/status\/\d+/ },
  { name: "Vimeo", host: /(^|\.)vimeo\.com$/, path: /^\/(\d+|video\/\d+|channels\/[^/]+\/\d+)/ },
  { name: "Dailymotion", host: /(^|\.)dailymotion\.com$/, path: /^\/(video|embed\/video)\// },
  { name: "Twitch", host: /(^|\.)twitch\.tv$/, path: /^\/(videos\/\d+|[^/]+\/clip\/)/ },
  { name: "Bilibili", host: /(^|\.)bilibili\.com$/, path: /^\/(video|bangumi\/play)\// },
  { name: "Reddit", host: /(^|\.)reddit\.com$/, path: /^\/r\/[^/]+\/comments\// },
  { name: "Pinterest", host: /(^|\.)pinterest\.[a-z.]+$/, path: /^\/pin\// },
  { name: "VK", host: /(^|\.)vk\.com$/, path: /^\/(video|clip)-?\d/ },
  { name: "OK", host: /(^|\.)ok\.ru$/, path: /^\/video\// },
  { name: "Rumble", host: /(^|\.)rumble\.com$/, path: /^\/(v[\w-]+|embed\/)/ },
  { name: "Odysee", host: /(^|\.)odysee\.com$/, path: /^\/@[^/]+\/.+/ },
  { name: "Streamable", host: /(^|\.)streamable\.com$/, path: /^\/[\w-]+$/ },
  { name: "Niconico", host: /(^|\.)nicovideo\.jp$/, path: /^\/watch\// },
  { name: "SoundCloud", host: /(^|\.)soundcloud\.com$/, path: /^\/[^/]+\/[^/]+/ },
  { name: "TED", host: /(^|\.)ted\.com$/, path: /^\/talks\// },
  { name: "Kick", host: /(^|\.)kick\.com$/, path: /^\/([^/]+\/)?(videos|clips)\// },
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
