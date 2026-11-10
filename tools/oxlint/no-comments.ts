import { definePlugin, defineRule } from "@oxlint/plugins";

const kept = /^\s*(?:(?:WHY|SAFETY): [^\n]+$|oxlint-(?:disable|enable)\S* .+ -- \S)|https:\/\//u;

export default definePlugin({
  meta: { name: "no-comments" },
  rules: {
    "no-comments": defineRule({
      create: (context) => ({
        Program: () => {
          for (const comment of context.sourceCode.getAllComments()) {
            if (comment.type !== "Shebang" && !kept.test(comment.value)) {
              context.report({
                message:
                  "Say it in a name, a type, a test or the commit. A comment is only for what the code can't show: a one-line WHY: for a decision or fact, a link (https://…), a one-line SAFETY: for a type assertion, a lint directive with its reason after --.",
                node: comment,
              });
            }
          }
        },
      }),
    }),
  },
});
