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
  overrides: [
    {
      files: ["crates/platform/linux/tests/electron_scratch.cjs"],
      rules: {
        "typescript/no-unsafe-assignment": "off",
        "typescript/no-unsafe-call": "off",
        "typescript/no-unsafe-member-access": "off",
        "typescript/no-unsafe-return": "off",
        "unicorn/filename-case": "off",
      },
    },
    {
      files: ["app/scripts/ui-regressions.tsx"],
      rules: { "no-empty-function": "off" },
    },
    {
      files: ["app/src/outside.ts"],
      rules: { "no-restricted-imports": "off", "react/exhaustive-deps": "off" },
    },
  ],
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
              "Derive in render and act in the event handler. To talk to something outside React, use useSubscription or useImperative from app/src/outside.ts.",
            name: "react",
          },
        ],
      },
    ],
    "oxc/no-accumulating-spread": "error",
    "react/exhaustive-deps": ["error", { additionalHooks: "^(useSubscription|useImperative)$" }],
  },
});
