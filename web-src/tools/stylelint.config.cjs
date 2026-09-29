module.exports = {
  // Lint only hand-authored styles (axinite/src/styles). Globs resolve from
  // this file's directory: skip the Vite build output and the copy embedded
  // in the gateway, which are generated from those sources.
  ignoreFiles: ["../dist/**", "../../src/channels/web/static/**"],
  rules: {
    "at-rule-no-unknown": [true, { ignoreAtRules: ["plugin", "source"] }],
    "color-function-notation": "modern",
    "declaration-block-no-duplicate-properties": true,
    "font-family-name-quotes": "always-where-recommended",
    "import-notation": "string",
    "property-no-unknown": true
  }
};
