import { defineConfig } from "oxlint";
import antiSlop from "ultracite/oxlint/anti-slop";
import core from "ultracite/oxlint/core";
import react from "ultracite/oxlint/react";

export default defineConfig({
  extends: [core, react, antiSlop],
  ignorePatterns: [
    ...(core.ignorePatterns ?? []),
    "app/src/bindings/**",
    "app/src/product.ts",
    "target/**",
  ],
  jsPlugins: [{ name: "no-comments", specifier: "./tools/oxlint/no-comments.ts" }],
  options: { reportUnusedDisableDirectives: "error", typeAware: true },
  rules: {
    "eslint/sort-keys": "off",
    "import/no-cycle": "error",
    "no-comments/no-comments": "error",
    "no-restricted-imports": [
      "error",
      {
        paths: [
          {
            importNames: ["useEffect", "useLayoutEffect", "useInsertionEffect"],
            message:
              "Derive in render, act in the event handler, or subscribe with useSyncExternalStore.",
            name: "react",
          },
        ],
      },
    ],
    "oxc/no-accumulating-spread": "error",
  },
});
