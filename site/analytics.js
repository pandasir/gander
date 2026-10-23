// Page views, via GoatCounter.
//
// One file so the six pages cannot disagree about the site code, and so the code
// is changed in exactly one place. Every page loads this with:
//
//     <script src="analytics.js" defer></script>
//
// Why GoatCounter: it sets no cookies, stores no personal data, and the dashboard
// counts only what a page view needs. That matches the rest of this project's
// choices (no telemetry in the binary, no vendor SDK for GPU figures), and it is
// the lightest thing that answers "which page did the traffic land on".
//
// The site code below must exist on goatcounter.com. An unregistered code answers
// 400 with "no site at this domain" and this file then fails silently, which is
// why packaging/check-channels.sh asks whether it is live instead of trusting
// that it was registered.
(function () {
  'use strict';

  var SITE_CODE = 'gander';

  // Honour a visitor who asked not to be counted. GoatCounter would count them
  // anyway; this is the one place we can notice the request before anything is
  // sent. Global Privacy Control is the legally-binding form of the same signal.
  var n = navigator;
  if (n.doNotTrack === '1' || n.doNotTrack === 'yes' || n.globalPrivacyControl === true) {
    return;
  }

  // This is a GitHub Pages project site, so every real path starts with /gander/.
  // GoatCounter would otherwise report "/gander/vs-htop.html", and every row in
  // the dashboard would repeat the same prefix. Strip it so the report lists
  // "vs-htop.html" and "entry" for the landing page.
  //
  // `window.goatcounter` has to be set before count.js loads, which is why it is
  // assigned here rather than with data-goatcounter-settings on a tag we control
  // after the fact.
  var PREFIX = '/gander/';
  var path = location.pathname;
  if (path.indexOf(PREFIX) === 0) {
    path = path.slice(PREFIX.length - 1); // keep the leading slash
  }
  if (path === '/' || path === '') {
    path = '/entry';
  }
  window.goatcounter = { path: path };

  // count.js also skips localhost and file:// on its own, so a local preview
  // never lands in the dashboard.
  var s = document.createElement('script');
  s.async = true;
  s.src = 'https://gc.zgo.at/count.js';
  s.setAttribute('data-goatcounter', 'https://' + SITE_CODE + '.goatcounter.com/count');
  document.head.appendChild(s);
})();
