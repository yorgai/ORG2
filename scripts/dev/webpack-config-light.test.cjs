const assert = require("node:assert/strict");
const test = require("node:test");

const createWebpackConfig = require("../../config/webpack.config.js");

function withEnv(overrides, fn) {
  const previous = {};
  for (const key of Object.keys(overrides)) {
    previous[key] = process.env[key];
    if (overrides[key] == null) {
      delete process.env[key];
    } else {
      process.env[key] = overrides[key];
    }
  }

  try {
    return fn();
  } finally {
    for (const key of Object.keys(overrides)) {
      if (previous[key] == null) {
        delete process.env[key];
      } else {
        process.env[key] = previous[key];
      }
    }
  }
}

function getDefinedValue(config, key) {
  const definePlugin = config.plugins.find(
    (plugin) => plugin.constructor?.name === "DefinePlugin"
  );
  return definePlugin?.definitions?.[key];
}

test("light dev disables webpack dev-server browser client", () => {
  const config = withEnv(
    {
      ORGII_LIGHT_DEV: "true",
      FAST_DEV: "true",
      DEV_SOURCEMAPS: "false",
      ORGII_RETRY_MAIN_SCRIPT_LOAD: "false",
    },
    () => createWebpackConfig({}, { mode: "development" })
  );

  assert.equal(config.devServer.hot, false);
  assert.equal(config.devServer.liveReload, false);
  assert.equal(config.devServer.client, false);

  const htmlPlugin = config.plugins.find(
    (plugin) => plugin.constructor?.name === "HtmlWebpackPlugin"
  );
  assert.equal(htmlPlugin?.userOptions?.inject, "body");
  assert.equal(htmlPlugin?.userOptions?.retryMainScriptLoad, false);
});

test("retrying main script loader disables static HTML injection in dev", () => {
  const config = withEnv(
    {
      ORGII_RETRY_MAIN_SCRIPT_LOAD: "true",
    },
    () => createWebpackConfig({}, { mode: "development" })
  );
  const htmlPlugin = config.plugins.find(
    (plugin) => plugin.constructor?.name === "HtmlWebpackPlugin"
  );
  assert.equal(htmlPlugin?.userOptions?.inject, false);
  assert.equal(htmlPlugin?.userOptions?.retryMainScriptLoad, true);
});

test("production keeps default HTML script injection", () => {
  const config = withEnv(
    {
      ORGII_RETRY_MAIN_SCRIPT_LOAD: "true",
    },
    () => createWebpackConfig({}, { mode: "production" })
  );
  const htmlPlugin = config.plugins.find(
    (plugin) => plugin.constructor?.name === "HtmlWebpackPlugin"
  );
  assert.equal(htmlPlugin?.userOptions?.inject, "body");
  assert.equal(htmlPlugin?.userOptions?.retryMainScriptLoad, false);
});

// `pnpm build:stats` dumps stats with `--json`, and webpack-cli serializes
// that dump with `compiler.options.stats`. The console block sets
// `all: false`, which overrides `preset` and once reduced the dump to
// `{time, errors, warnings}` — leaving scripts/quality/check-bundle-budget.mjs
// with no entrypoint to measure.
test("--json builds emit the stats fields the bundle budget reads", () => {
  const config = createWebpackConfig(
    {},
    { mode: "production", json: "build/stats.json" }
  );

  for (const field of [
    "assets",
    "entrypoints",
    "chunks",
    "chunkModules",
    "chunkOrigins",
    "nestedModules",
    "dependentModules",
    "cachedModules",
  ]) {
    assert.equal(config.stats[field], true, `stats.${field} must be enabled`);
  }
});

test("builds without --json keep the terse console stats", () => {
  const config = createWebpackConfig({}, { mode: "production" });

  assert.equal(config.stats.preset, "normal");
  assert.equal(config.stats.entrypoints, undefined);
});

test("WebDriver production bundles force-enable the Agent Org gate", () => {
  const config = withEnv(
    {
      ORGII_E2E: null,
      ORGII_AGENT_ORG_REDESIGN: null,
      WEBDRIVER: "1",
    },
    () => createWebpackConfig({}, { mode: "production" })
  );

  assert.equal(getDefinedValue(config, "process.env.ORGII_E2E"), '"1"');
  assert.equal(
    getDefinedValue(config, "process.env.ORGII_AGENT_ORG_ENABLED"),
    '"1"'
  );
});

test("ordinary production bundles enable Agent Org by default", () => {
  const config = withEnv(
    {
      ORGII_E2E: null,
      ORGII_AGENT_ORG_REDESIGN: null,
      WEBDRIVER: null,
    },
    () => createWebpackConfig({}, { mode: "production" })
  );

  assert.equal(getDefinedValue(config, "process.env.ORGII_E2E"), '"0"');
  assert.equal(
    getDefinedValue(config, "process.env.ORGII_AGENT_ORG_ENABLED"),
    '"1"'
  );
});

test("ordinary production bundles preserve an explicit Agent Org opt-out", () => {
  const config = withEnv(
    {
      ORGII_E2E: null,
      ORGII_AGENT_ORG_REDESIGN: "0",
      WEBDRIVER: null,
    },
    () => createWebpackConfig({}, { mode: "production" })
  );

  assert.equal(getDefinedValue(config, "process.env.ORGII_E2E"), '"0"');
  assert.equal(
    getDefinedValue(config, "process.env.ORGII_AGENT_ORG_ENABLED"),
    '"0"'
  );
});
