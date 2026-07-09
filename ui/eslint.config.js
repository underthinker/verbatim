import js from "@eslint/js";
import jsxA11y from "eslint-plugin-jsx-a11y";
import tseslint from "typescript-eslint";

// jsx-a11y is the standing guard for the UX.md 8 accessibility pass: it runs in
// the same `pnpm lint` CI job, so a regression on labels, roles, or keyboard
// affordances fails the build rather than waiting for a screen-reader sweep.
export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["src/**/*.{ts,tsx}"],
    extends: [jsxA11y.flatConfigs.strict],
    rules: {
      // A tabpanel is the one non-interactive element the WAI-ARIA tabs pattern
      // wants focusable, so a panel with no controls of its own stays reachable.
      "jsx-a11y/no-noninteractive-tabindex": [
        "error",
        { tags: [], roles: ["tabpanel"] },
      ],
    },
  },
);
