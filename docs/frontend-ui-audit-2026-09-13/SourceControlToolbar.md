# Source Control toolbar UI audit

Scope: Focus and All Changes share the existing 36px workstation header. Navigation/collapse controls precede one separator; the split toggle and ellipsis menu occupy the trailing action group without a separator between them. Focus retains editor-owned file callbacks through a portal. Empty Focus keeps disabled navigation arrows and the shared settings menu visible.

| Line                                   | Element                                    | Verdict          | Reason                                                                                                                                                                                                      | Suggested change |
| -------------------------------------- | ------------------------------------------ | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `SourceControlHeaderContent.tsx:252`   | Navigation separator and trailing controls | keep with reason | Existing theme border utilities, shared DiffViewModeToggle and small DS buttons preserve the workstation header sizing. No new pixel or color values.                                                       | None             |
| `SourceControlDiffSettingsMenu.tsx:26` | All Changes ellipsis menu                  | keep with reason | Reuses FileHeaderMoreMenu and canonical editor preference atoms; excludes operations requiring one editable file. Existing menu keyboard behavior remains shared.                                           | None             |
| `FileHeader/index.tsx:462`             | Focus menu portal                          | keep with reason | Keeps file-specific callbacks and menu state with the editor while moving its rendered button into the owning pane's toolbar. The inline breadcrumb, stats, open and close controls remain in the file row. | None             |

Verdict totals: **0 fix**, **3 keep with reason**, **0 abstract**.

D1–D5 reviewed within the changed controls: shared DS primitives, existing spacing/theme tokens, existing accessible toggle/menu implementations, no new three-site duplication or sweep candidate.

Verification: SourceControlHeaderContent tests (7), FileHeaderMoreMenu tests (6), and FileHeader portal lifecycle test (1) pass. `pnpm typecheck:fast` passes. No desktop visual verification was performed because computer control was not requested.
