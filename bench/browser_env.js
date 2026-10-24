// Shared browser-environment capture used by the bridge and one-time recorder.
const nav = {};
      for (const k of ["userAgent", "appVersion", "platform", "vendor", "vendorSub", "product", "productSub", "appName",
                       "appCodeName", "language", "cookieEnabled", "onLine", "hardwareConcurrency", "maxTouchPoints",
                       "doNotTrack", "oscpu", "buildID", "pdfViewerEnabled"]) nav[k] = window.navigator[k];
      nav.languages = Array.from(window.navigator.languages);
      const scr = {};
      for (const k of ["width", "height", "availWidth", "availHeight", "availLeft", "availTop", "colorDepth", "pixelDepth"]) scr[k] = window.screen[k];
      const own = o => Array.from(Object.getOwnPropertyNames(o));
      const names = {
        window: [...new Set([...own(window), ...own(Object.getPrototypeOf(window)), ...own(window.EventTarget.prototype)])],
        Document: [...new Set([...own(window.Document.prototype), ...own(window.HTMLDocument.prototype)])],
        Element: own(window.Element.prototype),
        HTMLElement: own(window.HTMLElement.prototype),
        Node: own(window.Node.prototype),
        Navigator: own(window.Navigator.prototype),
      };
      // the tab's own viewport: the runtime is laid out at it (realsite --viewport, GAPS B28)
      return {navigator: nav, screen: scr, innerWidth: window.innerWidth, innerHeight: window.innerHeight,
              outerWidth: window.outerWidth, outerHeight: window.outerHeight,
              screenX: window.screenX, screenY: window.screenY, names};
