import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const source = (path: string) =>
  readFileSync(new URL(`../app/${path}`, import.meta.url), "utf8");

test("desktop client preserves the legacy management shell", () => {
  const app = source("app.vue");
  const css = source("assets/css/main.css");

  expect(app).toContain("app-root");
  expect(app).toContain("app-header");
  expect(app).toContain("app-frame");
  expect(app).toContain("大模型服务透传代理");
  expect(css).toContain("--pr-color-page");
  expect(css).toContain(".page-shell");
  expect(css).toContain(".surface-panel");
  expect(css).toContain(".data-table");
  expect(css).toContain(".drawer-panel");
});

test("management pages use the legacy page, table, and drawer primitives", () => {
  for (const page of ["pages/providers.vue", "pages/interfaces.vue"]) {
    const content = source(page);

    expect(content).toContain("PageShell");
    expect(content).toContain("PageHeader");
    expect(content).toContain("DrawerPanel");
  }

  for (const component of [
    "components/providers/ProviderList.vue",
    "components/interfaces/InterfaceList.vue",
  ]) {
    const content = source(component);

    expect(content).toContain("SurfacePanel");
    expect(content).toContain("DataTableShell");
  }

  for (const page of ["pages/stats.vue", "pages/diagnostics.vue"]) {
    const content = source(page);

    expect(content).toContain("PageShell");
    expect(content).toContain("SurfacePanel");
    expect(content).toContain("data-table");
  }
});
