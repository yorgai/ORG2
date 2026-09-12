# reveal-hook-config UI audit

| Line                                                                        | Element            | Verdict          | Reason                                                                                                               | Suggested change |
| --------------------------------------------------------------------------- | ------------------ | ---------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `src/modules/shared/dataSource/SessionProvenanceHookPlatformsTable.tsx:369` | Config path action | keep with reason | Native button supports keyboard activation and focus indication; truncated path plus icon forms a custom text action | None             |

Verdict totals: **0 fix**, **1 keep with reason**, **0 abstract**.

Source inspection only; no desktop screenshots or live visual verification because computer control is explicitly opt-in.

The existing show_in_folder command runs only after activation, with failures shown through Message. No polling, automatic file opening or retained state is introduced. Real file-manager behavior was not exercised.
