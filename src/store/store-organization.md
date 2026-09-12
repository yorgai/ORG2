# Store Organization

This document describes the organization of the Jotai store in `/src/store/`.

---

## Hybrid Strategy: Shared vs Colocated Store

### Core Rule: `src/store/` is for atoms used by **2+ modules**

| Location                            | When to Use                                                     |
| ----------------------------------- | --------------------------------------------------------------- |
| `src/store/{category}/`             | Atom is imported by **2+ distinct** `src/modules/*` directories |
| `src/modules/{Module}/store/`       | Atom is **only** used within that single module                 |
| `src/features/{Feature}/store/`     | Atom is **only** used by that feature                           |
| `src/components/{Component}/store/` | Atom is **only** used by that component tree                    |

### Decision Flowchart

```
Is this atom used by 2+ modules?
├── YES → Place in src/store/{category}/
└── NO  → Colocate with the consuming module/feature/component
```

### Completed Migrations (2026-03-30)

| Store           | Old Location              | New Location                              | Status                 |
| --------------- | ------------------------- | ----------------------------------------- | ---------------------- |
| `marketplace/`  | `src/store/marketplace/`  | —                                         | ✅ Deleted (dead code) |
| `devJourney/`   | `src/store/devJourney/`   | `src/features/DevJourney/store/`          | ✅ Migrated            |
| `integrations/` | `src/store/integrations/` | `src/modules/MainApp/Integrations/store/` | ✅ Migrated            |

### Confirmed Shared Store (KEEP in `src/store/`)

| Store          | Consumer Count | Key Consumers                                         |
| -------------- | -------------- | ----------------------------------------------------- |
| `ui/`          | 5              | WorkStation, MainApp, ProjectManager, Session, shared |
| `session/`     | 4              | WorkStation, MainApp, Session, shared                 |
| `repo/`        | 4              | WorkStation, MainApp, ProjectManager, shared          |
| `workstation/` | 3              | WorkStation, MainApp, ProjectManager                  |
| `git/`         | 3              | MainApp, ProjectManager, shared                       |
| `project/`     | 3              | MainApp, ProjectManager, WorkStation                  |
| `search/`      | 2              | MainApp, WorkStation                                  |
| `agent/`       | 2              | WorkStation, MainApp                                  |
| `platform/`    | 2              | MainApp, WorkStation                                  |
| `config/`      | 2              | MainApp, WorkStation                                  |
| `settings/`    | 2              | MainApp, ProjectManager                               |
| `user/`        | 2              | MainApp (multiple sub-modules)                        |

---

## Directory Structure

```
src/store/
├── session/                   # Sessions, CLI runtime, file review, shell processes
├── repo/                      # Repository state
├── project/                     # Orgii projects, multi-repo, branch sync, tracker mode
├── user/                      # Current user entity
├── search/                    # Code search indexing across repos
├── settings/                  # App settings JSONC (~/.orgii/settings.jsonc)
├── git/                       # Git status and operations
├── config/                    # App config atoms (chat UI, IDE bridge)
├── agent/                     # OS agent + AI control panel
├── platform/                  # Dev mode toggle, system dependency scan cache
├── ui/                        # Pure UI state
└── workstation/               # Workstation apps
    ├── tabs/                  # Shared tab system
    ├── codeEditor/            # Code Editor app (includes outputIntegration/)
    ├── database/              # Database app
    └── browser/               # Browser app (includes browser automation atoms)

# Colocated store (single-module usage):
src/features/DevJourney/store/           # Dev Journey map state
src/modules/MainApp/Integrations/store/  # Policies + skills
```

## Categories

### `session/` - Session State

All session-related state including own_key sessions, hosted_key sessions, and file sync.

