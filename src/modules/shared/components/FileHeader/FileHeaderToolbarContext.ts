import { createContext } from "react";

/** Optional file-menu destination; the host toolbar owns the diff toggle. */
export const FileHeaderToolbarContext = createContext<HTMLElement | null>(null);
