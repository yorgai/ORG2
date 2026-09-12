# retire-sidebar-tabs UI audit

| Line                                              | Element               | Verdict          | Reason                                                                   | Suggested change |
| ------------------------------------------------- | --------------------- | ---------------- | ------------------------------------------------------------------------ | ---------------- |
| `src/components/FaviconIcon/index.tsx:25`         | Favicon helper import | keep with reason | Moves URL formatting to a stateless utility; icon rendering is unchanged | None             |
| `src/contexts/workstation/BrowserContext.tsx:115` | Browser provider      | keep with reason | Removes redundant global-tab mirroring; UI structure remains unchanged   | None             |

Verdict totals: **0 fix**, **2 keep with reason**, **0 abstract**.

Source inspection only; no desktop screenshots or live visual verification because computer control is explicitly opt-in.

Architecture: compilation is checked by commit hooks; dead-state ownership moves entirely to existing main-pane tabs; no new FSM, naming aliases or variant branches are introduced. Browser creation/unmount no longer mirrors a second atom. Persisted main-pane tabs, RPC types and resolver behavior are unchanged; removal of the legacy browser-pill fallback is covered by the updated test. No user settings are rewritten.

Performance: removes mirroring effects and duplicate tab state. No new timers, subscriptions, scans or caches. Idle/hidden/reopen behavior has less legacy work by source inspection; no CPU/RAM measurements were taken.
