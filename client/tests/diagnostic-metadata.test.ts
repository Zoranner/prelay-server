import { expect, test } from "bun:test";

import { formatDiagnosticMetadata } from "../app/utils/diagnosticMetadata";

test("诊断元数据格式化 JSON 并限制展示长度", () => {
  expect(formatDiagnosticMetadata('{"trace":"request-1"}')).toBe('{\n  "trace": "request-1"\n}');
  expect(formatDiagnosticMetadata(`{"detail":"${"x".repeat(13_000)}"}`)).toEndWith("...（已截断）");
});

test("诊断元数据不会因无效 JSON 中断页面渲染", () => {
  expect(formatDiagnosticMetadata("not-json")).toBe("not-json");
  expect(formatDiagnosticMetadata(null)).toBeNull();
});
