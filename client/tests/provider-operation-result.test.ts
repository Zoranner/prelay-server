import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";

import { getProviderOperationFeedback } from "../app/utils/providerOperations";

test("上游 200 但 ok 为 false 时将供应商操作显示为失败，并保留诊断指标", () => {
  const feedback = getProviderOperationFeedback({
    ok: false,
    error: "上游认证失败",
    latency_ms: 840,
    first_token_ms: 380,
  });

  expect(feedback.success).toBe(false);
  expect(feedback.message).toBe("上游认证失败");
  expect(feedback.metrics).toBe("延迟 840 ms；首 Token 380 ms");
});

test("模型发现成功时保留返回的模型列表", () => {
  const feedback = getProviderOperationFeedback({
    ok: true,
    models: ["deepseek-chat", "deepseek-reasoner"],
  });

  expect(feedback.success).toBe(true);
  expect(feedback.message).toBe("发现模型：deepseek-chat、deepseek-reasoner");
});

test("供应商页面根据结果成功状态使用不同提示颜色并显示指标", () => {
  const page = readFileSync(new URL("../app/pages/providers.vue", import.meta.url), "utf8");

  expect(page).toContain("operationFeedback.success");
  expect(page).toContain("operationFeedback.metrics");
  expect(page).toContain("text-rose-200");
  expect(page).toContain("getProviderOperationFeedback");
});
