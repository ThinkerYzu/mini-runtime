// X's posts seen: the status ids of the posts that were in the timeline at any time (X keeps only the
// posts near the viewport in its document), sampled every 250 ms, as window.__xposts; for the
// scrolling check (x-scroll.actions.json, task 846d), put into the page with REPLAY_INJECT
(function () {
  const seen = window.__xposts = new Set();
  window.__xmaxScroll = 0;
  setInterval(() => {
    for (const a of document.querySelectorAll('article[data-testid="tweet"] a[href*="/status/"]')) {
      const m = /\/status\/(\d+)/.exec(a.getAttribute("href"));
      if (m) seen.add(m[1]);
    }
    window.__xmaxScroll = Math.max(window.__xmaxScroll, window.scrollY);
  }, 250);
})();
