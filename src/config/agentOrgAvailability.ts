/**
 * Internal availability gate for Agent Org runtime features.
 * It is intentionally absent from Team settings and model/tool context.
 */
export const AGENT_ORG_ENABLED =
  process.env.NODE_ENV === "test" ||
  process.env.ORGII_AGENT_ORG_ENABLED === "1";

export function requireAgentOrgEnabled(): void {
  if (!AGENT_ORG_ENABLED) {
    throw new Error("agent_org_disabled: Agent Org is not enabled");
  }
}
