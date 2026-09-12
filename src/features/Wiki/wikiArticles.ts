/** English product guide. Keep authentication instructions aligned with KeyVault setup components. */
export interface WikiArticle {
  id: string;
  category: string;
  title: string;
  summary: string;
  steps: string[];
}

export const WIKI_ARTICLES: WikiArticle[] = [
  {
    id: "sessions",
    category: "Workspace",
    title: "Agent sessions & chat",
    summary:
      "Start a conversation, choose an agent and model, and follow the work in the Chat Panel.",
    steps: [
      "Use Start a session below to open the session creator and choose a working directory and available account.",
      "Send instructions in the Chat Panel, inspect tool activity, and follow up in the same session.",
      "Use session search to return to earlier work; imports bring supported external CLI histories into the app.",
    ],
  },
  {
    id: "projects",
    category: "Workspace",
    title: "Projects, work items & directories",
    summary:
      "Organize repositories and work items, and connect sessions to the work they belong to.",
    steps: [
      "Switch working directories to choose the repository or folder for your next task.",
      "Use Projects and Work Items to organize work and attach related sessions.",
      "Choose a branch or worktree when work needs its own checkout; review changes before integrating them.",
    ],
  },
  {
    id: "editor",
    category: "Apps",
    title: "Code Editor & source control",
    summary:
      "Open files, terminals and diffs alongside agent work, then review Git changes and history.",
    steps: [
      "Open the Code Editor from the station dock and choose a file or tab.",
      "Use Source Control to inspect diffs, stage or unstage changes, and commit.",
      "Use Git History to inspect previous commits, or open repository branches from the shortcut below.",
    ],
  },
  {
    id: "browser",
    category: "Apps",
    title: "Browser & replay",
    summary:
      "Keep web pages and recorded browser sessions within your workspace.",
    steps: [
      "Open Browser from the station dock to work with web tabs.",
      "Use the browser sidebar to find sessions and open available replays.",
      "Review captured activity alongside the agent conversation for context.",
    ],
  },
  {
    id: "extensions",
    category: "Apps",
    title: "Skills, rules, MCP & agents",
    summary:
      "Configure reusable instructions and tool integrations from Integrations.",
    steps: [
      "Choose the relevant Skills, Rules, MCP or Agents section in Integrations.",
      "Create an entry or use the supported external import flow to reuse an existing configuration.",
      "Review the entry and its scope before enabling it for your workflow. MCP servers may need their own credentials.",
    ],
  },
  {
    id: "cloud",
    category: "Collaboration",
    title: "Cloud workspaces, sharing & mobile remote",
    summary:
      "Sign in to ORG2 Cloud to access collaboration features available to your account.",
    steps: [
      "Sign in from the account menu, then connect or choose a cloud workspace.",
      "Use session sharing to share work and the shared-session import flow to bring shared context into your workspace.",
      "Use Mobile Remote to access supported remote session controls. Workspace membership and access permissions still apply.",
      "ORG2 Cloud sign-in is separate from model-provider authentication: it does not add a model API key.",
    ],
  },
  {
    id: "usage",
    category: "Apps",
    title: "Runtime, usage & settings",
    summary:
      "Inspect runtime activity and quota information, then tailor appearance and layout.",
    steps: [
      "Open Runtime to inspect usage and available account quota information.",
      "In a cloud workspace, inspect member runtime and sync status where access permits.",
      "Use Settings for appearance and layout preferences; the account menu also exposes appearance, layout and RAM tools.",
    ],
  },
  {
    id: "keys",
    category: "Keys & authentication",
    title: "Adding keys: choose a method",
    summary:
      "Open Integrations → Key Vault, add an account and choose a provider. Available methods depend on that provider.",
    steps: [
      "Direct API key: paste a provider-issued key, choose the endpoint and protocol where offered, validate, then complete the account wizard.",
      "Sign in: complete the provider authorization flow. This adds provider account credentials rather than an ORG2 Cloud login.",
      "Autodetect: sign in or configure the supported CLI locally first, then detect its existing credentials and review the result.",
      "Enter token: paste a supported session token or key only where the selected provider offers this method.",
      "Extract config: for supported generic providers, paste configuration text, extract the credentials, review the key and base URL, then validate.",
      "Import credentials from other apps: expand the import panel, select supported discovered entries, import them and review any per-item failures.",
      "Local model: choose the local runtime, configure its base URL and model, supply a key if the server requires one, then validate.",
    ],
  },
  {
    id: "provider-auth",
    category: "Keys & authentication",
    title: "Authentication by provider",
    summary:
      "The wizard exposes different setup choices for each provider; methods are not interchangeable.",
    steps: [
      "Claude Code: Sign in or Autodetect an existing OAuth login. Saving requires OAuth credentials; a plain API key is not a Claude Code login.",
      "Codex: Sign in, Autodetect, or Enter token. The manual field accepts supported OAuth session tokens or API keys; validate before saving.",
      "Cursor: Guided setup, Autodetect, or Enter token. The setup also offers API key entry; review the credential type selected by the wizard.",
      "GitHub Copilot: enter an existing token and validate, or use the guided token-creation browser flow.",
      "Kiro: Autodetect credentials from kiro-cli or Sign in through its device authorization flow.",
      "Direct API providers: enter a key, with official/custom base URL and protocol options only where supported.",
      "Other providers: use only the methods offered by their configuration. Autodetect, manual entry and config extraction are provider-dependent.",
    ],
  },
  {
    id: "key-troubleshooting",
    category: "Keys & authentication",
    title: "Validation, endpoints & troubleshooting",
    summary:
      "Validate credentials against the intended provider and endpoint before completing setup.",
    steps: [
      "If detection finds nothing, check that the supported CLI is installed and signed in on this machine, then retry detection.",
      "If validation fails, check the credential, selected provider, base URL, protocol and network connection. An account login is not necessarily an API key.",
      "For custom endpoints, confirm the server supports the selected protocol and model. Changing an endpoint can change where requests are sent.",
      "If authorization expires, return to the account setup and sign in again. For failed imports, inspect individual errors before retrying.",
      "After saving, choose the account and an available model in your session setup. Provider usage and ORG2 Cloud membership are separate.",
    ],
  },
];

export function searchWikiArticles(query: string): WikiArticle[] {
  const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  return WIKI_ARTICLES.filter((article) => {
    const text = [
      article.title,
      article.category,
      article.summary,
      ...article.steps,
    ]
      .join(" ")
      .toLowerCase();
    return terms.every((term) => text.includes(term));
  });
}
