// semantic-release (.github/workflows/semantic-release.yml): every push to main that CI validated is analysed
// from the last vX.Y.Z tag; a feat, fix or perf commit releases a new version without any human step.
// Plugin versions are pinned exactly in package.json (devDependencies) and bun.lock.
// conventional-changelog-conventionalcommits stays on 9.x: 10.x needs conventional-changelog-writer 9, while
// @semantic-release/release-notes-generator 14 renders with writer 8 ("Missing helper" error on 10.x).

// Changelog sections (French, as in CHANGELOG.md). "hidden" types are left out of the notes and release nothing.
const types = [
  { type: "feat", section: "Fonctionnalités" },
  { type: "fix", section: "Corrections" },
  { type: "perf", section: "Performances" },
  { type: "revert", section: "Retours en arrière" },
  { type: "refactor", section: "Refactorisation", hidden: true },
  { type: "docs", section: "Documentation", hidden: true },
  { type: "style", section: "Style", hidden: true },
  { type: "test", section: "Tests", hidden: true },
  { type: "build", section: "Build", hidden: true },
  { type: "ci", section: "CI", hidden: true },
  { type: "chore", section: "Divers", hidden: true },
];

export default {
  branches: ["main"],
  tagFormat: "v${version}",
  plugins: [
    [
      "@semantic-release/commit-analyzer",
      {
        preset: "conventionalcommits",
        presetConfig: { types },
        // Checked before the defaults; a commit matching none of them falls back to the defaults (a git
        // "Revert ..." message -> patch, anything else -> no release). Before 1.0 a breaking change bumps the minor
        // version: change the first rule to "major" (or drop it) once 1.0.0 is out.
        releaseRules: [
          { breaking: true, release: "minor" },
          { type: "feat", release: "minor" },
          { type: "fix", release: "patch" },
          { type: "perf", release: "patch" },
          { type: "revert", release: "patch" },
        ],
      },
    ],
    ["@semantic-release/release-notes-generator", { preset: "conventionalcommits", presetConfig: { types } }],
    ["@semantic-release/changelog", { changelogFile: "CHANGELOG.md", changelogTitle: "# Changelog" }],
    [
      "@semantic-release/exec",
      {
        // The app version lives in src-tauri/Cargo.toml; package.json and Cargo.lock (scribe-app) follow it.
        prepareCmd: "bun scripts/release-version.ts ${nextRelease.version}",
        // The only way out of semantic-release for the next steps of the workflow (it has no step outputs).
        successCmd: "bun scripts/release-version.ts --output ${nextRelease.version}",
      },
    ],
    [
      "@semantic-release/git",
      {
        assets: ["CHANGELOG.md", "package.json", "src-tauri/Cargo.toml", "Cargo.lock"],
        // Pushed with GITHUB_TOKEN: triggers no workflow (no CI run, no new release run).
        message: "chore(release): ${nextRelease.version}",
      },
    ],
    [
      "@semantic-release/github",
      {
        // The workflow builds Windows and macOS into this draft and publishes it once both builds succeeded.
        draftRelease: true,
        // Nothing but the release: no comments or labels on PRs and issues, no issue when a run fails.
        successCommentCondition: false,
        failCommentCondition: false,
        releasedLabels: false,
      },
    ],
  ],
};
