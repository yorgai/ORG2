/**
 * FaviconIcon Component
 *
 * Shared favicon renderer used across browser tabs, sidebar rows, URL bars,
 * and session replay entries. URL-derived logos are the priority; the globe
 * glyph is used only as a final fallback when no favicon URL is available
 * (or the image failed to load).
 *
 * Priority:
 *   1. URL favicon image (Google S2 service)
 *   2. Loader2 spinner when isLoading and no favicon could be derived
 *   3. HatGlasses for incognito with no resolvable URL
 *   4. Globe glyph (last-resort fallback)
 *
 * URL change resets stale error state so a new domain re-attempts the fetch.
 */
import React, { memo, useEffect, useState } from "react";

import {
  HatGlassesIcon,
  HugeiconsIcon,
  InternetIcon,
  Loading03Icon,
} from "@src/icons";
import { getFaviconUrl } from "@src/util/url/browserDisplay";

interface FaviconIconProps {
  url: string | undefined;
  isIncognito?: boolean;
  isLoading?: boolean;
  isSelected?: boolean;
  size?: number;
  fallbackColor?: string;
}

const DEFAULT_SIZE = 14;
const DEFAULT_FALLBACK_COLOR = "text-text-2";

export const FaviconIcon: React.FC<FaviconIconProps> = memo(
  ({
    url,
    isIncognito,
    isLoading,
    isSelected,
    size = DEFAULT_SIZE,
    fallbackColor = DEFAULT_FALLBACK_COLOR,
  }) => {
    const [imgError, setImgError] = useState(false);
    const faviconUrl = getFaviconUrl(url);

    useEffect(() => {
      queueMicrotask(() => {
        setImgError(false);
      });
    }, [url]);

    // Priority 1: URL logo. Tinted incognito frame still gets the real favicon
    // so the user can recognise the site; the warning tint comes from the
    // surrounding row, not this component.
    if (faviconUrl && !imgError) {
      return (
        <img
          src={faviconUrl}
          alt=""
          className="shrink-0 rounded-xs"
          style={{ width: size, height: size }}
          onError={() => setImgError(true)}
        />
      );
    }

    // Priority 2: loading spinner while the page is mid-navigation and no
    // favicon URL has been resolved yet (e.g. brand-new about:blank tab).
    if (isLoading) {
      return (
        <HugeiconsIcon
          icon={Loading03Icon}
          data-icon="loader-2"
          size={size}
          className="shrink-0 animate-spin text-text-3"
        />
      );
    }

    // Priority 3: incognito glyph for private tabs that have no URL at all
    // (so a private page that has navigated still shows its real favicon).
    if (isIncognito) {
      return (
        <HugeiconsIcon
          icon={HatGlassesIcon}
          data-icon="hat-glasses"
          size={size}
          strokeWidth={1.75}
          className="shrink-0 text-warning-6"
        />
      );
    }

    // Priority 4: globe glyph — final fallback only.
    const globeColor = isSelected ? "text-primary-6" : fallbackColor;
    return (
      <HugeiconsIcon
        icon={InternetIcon}
        data-icon="globe"
        size={size}
        strokeWidth={1.75}
        className={`shrink-0 ${globeColor}`}
      />
    );
  }
);

FaviconIcon.displayName = "FaviconIcon";

export default FaviconIcon;
