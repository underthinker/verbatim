// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

// End-user docs site (M4 Phase F). Versioned with the repo; content lives in
// src/content/docs/. Build/CI wiring is a follow-up - the content is the
// deliverable and is readable as plain Markdown today.
export default defineConfig({
  site: "https://underthinker.github.io/verbatim",
  base: "/verbatim",
  integrations: [
    starlight({
      title: "Verbatim",
      description:
        "Local-first dictation with on-device polish. Press a hotkey, speak, get polished text in any app - with zero cloud dependency.",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/underthinker/verbatim",
        },
      ],
      sidebar: [
        { label: "What is Verbatim?", link: "/" },
        { label: "Install", link: "/install/" },
        { label: "Permissions", link: "/permissions/" },
        { label: "Using Verbatim", link: "/using/" },
        { label: "Troubleshooting", link: "/troubleshooting/" },
      ],
    }),
  ],
});
