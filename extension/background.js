// RDM browser extension - background entry point.
//
// Each module registers its own browser listeners when it is imported, which
// Manifest V3 requires to happen synchronously at start-up:
//   src/app.js      talks to the RDM app on this computer
//   src/sites.js    knows which pages are "video pages" (YouTube, ...)
//   src/media.js    notices video/audio files a page loads
//   src/capture.js  hands browser downloads over to RDM
//   src/menus.js    right-click menu entries
//   src/messages.js answers the content script and the popup
import "./src/media.js";
import "./src/capture.js";
import "./src/menus.js";
import "./src/messages.js";
