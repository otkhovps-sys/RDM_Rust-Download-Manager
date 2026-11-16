// Right-click menu entries.

import { sendToApp } from "./app.js";

const LINK = "rdm-link";
const PAGE = "rdm-page";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({
      id: LINK,
      title: "Download with RDM",
      contexts: ["link", "video", "audio", "image"],
    });
    chrome.contextMenus.create({
      id: PAGE,
      title: "Download video on this page with RDM",
      contexts: ["page", "frame"],
      documentUrlPatterns: ["http://*/*", "https://*/*"],
    });
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === PAGE) {
    const url = info.frameUrl || info.pageUrl || tab?.url;
    if (url) sendToApp({ url, page: true });
    return;
  }
  const url = info.linkUrl || info.srcUrl;
  if (url && /^(https?|magnet):/i.test(url)) sendToApp({ url, referer: info.pageUrl || tab?.url });
});
