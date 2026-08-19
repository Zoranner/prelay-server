import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const page = (name: string) =>
  readFileSync(new URL(`../app/pages/${name}.vue`, import.meta.url), "utf8");

test("统计页读取总览、聚合和请求明细", () => {
  const stats = page("stats");
  expect(stats).toContain('"stats_overview"');
  expect(stats).toContain('"stats_models"');
  expect(stats).toContain('"stats_providers"');
  expect(stats).toContain('"stats_requests"');
  expect(stats).toContain("请求明细");
  expect(stats).toContain("error_message");
  expect(stats).toContain("latency_ms");
  expect(stats).toContain("upstream_request_id");
  expect(stats).toContain("metadataDetail");
  expect(stats).toContain("<details");
});
