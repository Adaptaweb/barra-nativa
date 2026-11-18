/** Links point at the public repository. `download` always serves the newest
 * published release (the release workflow attaches a stable Muelle-Setup.exe). */
export const site = {
  name: "Muelle",
  tagline: "The macOS Dock, made for Windows.",
  /** Kept concise for search snippets while naming the supported Windows versions. */
  description:
    "Free, open-source macOS-style dock for Windows 10 and 11, with the Genie effect, optional live window previews and an AI usage menu bar.",
  /** Canonical origin. Must match `site` in astro.config.mjs and the sitemap. */
  url: "https://muellee.vercel.app",
  /** Path to the social card in `public/`. */
  ogImage: "/og-image.png",
  ogImageWidth: 1200,
  ogImageHeight: 630,
  twitterHandle: "@CrisOliveraDev",
  locale: "en_US",
  repository: "https://github.com/crafter-station/Muelle",
  releases: "https://github.com/crafter-station/Muelle/releases",
  download: "https://github.com/crafter-station/Muelle/releases/latest/download/Muelle-Setup.exe",
  issues: "https://github.com/crafter-station/Muelle/issues",
  version: "0.1.13",
  license: "MIT",
} as const;
