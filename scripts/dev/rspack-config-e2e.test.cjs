const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const rspack = require("@rspack/core");
const ts = require("typescript");

const createRspackConfig = require("../../config/rspack.config.js");

// Discover actual expressions (excluding comments and test-only Node code).
// Compiling them catches new env reads that lack a browser build definition.
async function frontendEnvExpressions(root) {
  const expressions = new Set();
  async function walk(directory) {
    for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
      const filename = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "__tests__") await walk(filename);
      } else if (
        /\.[jt]sx?$/.test(entry.name) &&
        !/\.(test|spec|d)\.[jt]sx?$/.test(entry.name)
      ) {
        const source = ts.createSourceFile(
          filename,
          await fs.readFile(filename, "utf8"),
          ts.ScriptTarget.Latest,
          true
        );
        function visit(node) {
          if (
            ts.isPropertyAccessExpression(node) &&
            ts.isPropertyAccessExpression(node.expression) &&
            node.expression.expression.getText(source) === "process" &&
            node.expression.name.text === "env"
          ) {
            expressions.add(`process.env.${node.name.text}`);
          }
          ts.forEachChild(node, visit);
        }
        visit(source);
      }
    }
  }
  await walk(root);
  return [...expressions].sort();
}

const envExpressions = frontendEnvExpressions(
  path.resolve(__dirname, "../../src")
);

for (const [name, e2e, webdriver, exposed, availability, enabled] of [
  ["ordinary dev", undefined, undefined, false, undefined, true],
  ["explicit E2E", "1", undefined, true, "0", true],
  ["WebDriver", "0", "1", true, "0", true],
  ["availability opt-out", undefined, undefined, false, "0", false],
]) {
  test(`${name} startup configs run without a browser process global`, async () => {
    const keys = [
      "ORGII_E2E",
      "WEBDRIVER",
      "ORGII_IDE_SERVER_PORT",
      "ORGII_AGENT_ORG_REDESIGN",
    ];
    const previous = keys.map((key) => process.env[key]);
    let config;
    try {
      for (const [index, value] of [
        e2e,
        webdriver,
        "13847",
        availability,
      ].entries()) {
        if (value === undefined) delete process.env[keys[index]];
        else process.env[keys[index]] = value;
      }
      config = createRspackConfig();
    } finally {
      keys.forEach((key, index) => {
        if (previous[index] === undefined) delete process.env[key];
        else process.env[key] = previous[index];
      });
    }

    const outputPath = await fs.mkdtemp(
      path.join(os.tmpdir(), "orgii-ide-env-")
    );
    const compiler = rspack.rspack({
      mode: config.mode,
      context: config.context,
      target: "web",
      entry: {
        ide: "./src/config/ideServer.ts",
        availability: "./src/config/agentOrgAvailability.ts",
        env: `data:text/javascript,${encodeURIComponent(`export default [${(await envExpressions).join(",")}];`)}`,
      },
      module: config.module,
      resolve: config.resolve,
      plugins: config.plugins.filter(
        (plugin) => plugin.constructor.name === "DefinePlugin"
      ),
      devtool: false,
      output: {
        path: outputPath,
        filename: "[name].js",
        library: { name: "ideConfig", type: "var" },
      },
    });
    try {
      await new Promise((resolve, reject) => {
        compiler.run((error, stats) => {
          if (error) return reject(error);
          if (stats.hasErrors()) return reject(new Error(stats.toString()));
          resolve();
        });
      });
      const context = vm.createContext({ window: {} });
      vm.runInNewContext(
        await fs.readFile(path.join(outputPath, "env.js"), "utf8"),
        {}
      );
      vm.runInContext(
        await fs.readFile(path.join(outputPath, "ide.js"), "utf8"),
        context
      );
      const availabilityContext = vm.createContext({});
      vm.runInContext(
        await fs.readFile(path.join(outputPath, "availability.js"), "utf8"),
        availabilityContext
      );
      assert.equal(availabilityContext.ideConfig.AGENT_ORG_ENABLED, enabled);
      const key = "__ORGII_E2E_IDE_SERVER_WS_URL__";
      assert.equal(
        context.window[key],
        exposed ? "ws://localhost:13847/ws" : undefined
      );
      assert.equal(
        context.ideConfig.configureIdeServerForIdentifier(
          "org2ai.org2.instance2"
        ),
        13848
      );
      assert.equal(
        context.ideConfig.IDE_SERVER_WS_URL,
        "ws://localhost:13848/ws"
      );
      assert.equal(
        context.window[key],
        exposed ? "ws://localhost:13848/ws" : undefined
      );
    } finally {
      await new Promise((resolve, reject) => {
        compiler.close((error) => (error ? reject(error) : resolve()));
      });
      await fs.rm(outputPath, { recursive: true, force: true });
    }
  });
}
