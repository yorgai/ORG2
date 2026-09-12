import {
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import FindCard from "@src/components/FindCard";
import {
  type FindTarget,
  adoptFindTarget,
  closeFindTarget,
  registerFindTarget,
} from "@src/components/FindCard/findCoordinator";

import {
  REVIEW_SEARCH_LIMIT,
  type ReviewSearchFile,
  type ReviewSearchMatch,
  type ReviewSearchQuery,
} from "./reviewSearchTypes";

export function useReviewSearch({
  enabled,
  files,
  containerRef,
  onNavigate,
  loadFile,
}: {
  enabled: boolean;
  files: readonly ReviewSearchFile[];
  containerRef: RefObject<HTMLDivElement | null>;
  onNavigate: (match: ReviewSearchMatch) => void;
  loadFile?: (path: string) => Promise<ReviewSearchFile | null>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [modes, setModes] = useState({
    caseSensitive: false,
    wholeWord: false,
    regexp: false,
  });
  const [result, setResult] = useState<{
    matches: ReviewSearchMatch[];
    query: ReviewSearchQuery | null;
    error: boolean;
  }>({ matches: [], query: null, error: false });
  const [index, setIndex] = useState(0);
  const [pending, setPending] = useState(false);
  const [flush, setFlush] = useState(0);
  const generation = useRef(0);
  const targetRef = useRef<FindTarget | null>(null);
  const [host, setHost] = useState<HTMLElement | null>(null);
  const navigateRef = useRef(onNavigate);
  useLayoutEffect(() => {
    navigateRef.current = onNavigate;
  }, [onNavigate]);
  useEffect(() => {
    if (!enabled) return;
    const target: FindTarget = {
      scope: "file",
      element: () => containerRef.current,
      open: () => {
        setHost(
          containerRef.current?.closest<HTMLElement>(
            "[data-pane-surface-underlay]"
          ) ?? containerRef.current
        );
        setOpen(true);
      },
      close: () => {
        setOpen(false);
        setQuery("");
        setPending(false);
        setResult({ matches: [], query: null, error: false });
      },
    };
    targetRef.current = target;
    const unregister = registerFindTarget(target);
    return () => {
      unregister();
      targetRef.current = null;
    };
  }, [enabled, containerRef]);
  useEffect(() => {
    const target = targetRef.current;
    if (!target) return;
    if (open) adoptFindTarget(target);
    else closeFindTarget(target);
  }, [open]);
  useEffect(() => {
    if (!enabled || !open || !query.trim()) {
      return;
    }
    let live = true;
    let worker: Worker | null = null;
    let timer: number | undefined;
    let cancelReply: (() => void) | undefined;
    generation.current++;
    const config = { search: query, ...modes };
    const stop = () => {
      window.clearTimeout(timer);
      worker?.terminate();
      worker = null;
      cancelReply?.();
      cancelReply = undefined;
    };
    const run = () => {
      if (document.visibilityState === "hidden") return;
      setPending(true);
      const id = ++generation.current;
      try {
        worker = new Worker(
          new URL("./reviewSearch.worker.ts", import.meta.url),
          { type: "module" }
        );
      } catch {
        setResult({ matches: [], query: null, error: true });
        setPending(false);
        return;
      }
      worker.onmessage = (event: MessageEvent) => {
        if (!live || event.data.id !== generation.current) return;
        const matches = event.data.matches as ReviewSearchMatch[];
        setResult({
          matches,
          query: config,
          error: Boolean(event.data.error),
        });
        setPending(false);
        setIndex(0);
        stop();
        if (matches[0]) navigateRef.current(matches[0]);
      };
      worker.onerror = () => {
        if (!live || id !== generation.current) return;
        setResult({ matches: [], query: null, error: true });
        setPending(false);
        stop();
      };
      if (!loadFile) {
        worker.postMessage({ type: "files", files });
        worker.postMessage({
          type: "search",
          id,
          query: config,
        });
      } else {
        const scan = async () => {
          const matches: ReviewSearchMatch[] = [];
          let error = false;
          for (const file of files) {
            if (!live || id !== generation.current || !worker) return;
            let loaded: ReviewSearchFile | null = null;
            try {
              loaded = await loadFile(file.path);
            } catch {
              error = true;
            }
            if (!live || id !== generation.current || !worker) return;
            if (!loaded) {
              error = true;
              continue;
            }
            const response = await new Promise<{
              matches: ReviewSearchMatch[];
              error?: boolean;
            }>((resolve) => {
              cancelReply = () => resolve({ matches: [], error: true });
              worker!.onmessage = (event: MessageEvent) => {
                if (event.data.id === id) resolve(event.data);
              };
              worker!.onerror = () => resolve({ matches: [], error: true });
              worker!.postMessage({ type: "files", files: [loaded] });
              worker!.postMessage({ type: "search", id, query: config });
            });
            cancelReply = undefined;
            if (!live || id !== generation.current || !worker) return;
            if (response.error) {
              error = true;
              break;
            }
            matches.push(
              ...response.matches.slice(0, REVIEW_SEARCH_LIMIT - matches.length)
            );
            if (matches.length >= REVIEW_SEARCH_LIMIT) break;
          }
          if (!live || id !== generation.current || !worker) return;
          setResult({ matches, query: config, error });
          setPending(false);
          setIndex(0);
          stop();
          if (matches[0]) navigateRef.current(matches[0]);
        };
        scan().catch(() => {
          if (!live || id !== generation.current) return;
          setResult({ matches: [], query: null, error: true });
          setPending(false);
          stop();
        });
      }
    };
    const visibility = () => {
      generation.current++;
      stop();
      if (document.visibilityState !== "hidden") {
        setPending(true);
        timer = window.setTimeout(run, 500);
      }
    };
    timer = window.setTimeout(run, flush ? 0 : 500);
    document.addEventListener("visibilitychange", visibility);
    return () => {
      live = false;
      stop();
      document.removeEventListener("visibilitychange", visibility);
    };
  }, [enabled, open, query, modes, files, flush, loadFile]);
  const move = (delta: number) => {
    if (pending) {
      setFlush((v) => v + 1);
      return;
    }
    if (!result.matches.length) return;
    const next =
      (index + delta + result.matches.length) % result.matches.length;
    setIndex(next);
    navigateRef.current(result.matches[next]);
  };
  const changeQuery = (value: string) => {
    generation.current++;
    setFlush(0);
    setQuery(value);
    setPending(Boolean(value.trim()));
    if (!value.trim()) setResult({ matches: [], query: null, error: false });
  };
  const toggle = (key: keyof typeof modes) => {
    generation.current++;
    setFlush(0);
    setPending(Boolean(query.trim()));
    setModes((v) => ({ ...v, [key]: !v[key] }));
  };
  const card =
    open && host
      ? createPortal(
          <div className="pointer-events-none absolute top-2 right-2 left-2 z-50">
            <div className="pointer-events-auto ml-auto w-full max-w-sm">
              <FindCard
                scope="file"
                targetName={t("actions.review")}
                scopeControls={false}
                statusText={
                  result.error
                    ? t("status.error")
                    : result.matches.length >= REVIEW_SEARCH_LIMIT
                      ? `${index + 1} / ${REVIEW_SEARCH_LIMIT}+`
                      : undefined
                }
                search={{
                  query,
                  setQuery: changeQuery,
                  isSearching: pending,
                  isSearchVisible: open,
                  resultCount: result.matches.length,
                  currentResultIndex: index,
                  nextResult: () => move(1),
                  prevResult: () => move(-1),
                  closeSearch: () => targetRef.current?.close(),
                  caseSensitive: modes.caseSensitive,
                  wholeWord: modes.wholeWord,
                  useRegex: modes.regexp,
                  toggleCaseSensitive: () => toggle("caseSensitive"),
                  toggleWholeWord: () => toggle("wholeWord"),
                  toggleRegex: () => toggle("regexp"),
                }}
              />
            </div>
          </div>,
          host
        )
      : null;
  return {
    card,
    match: open ? (result.matches[index] ?? null) : null,
    appliedQuery: open ? result.query : null,
  };
}
