// Existing UI fixtures are Chinese. Do not couple their assertions to the CI host locale.
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  writable: true,
  value: { language: "zh-CN", languages: ["zh-CN"] },
});