| File/Folder               | Purpose                                          |
| ------------------------- | ------------------------------------------------ |
| `sessionAtom.ts`          | Local agent sessions                             |
| `cloudSessionAtom.ts`     | Cloud/marketplace sessions                       |
| `viewAtom.ts`             | Session view navigation state                    |
| `creatorStateAtom.ts`     | Session creator mode/location                    |
| `creatorDraftAtom.ts`     | Session creator draft (persisted)                |
| `historyAtom.ts`          | Session history filters                          |
| `cliSessionStatusAtom.ts` | CLI session WS status, planning indicator inputs |
| `fileReviewAtom.ts`       | Agent file change review (per session)           |
| `shellProcessAtom.ts`     | Shell processes per session (terminal UI)        |

```typescript
import { sessionViewAtom, sessionsAtom } from "@src/store/session";
```

### `project/` - Orgii Projects & Sync

Multi-repo project lists, centralized `.orgii` project store state, tracker mode.

```typescript
import {
  orgiiSyncStatusAtom,
  projectListRefreshAtom,
  trackerModeMapAtom,
} from "@src/store/project";
```

### `repo/` - Repository State

Repository and branch management.

| File             | Purpose             |
| ---------------- | ------------------- |
| `atoms.ts`       | Core repo atoms     |
| `derived.ts`     | Computed repo atoms |
| `branchCache.ts` | Branch cache LRU    |
| `storage.ts`     | Persistence helpers |
| `types.ts`       | Type definitions    |

```typescript
import {
  branchesAtom,
  reposAtom,
  selectedRepoAtom,
  selectedRepoIdAtom,
} from "@src/store/repo";
```

### `settings/` - App Settings

VS Code-style settings backed by `~/.orgii/settings.jsonc`.

| File              | Purpose                                      |
| ----------------- | -------------------------------------------- |
| `settingsAtom.ts` | Central atom — init, read/write, file events |
| `settingsSync.ts` | `useSettingsSync()` — init + Tauri listeners |
| `index.ts`        | Barrel exports (atoms, hooks, sync)          |

```typescript
import {
  settingsAtom,
  updateSettingAtom,
  useSetting,
} from "@src/store/settings";
```

### `ui/` - Pure UI State

UI-only state that doesn't contain business logic.

| File                    | Purpose                          |
| ----------------------- | -------------------------------- |
| `sidebarAtom.ts`        | Sidebar collapsed/expanded state |
| `notificationAtom.ts`   | Toast notifications              |
| `editorSettingsAtom.ts` | Editor appearance (font, theme)  |
| `languageAtom.ts`       | i18n language selection          |

### Other cross-cutting folders

| Folder      | Purpose                                                 |
| ----------- | ------------------------------------------------------- |
| `git/`      | Git status and operations — **prefer** `@src/store/git` |
| `config/`   | App-level config atoms (chat appearance, IDE prefs)     |
| `agent/`    | OS agent config mirrors, AI control panel               |
| `platform/` | Dev mode toggle, system dependency scan cache           |

### Colocated Store (single-module usage)

These stores have been moved to their consuming modules:

| Store        | Location                                  | Import Path                               |
| ------------ | ----------------------------------------- | ----------------------------------------- |
| Integrations | `src/modules/MainApp/Integrations/store/` | `@src/modules/MainApp/Integrations/store` |
| DevJourney   | `src/features/DevJourney/store/`          | `@src/features/DevJourney/store`          |

```typescript
// Integrations store (policies + skills)
// DevJourney store
import { devJourneyAtom } from "@src/features/DevJourney/store";
import { behaviorRulesAtom } from "@src/modules/MainApp/Integrations/store";
import { skillEditorDraftAtom } from "@src/modules/MainApp/Integrations/store/skills";
```

### `workstation/` - Workstation Apps

State for the three Workstation apps (Code Editor, Database, Browser).

#### `workstation/tabs/` - Shared Tab System

Shared tab types, mutations, and factories used across all Workstation apps.

```typescript
import {
  WorkStationTab,
  closeTab,
  createFileTab,
  openTab,
} from "@src/store/workstation/tabs";
```

