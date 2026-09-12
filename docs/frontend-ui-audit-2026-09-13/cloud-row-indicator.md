# cloud-row-indicator UI audit

| Line                                                                                                               | Element       | Verdict          | Reason                                                                                               | Suggested change |
| ------------------------------------------------------------------------------------------------------------------ | ------------- | ---------------- | ---------------------------------------------------------------------------------------------------- | ---------------- |
| `src/scaffold/NavigationSidebar/connectors/WorkstationSidebarConnector/cloudSessionsSection.rowItemBuilder.tsx:93` | Session badge | keep with reason | Removes the cloud-only indicator and its local-copy lookup; download progress and row actions remain | None             |

Verdict totals: **0 fix**, **1 keep with reason**, **0 abstract**.

Source inspection only; no desktop screenshots or live visual verification because computer control is explicitly opt-in.

Performance: removes per-row local-copy lookup and associated inputs. Existing download/presence subscriptions remain unchanged. No new idle or hidden work; runtime performance was not measured.
