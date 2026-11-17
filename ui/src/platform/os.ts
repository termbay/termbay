export type Os = "macos" | "windows" | "linux";

/** The parts of `navigator` used to guess the OS. All optional so tests can pass plain objects. */
export interface NavigatorLike {
  readonly userAgentData?: { readonly platform?: string } | undefined;
  readonly platform?: string | undefined;
  readonly userAgent?: string | undefined;
}

/**
 * Guesses the OS the webview runs on. Anything that is neither macOS nor
 * Windows is treated as Linux (BSDs and others behave the same for us).
 */
export function detectOs(nav: NavigatorLike | undefined = globalThis.navigator as NavigatorLike | undefined): Os {
  const hint = `${nav?.userAgentData?.platform ?? ""} ${nav?.platform ?? ""} ${nav?.userAgent ?? ""}`.toLowerCase();
  if (/mac|iphone|ipad/.test(hint)) return "macos";
  if (/win/.test(hint)) return "windows";
  return "linux";
}
