# runtime-table-controls UI audit

| Line                                                                | Element          | Verdict          | Reason                                                                                                         | Suggested change |
| ------------------------------------------------------------------- | ---------------- | ---------------- | -------------------------------------------------------------------------------------------------------------- | ---------------- |
| `src/modules/shared/dataSource/RuntimeSectionHeader.tsx:65`         | Refresh controls | keep with reason | Uses the shared secondary Button variant; refresh callbacks and existing useRefreshSpin lifecycle are retained | None             |
| `src/modules/shared/dataSource/RuntimeScanningPanelColumns.tsx:130` | Last scan text   | keep with reason | Places existing timestamp beside row actions using existing text tokens                                        | None             |

Verdict totals: **0 fix**, **2 keep with reason**, **0 abstract**.

Source inspection only; no desktop screenshots or live visual verification because computer control is explicitly opt-in.

Lifecycle review: existing refresh callbacks and useRefreshSpin cleanup remain in place. No new timers, requests, retries or retained state. The changes affect presentation; no runtime performance claim is made.
