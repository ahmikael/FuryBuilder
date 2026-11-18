import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // Each case starts a real stdio child; TypeScript startup can exceed the 5s default.
    testTimeout: 30_000,
    hookTimeout: 30_000,
  },
});
