/**
 * Get favicon URL for a given site URL.
 * Uses Google's favicon service which is reliable and fast.
 */
export const getFaviconUrl = (url: string | undefined): string | undefined => {
  if (!url) return undefined;
  try {
    const urlObj = new URL(url);
    if (!urlObj.hostname) return undefined;
    return `https://www.google.com/s2/favicons?domain=${urlObj.hostname}&sz=32`;
  } catch {
    return undefined;
  }
};

/**
 * Get site name from URL (e.g., "google.com.hk" -> "Google")
 */
export const getSiteNameFromUrl = (url: string | undefined): string => {
  if (!url) return "New Tab";
  try {
    const urlObj = new URL(url);
    const hostname = urlObj.hostname;
    const domain = hostname.replace(/^www\./, "");
    const parts = domain.split(".");
    if (parts.length >= 2) {
      const siteName = parts[0];
      return siteName.charAt(0).toUpperCase() + siteName.slice(1);
    }
    return domain;
  } catch {
    return "New Tab";
  }
};
