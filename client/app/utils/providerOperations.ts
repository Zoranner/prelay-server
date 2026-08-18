export interface ProviderOperationResult {
  ok: boolean;
  protocol: string | null;
  latency_ms: number | null;
  first_token_ms: number | null;
  error: string | null;
  models: string[] | null;
}

export interface ProviderOperationFeedback {
  success: boolean;
  message: string;
  metrics: string | null;
}

export function getProviderOperationFeedback(
  result: ProviderOperationResult,
): ProviderOperationFeedback {
  const models = result.models?.filter(Boolean) ?? [];
  const metrics = [
    typeof result.latency_ms === "number" ? `延迟 ${result.latency_ms} ms` : null,
    typeof result.first_token_ms === "number" ? `首 Token ${result.first_token_ms} ms` : null,
  ].filter(Boolean);

  if (!result.ok) {
    return {
      success: false,
      message: result.error?.trim() || "操作失败。",
      metrics: metrics.join("；") || null,
    };
  }

  return {
    success: true,
    message:
      models.length > 0
        ? `发现模型：${models.join("、")}`
        : result.protocol
          ? `${result.protocol} 协议测试完成。`
          : "操作完成。",
    metrics: metrics.join("；") || null,
  };
}
