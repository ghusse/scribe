export type Os = "mac" | "windows";

export function detectOs(userAgent: string): Os {
  return /Macintosh|Mac OS X/.test(userAgent) ? "mac" : "windows";
}

/** The OS the app runs on (WKWebView on macOS, WebView2 on Windows). */
export const OS: Os = detectOs(navigator.userAgent);
