import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

test("client uses Nuxt Tauri and Tailwind entrypoints", () => {
  const packageJson = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  );
  const config = readFileSync(
    new URL("../nuxt.config.ts", import.meta.url),
    "utf8",
  );

  expect(packageJson.scripts.typecheck).toBe("nuxt typecheck");
  expect(packageJson.devDependencies["@tauri-apps/cli"]).toBeDefined();
  expect(config).toContain("@tailwindcss/vite");
});

test("Tauri uses the static Nuxt output and a fixed development port", () => {
  const packageJson = JSON.parse(
    readFileSync(new URL("../package.json", import.meta.url), "utf8"),
  );
  const nuxtConfig = readFileSync(
    new URL("../nuxt.config.ts", import.meta.url),
    "utf8",
  );
  const tauriConfig = JSON.parse(
    readFileSync(
      new URL("../src-tauri/tauri.conf.json", import.meta.url),
      "utf8",
    ),
  );

  expect(packageJson.scripts.generate).toBe("nuxt generate");
  expect(nuxtConfig).toContain("ssr: false");
  expect(nuxtConfig).toContain("devServer:");
  expect(nuxtConfig).toContain("port: 3000");
  expect(nuxtConfig).toContain("strictPort: true");
  expect(tauriConfig.build.devUrl).toBe("http://localhost:3000");
  expect(tauriConfig.build.beforeBuildCommand).toBe("bun run generate");
  expect(tauriConfig.build.frontendDist).toBe("../.output/public");
});
