const fs = require("fs");
const webpack = require("webpack");
const path = require("path");
const HtmlWebpackPlugin = require("html-webpack-plugin");
const ReactRefreshWebpackPlugin = require("@pmmmwh/react-refresh-webpack-plugin");
const Dotenv = require("dotenv-webpack");
const MiniCssExtractPlugin = require("mini-css-extract-plugin");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const TerserPlugin = require("terser-webpack-plugin");
const CssMinimizerPlugin = require("css-minimizer-webpack-plugin");
const { EsbuildPlugin } = require("esbuild-loader");
const { CleanWebpackPlugin } = require("clean-webpack-plugin");

const repoRoot = path.resolve(__dirname, "..");

// ForkTsCheckerWebpackPlugin removed - causes memory issues with large codebase
// Type checking handled by IDE; transpileOnly: true provides fast builds

module.exports = (env, argv) => {
  const isProduction = argv.mode === "production";
  const isMobileRemoteNativeBuild =
    process.env.ORGII_MOBILE_REMOTE_NATIVE === "true";

  const isLightDev = !isProduction && process.env.ORGII_LIGHT_DEV === "true";

  // Development build mode:
  // - Default: SWC (fast builds ~3-5s + React Fast Refresh for state-preserving HMR)
  // - FAST_DEV=true: esbuild (fastest ~2s, but full app remount on changes)
  // - ORGII_LIGHT_DEV=true: esbuild, no HMR, no source maps
  //
  // SWC is a Rust-based compiler that's nearly as fast as esbuild but supports
  // React Fast Refresh. esbuild is faster but can't support Fast Refresh.
  const useFastDev =
    !isProduction && (isLightDev || process.env.FAST_DEV === "true");
  // DEV_SOURCEMAPS: unset/"false" (default) → no maps; "true"/"inline" →
  // eval-cheap-module-source-map; "external" → cheap-module-source-map
  // (.map files fetched only when DevTools opens). Default flipped to off on
  // 2026-09-03: inline maps were 53% of the source text the webview retains
  // (see config/rspack.config.js for the measurement and the mode table).
  const devSourceMapMode =
    isProduction || isLightDev
      ? "none"
      : process.env.DEV_SOURCEMAPS === "true" ||
          process.env.DEV_SOURCEMAPS === "inline"
        ? "inline"
        : process.env.DEV_SOURCEMAPS === "external"
          ? "external"
          : "none";
  const useDevSourceMaps = devSourceMapMode !== "none";
  const retryMainScriptLoad =
    !isProduction &&
    (process.env.ORGII_RETRY_MAIN_SCRIPT_LOAD === "true" ||
      (process.env.ORGII_RETRY_MAIN_SCRIPT_LOAD !== "false" &&
        process.platform === "linux"));
  // WebKitGTK (Linux dev) cannot load App as a runtime dynamic-import chunk
  // (see commit 291f95be6), so Linux keeps App inlined into main.js via
  // `webpackMode: "eager"` in src/index.tsx. Every other platform loads App
  // as a normal async chunk so an edit does not re-render a 31 MB entry.
  const eagerDevApp =
    !isProduction &&
    (process.env.ORGII_DEV_EAGER_APP === "true" ||
      (process.env.ORGII_DEV_EAGER_APP !== "false" &&
        process.platform === "linux"));

  // FAST_PROD=true: use esbuild for transpilation + minification in production.
  // Saves ~30-40s vs the SWC+Terser path. Trades some dead-code elimination
  // depth for speed. Intended for local fast .app builds, not release builds.
  const useFastProd = isProduction && process.env.FAST_PROD === "true";

  // ORGII_REACT_COMPILER=true: run React Compiler (babel-plugin-react-compiler)
  // over src/**/*.tsx before swc transpiles it. Opt-in spike flag, default off.
  // Only the SWC pipeline gets the pass — FAST_DEV / FAST_PROD / light-dev
  // builds skip it, so a compiler-enabled build must not use those env flags.
  const useReactCompiler = process.env.ORGII_REACT_COMPILER === "true";

  // Babel runs ONLY the compiler transform. TS/JSX are enabled as parser
  // plugins (parse-only, no preset), so type annotations and JSX survive
  // into the output and swc stays the transpiler for types/JSX/target
  // lowering, dev Fast Refresh included.
  const reactCompilerLoader = {
    loader: "babel-loader",
    options: {
      babelrc: false,
      configFile: false,
      parserOpts: { sourceType: "module", plugins: ["typescript", "jsx"] },
      plugins: [["babel-plugin-react-compiler", { target: "19" }]],
    },
  };

  const isE2E = process.env.ORGII_E2E === "1" || process.env.WEBDRIVER === "1";
  const devServerPort = Number.parseInt(
    process.env.WEBPACK_DEV_SERVER_PORT ?? process.env.PORT ?? "1998",
    10
  );

  return {
    context: repoRoot,
    entry: isMobileRemoteNativeBuild
      ? { mobileNative: "./src/mobileRemoteNativeEntry.tsx" }
      : {
          main: "./src/index.tsx",
          mobile: "./src/mobileRemoteEntry.tsx",
        },
    output: {
      // The native shell has its own artifact directory so an iOS build cannot
      // clean or overwrite the Desktop/Web Remote production bundle.
      path: path.resolve(
        repoRoot,
        isMobileRemoteNativeBuild ? "build-mobile-native" : "build"
      ),
      // IMPORTANT: publicPath must be "/" to ensure assets load from root
      // Without this, deep routes like /orgii/marketplace/callback cause 404s
      publicPath: "/",
      // IMPORTANT: Use stable names in development to prevent 404s during hot reload
      // Content hashes change on every rebuild, causing chunk loading failures
      filename: isProduction ? "[name].[contenthash].js" : "[name].js",
      chunkFilename: isProduction ? "[name].[contenthash].js" : "[name].js",
      clean: true,
    },
    cache: {
      type: "filesystem",
      // FAST_PROD swaps both the transpiler and minimizer. Keep it in a
      // separate filesystem-cache namespace so webpack cannot reuse cached
      // runtime-condition code generated by the regular production pipeline.
      // Mixing those caches can leave async chunks guarded by a stale runtime
      // id (for example `__webpack_require__.j == 9121` while the emitted
      // runtime id is `49121`), which turns otherwise valid imports into
      // `undefined` only in the packaged app.
      // React Compiler builds get their own cache DIRECTORY, not just a
      // version bump: a version change invalidates the whole shared pack, so
      // toggling the flag would wipe the warm non-compiler dev cache other
      // sessions rely on. A separate name keeps both caches alive side by side.
      ...(useReactCompiler
        ? {
            name: `react-compiler-${isProduction ? "production" : "development"}`,
          }
        : {}),
      version: `${
        isProduction ? (useFastProd ? "prod-fast" : "prod") : "dev"
      }-12`,
      buildDependencies: {
        config: [__filename],
      },
      // gzip the pack files. Uncompressed, webpack keeps the whole multi-GB
      // pack buffer-mapped for lazy slices; gzip reads it back as a stream
      // of small buffers that can be released as entries deserialize.
      // Measured on a warm start of the dev server (9k modules, full source
      // maps): kernel peak 4.7 GB -> 3.6 GB, idle 3.7 GB -> 2.4 GB, warm
      // compile unchanged (~6 s). The old "avoids sass serialization issues"
      // note predates sass-loader's modern API; scss caches round-trip fine.
      // Production keeps the uncompressed pack (CI build cache unchanged).
      compression: isProduction ? false : "gzip",
    },
    // Snapshot: use timestamps for node_modules instead of content hashing.
    // node_modules rarely change during a dev session; timestamp checks are much faster.
    snapshot: {
      managedPaths: [path.resolve(repoRoot, "node_modules")],
      immutablePaths: [],
      module: {
        timestamp: true,
        hash: false,
      },
      resolve: {
        timestamp: true,
        hash: false,
      },
    },

    module: {
      parser: {
        javascript: {
          exportsPresence: "warn",
        },
      },
      rules: [
        {
          // src/icons.ts is a pure re-export barrel over per-icon deep
          // modules. Flagging it side-effect-free lets webpack skip it in the
          // module graph (dev and prod), linking each importer straight to
          // the deep icon modules — same output as hand-written deep imports.
          // Without this flag every icon in the barrel would land in the
          // first chunk that imports it.
          test: /[\\/]src[\\/]icons\.ts$/,
          sideEffects: false,
        },
        {
          test: /\.css$/,
          use: [
            isProduction ? MiniCssExtractPlugin.loader : "style-loader",
            "css-loader",
          ],
        },
        {
          test: /\.scss$/,
          use: [
            isProduction ? MiniCssExtractPlugin.loader : "style-loader",
            "css-loader",
            {
              loader: "postcss-loader",
              options: {
                postcssOptions: {
                  config: path.resolve(repoRoot, "config/postcss.config.js"),
                },
              },
            },
            {
              loader: "sass-loader",
              options: {
                // sass-embedded: native Dart VM compiler (vs the pure-JS
                // dart-sass build) — 2-5x faster scss compiles. The
                // "modern-compiler" API keeps one shared compiler process
                // across files instead of booting one per compile.
                implementation: require("sass-embedded"),
                api: "modern-compiler",
                sassOptions: {
                  // Silence deprecation warnings for faster compilation
                  quietDeps: true,
                  silenceDeprecations: ["import"],
                },
              },
            },
          ],
        },
        {
          test: /\.jsx$/,
          exclude: /node_modules/,
          use: useFastDev
            ? {
                loader: "esbuild-loader",
                options: {
                  loader: "jsx",
                  target: "es2018",
                  jsx: "automatic",
                },
              }
            : {
                // SWC: Fast Rust-based compiler with React Fast Refresh support
                loader: "swc-loader",
                options: {
                  jsc: {
                    target: "es2020",
                    parser: { syntax: "ecmascript", jsx: true },
                    transform: {
                      react: {
                        runtime: "automatic",
                        refresh: !isProduction,
                      },
                    },
                  },
                },
              },
        },
        {
          test: /\.js$/,
          exclude: /node_modules/,
          use:
            useFastDev || useFastProd
              ? {
                  loader: "esbuild-loader",
                  options: {
                    loader: "js",
                    target: "es2020",
                  },
                }
              : {
                  loader: "swc-loader",
                  options: {
                    jsc: {
                      target: "es2020",
                      parser: { syntax: "ecmascript" },
                    },
                  },
                },
        },
        {
          test: /\.tsx$/,
          exclude: /node_modules/,
          use:
            useFastDev || useFastProd
              ? {
                  // esbuild-loader: fastest but no React Fast Refresh
                  loader: "esbuild-loader",
                  options: {
                    loader: "tsx",
                    target: "es2020",
                    jsx: "automatic",
                  },
                }
              : [
                  {
                    // SWC: Fast Rust-based compiler with React Fast Refresh support
                    loader: "swc-loader",
                    options: {
                      jsc: {
                        target: "es2020",
                        parser: { syntax: "typescript", tsx: true },
                        transform: {
                          react: {
                            runtime: "automatic",
                            refresh: !isProduction,
                          },
                        },
                      },
                    },
                  },
                  // Loaders run right-to-left: React Compiler sees the
                  // original TSX, swc transpiles its output.
                  ...(useReactCompiler ? [reactCompilerLoader] : []),
                ],
        },
        {
          test: /\.ts$/,
          exclude: /node_modules/,
          use:
            useFastDev || useFastProd
              ? {
                  // IMPORTANT: .ts must be parsed as TS (not TSX) to avoid JSX ambiguity
                  loader: "esbuild-loader",
                  options: {
                    loader: "ts",
                    target: "es2020",
                  },
                }
              : {
                  loader: "swc-loader",
                  options: {
                    jsc: {
                      target: "es2020",
                      parser: { syntax: "typescript", tsx: false },
                    },
                  },
                },
        },
        {
          test: /\.(mp4|webm)$/i,
          type: "asset/resource",
          generator: {
            filename: "videos/[name].[contenthash:8][ext]",
          },
        },
        {
          // Use webpack 5 asset modules for better performance
          // Images smaller than 8KB will be inlined as data URLs
          test: /\.(png|jpe?g|gif|webp)$/i,
          type: "asset",
          parser: {
            dataUrlCondition: {
              maxSize: 8 * 1024, // 8KB threshold for inlining
            },
          },
          generator: {
            filename: "images/[name].[contenthash:8][ext]",
          },
        },
        {
          test: /\.(woff2?|ttf|otf)$/i,
          type: "asset/resource",
          generator: {
            filename: "fonts/[name].[contenthash:8][ext]",
          },
        },
        {
          // SVGs with ?url query - return URL instead of React component (for <img src>)
          test: /\.svg$/,
          resourceQuery: /url/,
          type: "asset/resource",
          generator: {
            filename: "images/[name].[contenthash:8][ext]",
          },
        },
        {
          // Regular SVGs - convert to React components with @svgr
          test: /\.svg$/,
          resourceQuery: { not: [/url/] },
          use: [
            {
              loader: "@svgr/webpack",
              options: {
                configFile: path.resolve(repoRoot, "config/svgr.json"),
                svgo: true,
                svgoConfig: {
                  plugins: [
                    {
                      name: "preset-default",
                      params: {
                        overrides: {
                          removeViewBox: false, // Keep viewBox for proper scaling
                        },
                      },
                    },
                  ],
                },
              },
            },
          ],
        },
        {
          test: /node_modules\/@webcontainer\/api/,
          sideEffects: false,
        },
        {
          // GLSL shaders - load as raw text for WebGL
          test: /\.glsl$/,
          use: "raw-loader",
        },
        {
          // Markdown files - load as raw text strings
          test: /\.md$/,
          type: "asset/source",
        },
        {
          // `?raw` JS imports - return the file's text without parsing it as
          // a module. Used to inline the React 18 UMD runtimes into generated
          // canvas-artifact documents (reactArtifactDocument.ts). Vitest
          // resolves the same imports through vite's built-in `?raw` support.
          test: /\.js$/,
          resourceQuery: /raw/,
          type: "asset/source",
        },
      ],
    },
    resolve: {
      extensions: [".tsx", ".ts", ".js", ".mjs"],
      // Only resolve from node_modules. src/ paths are handled by aliases (@src, etc.)
      // Having src in modules causes extra filesystem lookups for every bare import.
      modules: ["node_modules"],
      alias: {
        "@": path.resolve(repoRoot),
        "@src": path.resolve(repoRoot, "src/"),
        "@api": path.resolve(repoRoot, "src/api/"),
        "@common": path.resolve(repoRoot, "src/common/"),
        "@page": path.resolve(repoRoot, "src/page/"),
        "@assets": path.resolve(repoRoot, "src/assets/"),
        "@codemirror/commands": path.dirname(
          require.resolve("@codemirror/commands")
        ),
        "@codemirror/language": path.dirname(
          require.resolve("@codemirror/language")
        ),
        "@codemirror/state": path.dirname(require.resolve("@codemirror/state")),
        "@codemirror/view": path.dirname(require.resolve("@codemirror/view")),
        // @a2ui/web_core exports ./v0_9 only under "default" condition, not "import"/"browser".
        // Webpack's package exports resolution omits "default"-only exports, so alias directly.
        // Point at the DIRECTORY (not index.js): webpack alias does prefix matching, so the bare
        // import resolves via directory-index (src/v0_9/index.js) while subpath imports like
        // "@a2ui/web_core/v0_9/basic_catalog" resolve to src/v0_9/basic_catalog (its index.js).
        // The "." entry resolves to .../src/v0_8/index.js; walk up to src/ then into v0_9 so this
        // stays correct under pnpm's symlinked store (matches the @codemirror/* pattern above).
        "@a2ui/web_core/v0_9": path.join(
          path.dirname(path.dirname(require.resolve("@a2ui/web_core"))),
          "v0_9"
        ),
        // react-syntax-highlighter expects the v1 lowlight lib/core.js entry.
        // Resolve that nested dependency explicitly from the pnpm store when needed.
        "lowlight/lib/core": (() => {
          const fs = require("fs");
          const pnpmDir = path.resolve(repoRoot, "node_modules/.pnpm");
          try {
            const dir = fs
              .readdirSync(pnpmDir)
              .find((d) => d.startsWith("lowlight@1."));
            if (dir)
              return path.join(
                pnpmDir,
                dir,
                "node_modules/lowlight/lib/core.js"
              );
          } catch (_ignored) {}
          return path.resolve(
            repoRoot,
            "node_modules/react-syntax-highlighter/node_modules/lowlight/lib/core.js"
          );
        })(),
      },
      fallback: {
        process: require.resolve("process/browser"),
        fs: false,
        // sql.js requires crypto but doesn't actually use it in browser
        crypto: false,
        path: false,
      },
    },
    optimization: {
      minimize: isProduction,
      minimizer: isProduction
        ? useFastProd
          ? [
              // FAST_PROD: esbuild minifier — ~10× faster than Terser, saves ~30s locally.
              // Slightly less aggressive dead-code elimination but output is production-safe.
              new EsbuildPlugin({
                target: "es2020",
                css: true,
                keepNames: true,
                drop: ["console", "debugger"],
              }),
            ]
          : [
              new TerserPlugin({
                parallel: true,
                terserOptions: {
                  compress: {
                    drop_console: true,
                    drop_debugger: true,
                    pure_funcs: [
                      "console.log",
                      "console.info",
                      "console.debug",
                      "console.trace",
                    ],
                    passes: 1,
                    dead_code: true,
                  },
                  mangle: {
                    keep_classnames: true,
                    keep_fnames: true,
                  },
                  keep_classnames: true,
                  keep_fnames: true,
                  output: {
                    comments: false,
                    ascii_only: true,
                  },
                },
                extractComments: false,
              }),
              new CssMinimizerPlugin(),
            ]
        : [],
      // In dev, skip expensive per-module regex splitting and runtime chunk extraction.
      // Only apply granular code splitting in production for caching benefits.
      ...(isProduction
        ? {
            splitChunks: {
              chunks: "all",
              maxInitialRequests: 25,
              maxAsyncRequests: 30,
              minSize: 20000,
              cacheGroups: {
                initialVendors: {
                  test: /[\\/]node_modules[\\/]/,
                  name: "vendors",
                  chunks: "initial",
                  priority: 20,
                  reuseExistingChunk: true,
                },
                asyncVendors: {
                  test: /[\\/]node_modules[\\/]/,
                  name(module, chunks) {
                    const moduleContext = module.context || "";
                    const packageMatch =
                      moduleContext.match(
                        /[\\/]node_modules[\\/]\.pnpm[\\/][^\\/]+[\\/]node_modules[\\/](.*?)([\\/]|$)/
                      ) ||
                      moduleContext.match(
                        /[\\/]node_modules[\\/](.*?)([\\/]|$)/
                      );
                    let packageName = packageMatch?.[1];
                    if (packageName?.startsWith("@")) {
                      const scopedPackageMatch =
                        moduleContext.match(
                          /[\\/]node_modules[\\/]\.pnpm[\\/][^\\/]+[\\/]node_modules[\\/](@[^\\/]+[\\/][^\\/]+)/
                        ) ||
                        moduleContext.match(
                          /[\\/]node_modules[\\/](@[^\\/]+[\\/][^\\/]+)/
                        );
                      packageName = scopedPackageMatch?.[1] ?? packageName;
                    }

                    if (packageName) {
                      return `async-vendors.${packageName
                        .replace("@", "")
                        .replace(/[\\/]/g, ".")
                        .replace(/[^a-zA-Z0-9_.-]/g, "-")}`;
                    }

                    const namedChunks = chunks
                      .map((chunk) => chunk.name)
                      .filter(Boolean)
                      .sort();

                    if (namedChunks.length === 0) {
                      return "async-vendors";
                    }

                    return `async-vendors.${namedChunks[0].replace(
                      /[^a-zA-Z0-9_.-]/g,
                      "-"
                    )}`;
                  },
                  chunks: "async",
                  priority: 15,
                  reuseExistingChunk: true,
                },
                asyncCommon: {
                  chunks: "async",
                  minChunks: 2,
                  priority: 8,
                  reuseExistingChunk: true,
                },
                common: {
                  chunks: "initial",
                  minChunks: 2,
                  priority: 5,
                  reuseExistingChunk: true,
                  name: "common",
                },
              },
            },
            runtimeChunk: "single",
          }
        : {
            // Dev still needs chunk de-duplication. With 275+ dynamic-import
            // boundaries and no splitChunks, every shared module (CodeMirror,
            // xterm, Prism, ...) was copied into each async chunk that used it:
            // 9k distinct modules became 43k emitted module instances (4.8x),
            // 305 MB JS + 258 MB maps in the dev server's memory FS, and each
            // HMR edit to a shared module re-rendered every copy. This group
            // hoists any module used by >= 2 chunks into a shared chunk. No
            // name() functions / regex cache groups on purpose (cheap to run).
            splitChunks: {
              chunks: "async",
              minSize: 0,
              minChunks: 2,
              cacheGroups: {
                default: false,
                defaultVendors: false,
                shared: {
                  minChunks: 2,
                  reuseExistingChunk: true,
                  priority: 10,
                },
              },
            },
            runtimeChunk: false,
          }),
      moduleIds: isProduction ? "deterministic" : "named",
    },
    plugins: [
      // CleanWebpackPlugin: only needed for production builds.
      // Dev server uses in-memory FS; output.clean handles the rest.
      isProduction && new CleanWebpackPlugin(),
      // Main app HTML
      !isMobileRemoteNativeBuild &&
        new HtmlWebpackPlugin({
          template: "./public/index.html",
          chunks: ["main"],
          filename: "index.html",
          // Linux WebKitGTK can internally fail a static <script src="/main.js">
          // load even after the dev server is ready; a failed script is not
          // retried, so Linux dev uses the retrying external loader below.
          inject: retryMainScriptLoad ? false : "body",
          retryMainScriptLoad,
        }),
      // Browser-only Mobile Remote entry. It must not load the Tauri desktop
      // bootstrap from src/index.tsx.
      !isMobileRemoteNativeBuild &&
        new HtmlWebpackPlugin({
          template: "./public/mobile.html",
          chunks: ["mobile"],
          filename: "mobile.html",
          inject: "body",
        }),
      isMobileRemoteNativeBuild &&
        new HtmlWebpackPlugin({
          template: "./public/mobile.html",
          chunks: ["mobileNative"],
          filename: "mobile-native.html",
          inject: "body",
        }),
      // NOTE: HotModuleReplacementPlugin is automatically added by webpack-dev-server when hot: true
      // ReactRefreshWebpackPlugin works with SWC's refresh: true option to enable
      // state-preserving hot reload. Only enabled when not using esbuild/light mode.
      !isProduction &&
        !useFastDev &&
        !isLightDev &&
        new ReactRefreshWebpackPlugin({ overlay: false }),
      new Dotenv({
        systemvars: true,
        silent: !fs.existsSync(path.resolve(repoRoot, ".env")),
      }),
      isProduction &&
        new MiniCssExtractPlugin({
          filename: "[name].[contenthash].css",
          chunkFilename: "[id].[contenthash].css",
          ignoreOrder: true,
        }),
      new webpack.DefinePlugin({
        "process.env.NODE_ENV": JSON.stringify(argv.mode),
        // Inline-compared in src/index.tsx so webpack constant-folds the
        // `webpackMode: "eager"` App import away on platforms that don't need it.
        "process.env.ORGII_DEV_EAGER_APP": JSON.stringify(String(eagerDevApp)),
        // WebDriver builds are explicit test artifacts, even when their
        // embedded frontend uses production optimization. Ordinary release
        // builds receive "0", so E2E helpers remain tree-shaken away.
        "process.env.ORGII_E2E": JSON.stringify(isE2E ? "1" : "0"),
        // Local Rust IDE-server port, baked into the bundle so a second app
        // instance (dual-instance collab testing) talks to its own backend.
        // Must match the ORGII_IDE_SERVER_PORT the Rust side is launched with.
        "process.env.ORGII_IDE_SERVER_PORT": JSON.stringify(
          process.env.ORGII_IDE_SERVER_PORT ?? "13847"
        ),
        "process.env.ORGII_DEEP_LINK_SCHEME": JSON.stringify(
          process.env.ORGII_DEEP_LINK_SCHEME ?? "orgii"
        ),
        // The shell-facing key keeps its historical spelling so existing
        // operator opt-outs retain the same behavior. Frontend code receives
        // only the stable Agent Org availability name.
        "process.env.ORGII_AGENT_ORG_ENABLED": JSON.stringify(
          isE2E ? "1" : (process.env.ORGII_AGENT_ORG_REDESIGN ?? "1")
        ),
        "process.env.E2E_BASE_URL": JSON.stringify(
          process.env.E2E_BASE_URL ??
            `http://127.0.0.1:${process.env.ORGII_IDE_SERVER_PORT ?? "13847"}`
        ),
      }),
      // CopyWebpackPlugin: only needed for production.
      // In dev, static directory serves public/ files directly.
      isProduction &&
        new CopyWebpackPlugin({
          patterns: [{ from: "public/**/*.css", to: "[name][ext]" }],
        }),
      // ForkTsCheckerWebpackPlugin disabled - causes memory issues with large codebase
      // Type checking is handled by IDE instead. transpileOnly: true provides fast builds.
    ].filter(Boolean),
    devServer: {
      port: devServerPort,
      hot: !isLightDev,
      // Light dev avoids the webpack-dev-server browser client entirely.
      // WebKitGTK can trip internal loader errors around the injected
      // liveReload websocket path, and Tauri dev does not need it here.
      liveReload: !isLightDev,
      historyApiFallback: {
        rewrites: [{ from: /^\/orgii\/mobile(?:\/.*)?$/, to: "/mobile.html" }],
      },
      // Disable static file watching to prevent full page reloads during HMR.
      // Default behavior watches public/ directory, which can race with HMR
      // updates and trigger unnecessary index.html reloads.
      static: {
        directory: path.resolve(repoRoot, "public"),
        watch: false,
      },
      client: isLightDev
        ? false
        : {
            overlay: false,
            // Reconnect settings for better HMR recovery
            reconnect: 5,
            webSocketURL: {
              hostname: "localhost",
              pathname: "/ws",
              port: devServerPort,
            },
          },
      open: false,
      headers: {
        "Cross-Origin-Embedder-Policy": "credentialless",
        "Cross-Origin-Opener-Policy": "same-origin-allow-popups",
        "Cross-Origin-Resource-Policy": "cross-origin",
      },
      proxy: {
        // TaskTracker API proxy - avoids CORS issues with localhost:8002
        "/tasktracker-api": {
          target: "http://127.0.0.1:8002",
          changeOrigin: true,
          secure: false,
          pathRewrite: { "^/tasktracker-api": "" },
        },
      },
    },
    performance: {
      hints: false,
    },
    // webpack-cli hands `compiler.options.stats` straight to `stats.toJson()`
    // when `--json` is passed, and `all: false` overrides `preset` — so the
    // console block below would reduce a JSON dump to `{time, errors,
    // warnings}`. `pnpm build:stats` feeds
    // scripts/quality/check-bundle-budget.mjs, which needs entrypoints,
    // assets, and per-chunk modules and origins. Widen those fields for JSON
    // dumps only; module `source` stays excluded, so stats.json stays small.
    stats: argv.json
      ? {
          all: false,
          errors: true,
          warnings: true,
          assets: true,
          entrypoints: true,
          chunks: true,
          chunkModules: true,
          chunkOrigins: true,
          nestedModules: true,
          // `all: false` would otherwise collapse dependent modules into
          // "N dependent modules" placeholders and drop every module the
          // filesystem cache served, making the module count depend on
          // whether the cache was warm.
          dependentModules: true,
          cachedModules: true,
          ids: true,
        }
      : {
          all: false,
          errors: true,
          warnings: true,
          timings: true,
          version: false, // Skip version check for faster startup
          builtAt: false, // Skip timestamp for faster startup
          modules: false, // Skip module list for faster startup
          colors: true,
          // Only show minimal info in dev mode
          preset: isProduction ? "normal" : "minimal",
        },
    // Linux (eagerDevApp): App is bundled into main.js via `webpackMode:
    // "eager"` (see src/index.tsx). With eval-cheap-module-source-map that
    // inlines every module's source into main.js, swelling it past 80MB — too
    // large for WebKitGTK to load. So Linux dev writes source maps to separate
    // lazily-loaded .map files (`cheap-source-map`).
    //
    // Everywhere else: `eval-cheap-module-source-map` — webpack's cheapest
    // source-mapped dev devtool. No separate .map assets are generated or
    // held in the dev server's memory FS, and rebuilds only re-eval the
    // changed modules instead of re-mapping whole chunks.
    //
    // No maps (default, and light dev) trade original-line mapping for
    // memory: plain `eval` keeps per-module eval (fast HMR) but emits no maps
    // at all. Measured warm-start dev server: peak 3.6 GB -> 2.5 GB, idle
    // 2.4 GB -> 1.7 GB, emitted JS 171 MB -> 92 MB, and the webview parses
    // proportionally less. Linux keeps `false` there (WebKitGTK path).
    // DEV_SOURCEMAPS=external writes separate .map files instead.
    devtool: useDevSourceMaps
      ? eagerDevApp
        ? "cheap-source-map"
        : devSourceMapMode === "external"
          ? "cheap-module-source-map"
          : "eval-cheap-module-source-map"
      : isProduction || eagerDevApp
        ? false
        : "eval",
  };
};
