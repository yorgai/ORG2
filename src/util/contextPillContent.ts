/**
 * Context Pill Content Helpers
 *
 * Utilities for loading and formatting content when inserting @browser
 * and @workitem pills. Content is stored in window.__orgiiTerminalPillTexts
 * and appended to the agent message on send.
 */
import { invoke } from "@tauri-apps/api/core";

import { type TodoEntry, projectApi } from "@src/api/http/project";
import { storePillText } from "@src/config/pillTokens";
import { mainPaneTabsAtom } from "@src/store/workstation/tabs";
import { getInstrumentedStore } from "@src/util/core/state/instrumentedStore";

export { capPillText } from "@src/config/pillTokens";

const MAX_CONTENT_LENGTH = 100_000;

const pendingLoads = new Map<string, Promise<void>>();

function htmlToPlainText(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  let text = doc.body?.innerText || "";
  if (text.length > MAX_CONTENT_LENGTH) {
    text = text.slice(0, MAX_CONTENT_LENGTH) + "\n[... truncated]";
  }
  return text;
}

/**
 * Wait for all pending async pill content loads to complete.
 * Call before collecting pill texts on send to avoid race conditions.
 */
export async function waitForPendingPills(): Promise<void> {
  if (pendingLoads.size === 0) return;
  await Promise.all(pendingLoads.values());
}

/**
 * Load browser tab content (URL + page text) and store as pill content.
 * Falls back to URL-only if page content is unavailable (webview not mounted).
 */
export function loadBrowserPillContent(tabId: string, pillPath: string): void {
  const store = getInstrumentedStore();
  const mainPaneTab = store
    .get(mainPaneTabsAtom)
    .find(
      (tab) => tab.type === "browser-session" && tab.data.sessionId === tabId
    );
  const url =
    typeof mainPaneTab?.data.url === "string" ? mainPaneTab.data.url : "";
  const webviewLabel = `browser-session-${tabId}`;

  const promise = invoke<string>("get_full_html_document", {
    label: webviewLabel,
  })
    .then((html) => {
      const text = htmlToPlainText(html);
      const content = url ? `URL: ${url}\n\n${text}` : text;
      storePillText(pillPath, content);
    })
    .catch(() => {
      if (url) {
        storePillText(pillPath, `URL: ${url}`);
      }
    })
    .finally(() => {
      pendingLoads.delete(pillPath);
    });
  pendingLoads.set(pillPath, promise);
}

function formatTodos(todos: TodoEntry[]): string {
  if (todos.length === 0) return "";
  const lines = todos.map(
    (todo) => `- [${todo.status === "completed" ? "x" : " "}] ${todo.content}`
  );
  return `\n## Todos\n${lines.join("\n")}`;
}

/**
 * Load work item content (title, description, todos) and store as pill content.
 * `workItemPath` is "projectSlug/shortId", `pillPath` is the full workitem:// URI.
 */
export function loadWorkItemPillContent(
  workItemPath: string,
  pillPath: string
): void {
  const [projectSlug, shortId] = workItemPath.split("/");
  if (!projectSlug || !shortId) {
    storePillText(pillPath, `[Work Item] Invalid path: ${workItemPath}`);
    return;
  }

  const promise = projectApi
    .readWorkItem(projectSlug, shortId)
    .then((item) => {
      const parts: string[] = [
        `[Work Item: ${item.frontmatter.short_id}]`,
        `Title: ${item.frontmatter.title}`,
        `Status: ${item.frontmatter.status}`,
        `Priority: ${item.frontmatter.priority}`,
      ];
      if (item.frontmatter.assignee) {
        parts.push(`Assignee: ${item.frontmatter.assignee}`);
      }
      if (item.frontmatter.labels.length > 0) {
        parts.push(`Labels: ${item.frontmatter.labels.join(", ")}`);
      }
      if (item.body.trim()) {
        parts.push(`\n## Description\n${item.body.trim()}`);
      }
      parts.push(formatTodos(item.frontmatter.todos));

      let content = parts.filter(Boolean).join("\n");
      if (content.length > MAX_CONTENT_LENGTH) {
        content = content.slice(0, MAX_CONTENT_LENGTH) + "\n[... truncated]";
      }
      storePillText(pillPath, content);
    })
    .catch((err: unknown) => {
      const errorMessage = err instanceof Error ? err.message : String(err);
      storePillText(
        pillPath,
        `[Work Item] ${workItemPath}\nFailed to load: ${errorMessage}`
      );
    })
    .finally(() => {
      pendingLoads.delete(pillPath);
    });
  pendingLoads.set(pillPath, promise);
}
