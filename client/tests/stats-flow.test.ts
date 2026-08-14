import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const page = (name: string) =>
  readFileSync(new URL(`../app/pages/${name}.vue`, import.meta.url), "utf8");

test("统计页读取总览、模型与供应商聚合", () => {
  const stats = page("stats");
  expect(stats).toContain('"stats_overview"');
  expect(stats).toContain('"stats_models"');
  expect(stats).toContain('"stats_providers"');
});

test("诊断页读取请求明细并呈现错误和延迟", () => {
  const diagnostics = page("diagnostics");
  expect(diagnostics).toContain('"stats_requests"');
  expect(diagnostics).toContain("error_message");
  expect(diagnostics).toContain("latency_ms");
});