#### `workstation/codeEditor/` - Code Editor App

| Subfolder            | Purpose                                                   |
| -------------------- | --------------------------------------------------------- |
| `editor/`            | Editor UI state (chat, themes, code citation)             |
| `terminal/`          | Terminal sessions state                                   |
| `file/`              | File explorer state                                       |
| `search/`            | Code search state                                         |
| `testRunner/`        | Test runner state                                         |
| `extensions/`        | VSCode extensions state                                   |
| `outputIntegration/` | Git / task output hook refs (set by `EditorIntegrations`) |

#### `workstation/database/` - Database App

Database connections and tabs.

#### `workstation/browser/` - Browser App

| Subfolder   | Purpose                    |
| ----------- | -------------------------- |
| `tabs/`     | Browser session tabs       |
| `tokens/`   | Design tokens for previews |
| `designer/` | Design canvas state        |

## Import Guidelines

### Direct Imports (Recommended)

```typescript
// Session
// Workstation
// Repo
import { reposAtom, selectedRepoAtom } from "@src/store/repo";
import { sessionViewAtom, sessionsAtom } from "@src/store/session";
import { terminalSessionsAtom } from "@src/store/workstation/codeEditor/terminal";
import { workstationLayoutAtom } from "@src/store/workstation/tabs";
```

### Import the owning module

The root store barrel has been removed. Import atoms from the module that defines them; use a focused domain entry only when it provides a deliberate public boundary.

```typescript
import { updateSettingsBatchAtom } from "@src/store/settings/settingsAtom";
import { spotlightOpenAtom } from "@src/store/ui/uiAtom";
```

## Migration Notes

The following imports were migrated (Feb 2026):

| Old Path                                    | New Path                                               |
| ------------------------------------------- | ------------------------------------------------------ |
| `@src/store/domain/session`                 | `@src/store/session`                                   |
| `@src/store/domain/sessionAtom`             | `@src/store/session`                                   |
| `@src/store/domain/cloudSessionAtom`        | `@src/store/session`                                   |
| `@src/store/domain/repo`                    | `@src/store/repo`                                      |
| `@src/store/domain/repoAtom`                | `@src/store/repo`                                      |
| `@src/store/tabs/sessionViewAtom`           | `@src/store/session`                                   |
| `@src/store/domain/editorAtom`              | `@src/store/workstation/codeEditor/editor`             |
| `@src/store/domain/gitOperationAtom`        | `@src/store/git`                                       |
| `@src/store/domain/settings`                | `@src/store/settings`                                  |
| `@src/store/features/projectAtom` (etc.)    | `@src/store/project/...`                               |
| `@src/store/features/userAtom`              | `@src/store/user/userAtom`                             |
| `@src/store/features/codeSearchIndexAtom`   | `@src/store/search/codeSearchIndexAtom`                |
| `@src/store/features/skillEditorDraftAtom`  | `@src/store/integrations/skills/skillEditorDraftAtom`  |
| `@src/store/skills/skillEditorDraftAtom`    | `@src/store/integrations/skills/skillEditorDraftAtom`  |
| `@src/store/features/browserAutomationAtom` | `@src/store/workstation/browser/browserAutomationAtom` |
| `@src/store/features/cliSessionStatusAtom`  | `@src/store/session/cliSessionStatusAtom`              |
| `@src/store/features/fileReviewAtom`        | `@src/store/session/fileReviewAtom`                    |
| `@src/store/features/shellProcessAtom`      | `@src/store/session/shellProcessAtom`                  |
| `@src/store/features/agentMarketplace`      | `@src/store/marketplace`                               |
| `@src/store/integration`                    | `@src/store/workstation/codeEditor/outputIntegration`  |

## See also

- **Hooks:** `src/hooks/hooks-organization.md`
- **Util:** `src/util/util-organization.md`
- **API:** `src/api/api_organization.md`
