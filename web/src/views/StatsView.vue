<template>
  <PageShell>
    <PageHeader title="统计" description="查看请求总览、最近明细、模型和供应商消耗。">
      <template #actions>
        <Button variant="secondary" size="sm" :disabled="loading" @click="loadStats">
          {{ loading ? '刷新中…' : '刷新' }}
        </Button>
      </template>
    </PageHeader>

    <SurfacePanel>
      <div class="min-h-0 flex-1 space-y-5 overflow-auto px-6 py-5">
        <Alert v-if="error" type="error">
          {{ error }}
        </Alert>

        <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
          <div
            v-for="metric in metrics"
            :key="metric.label"
            class="rounded-xl border border-stone-100 bg-stone-50/70 px-4 py-3"
          >
            <p class="text-xs font-medium text-stone-400">{{ metric.label }}</p>
            <p class="text-2xl font-semibold text-stone-800 mt-2 tabular-nums">
              {{ metric.value }}
            </p>
          </div>
        </div>

        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <h3 class="text-sm font-semibold text-stone-700">最近请求</h3>
            <span class="text-xs text-stone-400">最多 50 条</span>
          </div>

          <div class="overflow-x-auto rounded-xl border border-stone-100">
            <table class="min-w-full divide-y divide-stone-100 text-sm">
              <thead class="bg-stone-50 text-xs font-medium text-stone-400">
                <tr>
                  <th class="px-4 py-3 text-left whitespace-nowrap">时间</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">状态</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">提供商</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">模型</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">协议</th>
                  <th class="px-4 py-3 text-right whitespace-nowrap">HTTP</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">上游 ID</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">错误</th>
                  <th class="px-4 py-3 text-left whitespace-nowrap">诊断</th>
                  <th class="px-4 py-3 text-right whitespace-nowrap">Token</th>
                  <th class="px-4 py-3 text-right whitespace-nowrap">耗时</th>
                  <th class="px-4 py-3 text-right whitespace-nowrap">详情</th>
                </tr>
              </thead>
              <tbody class="divide-y divide-stone-100 bg-white">
                <tr v-if="!loading && requestLogs.length === 0">
                  <td colspan="12" class="px-4 py-8 text-center text-stone-400">暂无请求记录</td>
                </tr>
                <template
                  v-for="{ request, metadata, diagnostics } in requestRows"
                  :key="request.id"
                >
                  <tr class="text-stone-600 hover:bg-stone-50/60">
                    <td class="px-4 py-3 whitespace-nowrap text-xs text-stone-500">
                      {{ formatDate(request.created_at) }}
                    </td>
                    <td class="px-4 py-3 whitespace-nowrap">
                      <span
                        class="inline-flex rounded-full px-2 py-0.5 text-xs font-medium"
                        :class="statusClass(request.status)"
                      >
                        {{ statusLabel(request.status) }}
                      </span>
                    </td>
                    <td class="px-4 py-3 whitespace-nowrap">
                      {{ request.provider_name || '—' }}
                    </td>
                    <td class="px-4 py-3 whitespace-nowrap font-mono text-xs">
                      {{ request.model_requested || '—' }}
                    </td>
                    <td class="px-4 py-3 whitespace-nowrap text-xs">
                      {{ protocolLabel(request) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ request.http_status ?? '—' }}
                    </td>
                    <td class="px-4 py-3 max-w-[180px]">
                      <span
                        class="block truncate font-mono text-xs"
                        :title="upstreamRequestId(request)"
                      >
                        {{ upstreamRequestId(request) }}
                      </span>
                    </td>
                    <td class="px-4 py-3 max-w-[220px]">
                      <span class="block truncate text-xs" :title="errorTitle(request)">
                        {{ errorLabel(request) }}
                      </span>
                    </td>
                    <td class="px-4 py-3 whitespace-nowrap">
                      <span
                        class="inline-flex rounded-full border px-2 py-0.5 text-xs font-medium"
                        :class="diagnosticsClass(diagnostics.tone)"
                        :title="diagnostics.title"
                      >
                        {{ diagnostics.label }}
                      </span>
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatTokens(request) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatLatency(request.latency_ms) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap">
                      <button
                        class="rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-stone-500 hover:bg-stone-50"
                        type="button"
                        @click="toggleRequestDetails(request.id)"
                      >
                        {{ expandedRequestId === request.id ? '收起' : '查看' }}
                      </button>
                    </td>
                  </tr>
                  <tr v-if="expandedRequestId === request.id" class="bg-stone-50/70">
                    <td colspan="12" class="px-4 py-4">
                      <div class="grid gap-3 text-xs text-stone-600 lg:grid-cols-4">
                        <div class="rounded-xl border border-stone-100 bg-white px-4 py-3">
                          <p class="font-semibold text-stone-700">桥接</p>
                          <dl class="mt-2 space-y-1">
                            <div class="flex justify-between gap-3">
                              <dt class="text-stone-400">协议</dt>
                              <dd class="font-mono text-right">
                                {{ metadataBridgeProtocol(metadata, request) }}
                              </dd>
                            </div>
                            <div class="flex justify-between gap-3">
                              <dt class="text-stone-400">请求模型</dt>
                              <dd class="font-mono text-right">
                                {{ metadataBridgeValue(metadata, 'model_requested') }}
                              </dd>
                            </div>
                            <div class="flex justify-between gap-3">
                              <dt class="text-stone-400">上游模型</dt>
                              <dd class="font-mono text-right">
                                {{ metadataBridgeValue(metadata, 'model_upstream') }}
                              </dd>
                            </div>
                          </dl>
                        </div>

                        <div class="rounded-xl border border-stone-100 bg-white px-4 py-3">
                          <p class="font-semibold text-stone-700">流式</p>
                          <dl class="mt-2 space-y-1">
                            <div
                              v-for="item in streamMetadataItems(metadata)"
                              :key="item.label"
                              class="flex justify-between gap-3"
                            >
                              <dt class="text-stone-400">{{ item.label }}</dt>
                              <dd class="font-mono text-right">{{ item.value }}</dd>
                            </div>
                          </dl>
                        </div>

                        <div class="rounded-xl border border-stone-100 bg-white px-4 py-3">
                          <p class="font-semibold text-stone-700">上游</p>
                          <dl class="mt-2 space-y-1">
                            <div class="flex justify-between gap-3">
                              <dt class="text-stone-400">request_id</dt>
                              <dd class="max-w-[180px] truncate font-mono text-right">
                                {{ metadataUpstreamRequestId(metadata, request) }}
                              </dd>
                            </div>
                            <div class="space-y-1">
                              <dt class="text-stone-400">error_body_excerpt</dt>
                              <dd class="break-words font-mono text-[11px] leading-5">
                                {{ metadataUpstreamErrorExcerpt(metadata) }}
                              </dd>
                            </div>
                          </dl>
                        </div>

                        <div class="rounded-xl border border-stone-100 bg-white px-4 py-3">
                          <div class="flex items-center justify-between gap-3">
                            <p class="font-semibold text-stone-700">Metadata</p>
                            <span
                              class="rounded-full border px-2 py-0.5 font-medium"
                              :class="metadataStatusClass(metadata.status)"
                            >
                              {{ metadataStatusLabel(metadata) }}
                            </span>
                          </div>
                          <p class="mt-2 break-words text-stone-400">
                            {{ metadataStatusDetail(metadata) }}
                          </p>
                        </div>
                      </div>

                      <div class="mt-3 rounded-xl border border-stone-100 bg-white px-4 py-3">
                        <div class="flex items-center justify-between gap-3">
                          <p class="text-xs font-semibold text-stone-700">诊断明细</p>
                          <span class="text-xs text-stone-400">
                            {{ metadataDiagnostics(metadata).length }} 条
                          </span>
                        </div>
                        <div
                          v-if="metadataDiagnostics(metadata).length === 0"
                          class="mt-3 text-xs text-stone-400"
                        >
                          无 diagnostics 明细
                        </div>
                        <div v-else class="mt-3 overflow-x-auto">
                          <table class="min-w-full divide-y divide-stone-100 text-xs">
                            <thead class="text-stone-400">
                              <tr>
                                <th class="py-2 pr-3 text-left font-medium">级别</th>
                                <th class="px-3 py-2 text-left font-medium">阶段</th>
                                <th class="px-3 py-2 text-left font-medium">动作</th>
                                <th class="px-3 py-2 text-left font-medium">协议</th>
                                <th class="px-3 py-2 text-left font-medium">路径</th>
                                <th class="px-3 py-2 text-left font-medium">代码</th>
                                <th class="py-2 pl-3 text-left font-medium">摘要</th>
                              </tr>
                            </thead>
                            <tbody class="divide-y divide-stone-100">
                              <tr
                                v-for="(diagnostic, index) in metadataDiagnostics(metadata)"
                                :key="`${request.id}-diagnostic-${index}`"
                              >
                                <td class="py-2 pr-3">
                                  <span
                                    class="rounded-full border px-2 py-0.5 font-medium"
                                    :class="diagnosticSeverityClass(diagnostic.severity)"
                                  >
                                    {{ diagnosticValue(diagnostic.severity) }}
                                  </span>
                                </td>
                                <td class="px-3 py-2 font-mono">
                                  {{ diagnosticValue(diagnostic.phase) }}
                                </td>
                                <td class="px-3 py-2 font-mono">
                                  {{ diagnosticValue(diagnostic.action) }}
                                </td>
                                <td class="px-3 py-2 font-mono">
                                  {{ diagnosticValue(diagnostic.protocol) }}
                                </td>
                                <td class="px-3 py-2 font-mono">
                                  {{ diagnosticValue(diagnostic.path) }}
                                </td>
                                <td class="px-3 py-2 font-mono">
                                  {{ diagnosticValue(diagnostic.code) }}
                                </td>
                                <td class="py-2 pl-3">
                                  {{ diagnosticMessage(diagnostic) }}
                                </td>
                              </tr>
                            </tbody>
                          </table>
                        </div>
                      </div>
                    </td>
                  </tr>
                </template>
              </tbody>
            </table>
          </div>
        </div>

        <div class="grid gap-4 lg:grid-cols-2">
          <div class="space-y-3">
            <div class="flex items-center justify-between">
              <h3 class="text-sm font-semibold text-stone-700">模型统计</h3>
              <span class="text-xs text-stone-400">按请求模型聚合</span>
            </div>

            <div class="overflow-x-auto rounded-xl border border-stone-100">
              <table class="min-w-full divide-y divide-stone-100 text-sm">
                <thead class="bg-stone-50 text-xs font-medium text-stone-400">
                  <tr>
                    <th class="px-4 py-3 text-left whitespace-nowrap">模型</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">请求</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">失败</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">Token</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">均耗时</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-stone-100 bg-white">
                  <tr v-if="!loading && modelStats.length === 0">
                    <td colspan="5" class="px-4 py-8 text-center text-stone-400">暂无模型统计</td>
                  </tr>
                  <tr
                    v-for="model in modelStats"
                    :key="model.model_requested || 'unknown-model'"
                    class="text-stone-600 hover:bg-stone-50/60"
                  >
                    <td class="px-4 py-3 whitespace-nowrap font-mono text-xs">
                      {{ model.model_requested || '—' }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatNumber(model.total_requests) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatNumber(model.failed_requests) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatTokenPair(model.input_tokens, model.output_tokens) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatLatency(model.average_latency_ms) }}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          <div class="space-y-3">
            <div class="flex items-center justify-between">
              <h3 class="text-sm font-semibold text-stone-700">Provider 统计</h3>
              <span class="text-xs text-stone-400">按上游聚合</span>
            </div>

            <div class="overflow-x-auto rounded-xl border border-stone-100">
              <table class="min-w-full divide-y divide-stone-100 text-sm">
                <thead class="bg-stone-50 text-xs font-medium text-stone-400">
                  <tr>
                    <th class="px-4 py-3 text-left whitespace-nowrap">Provider</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">请求</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">失败</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">均耗时</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">首 Token</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-stone-100 bg-white">
                  <tr v-if="!loading && providerStats.length === 0">
                    <td colspan="5" class="px-4 py-8 text-center text-stone-400">
                      暂无 Provider 统计
                    </td>
                  </tr>
                  <tr
                    v-for="provider in providerStats"
                    :key="provider.provider_id || provider.provider_name || 'unknown-provider'"
                    class="text-stone-600 hover:bg-stone-50/60"
                  >
                    <td class="px-4 py-3 whitespace-nowrap">
                      {{ provider.provider_name || provider.provider_id || '—' }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatNumber(provider.total_requests) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatNumber(provider.failed_requests) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatLatency(provider.average_latency_ms) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatLatency(provider.average_first_token_ms) }}
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </div>
    </SurfacePanel>
  </PageShell>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { Alert, Button } from '../components/base';
import PageHeader from '../components/base/PageHeader.vue';
import PageShell from '../components/base/PageShell.vue';
import SurfacePanel from '../components/base/SurfacePanel.vue';
import {
  statsApi,
  type BridgeDiagnostic,
  type ModelStatsSummary,
  type ProviderStatsSummary,
  type RequestMetadata,
  type RequestLogSummary,
  type StatsOverview,
} from '../api';

const props = withDefaults(
  defineProps<{
    searchQuery?: string;
  }>(),
  {
    searchQuery: '',
  },
);

const emptyOverview: StatsOverview = {
  total_requests: 0,
  successful_requests: 0,
  failed_requests: 0,
  input_tokens: 0,
  output_tokens: 0,
};

const overview = ref<StatsOverview>(emptyOverview);
const requestLogs = ref<RequestLogSummary[]>([]);
const modelStats = ref<ModelStatsSummary[]>([]);
const providerStats = ref<ProviderStatsSummary[]>([]);
const loading = ref(false);
const error = ref('');
const expandedRequestId = ref<string | null>(null);

const numberFormatter = new Intl.NumberFormat('zh-CN');

type DiagnosticsTone = 'empty' | 'normal' | 'warning' | 'invalid';
type MetadataStatus = 'empty' | 'valid' | 'invalid';
type BridgeField = keyof NonNullable<RequestMetadata['bridge']>;

interface DiagnosticsSummary {
  label: string;
  title: string;
  tone: DiagnosticsTone;
}

interface MetadataParseResult {
  status: MetadataStatus;
  metadata: RequestMetadata | null;
  detail: string;
}

const metrics = computed(() => [
  { label: '总请求', value: formatNumber(overview.value.total_requests) },
  { label: '成功', value: formatNumber(overview.value.successful_requests) },
  { label: '失败', value: formatNumber(overview.value.failed_requests) },
  { label: '输入 Token', value: formatNumber(overview.value.input_tokens) },
  { label: '输出 Token', value: formatNumber(overview.value.output_tokens) },
]);

const requestRows = computed(() =>
  requestLogs.value.filter(requestMatchesSearch).map((request) => {
    const metadata = parseRequestMetadata(request);

    return {
      request,
      metadata,
      diagnostics: diagnosticsSummary(request, metadata),
    };
  }),
);

onMounted(() => {
  loadStats();
});

async function loadStats() {
  loading.value = true;
  error.value = '';

  try {
    const [overviewResponse, requestsResponse, modelsResponse, providersResponse] =
      await Promise.all([
        statsApi.getOverview(),
        statsApi.listRequests(),
        statsApi.listModels(),
        statsApi.listProviders(),
      ]);

    overview.value = overviewResponse.data;
    requestLogs.value = requestsResponse.data;
    modelStats.value = modelsResponse.data;
    providerStats.value = providersResponse.data;
  } catch {
    error.value = '统计数据加载失败，请稍后重试。';
  } finally {
    loading.value = false;
  }
}

function formatNumber(value: number) {
  return numberFormatter.format(value);
}

function formatDate(value: string) {
  const date = new Date(value);

  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return date.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function statusLabel(status: string) {
  return status === 'success' ? '成功' : '失败';
}

function statusClass(status: string) {
  return status === 'success' ? 'bg-[#f2f8f5] text-[#256047]' : 'bg-[#fdf3f3] text-[#a83232]';
}

function protocolLabel(request: RequestLogSummary) {
  const source = request.protocol_in || '—';
  const upstream = request.protocol_upstream || '—';

  return `${source} → ${upstream}`;
}

function formatTokens(request: RequestLogSummary) {
  return formatTokenPair(request.input_tokens ?? 0, request.output_tokens ?? 0);
}

function errorLabel(request: RequestLogSummary) {
  return request.error_code || request.error_message || '—';
}

function errorTitle(request: RequestLogSummary) {
  return [request.error_code, request.error_message].filter(Boolean).join('：') || '无错误';
}

function upstreamRequestId(request: RequestLogSummary) {
  return request.upstream_request_id || '—';
}

function toggleRequestDetails(requestId: string) {
  expandedRequestId.value = expandedRequestId.value === requestId ? null : requestId;
}

function diagnosticsSummary(
  request: RequestLogSummary,
  metadataResult = parseRequestMetadata(request),
): DiagnosticsSummary {
  if (!request.metadata_json?.trim()) {
    return {
      label: '—',
      title: '无 metadata',
      tone: 'empty',
    };
  }

  if (metadataResult.status === 'invalid') {
    return {
      label: '解析失败',
      title: metadataResult.detail,
      tone: 'invalid',
    };
  }

  const diagnostics = metadataDiagnostics(metadataResult);

  if (diagnostics.length === 0) {
    return {
      label: '无诊断',
      title: 'metadata 未包含 diagnostics，或 diagnostics 为空',
      tone: 'empty',
    };
  }

  const warningCount = diagnostics.filter(isWarningDiagnostic).length;
  const title = `diagnostics: ${diagnostics.length}，warning: ${warningCount}`;

  return {
    label:
      warningCount > 0
        ? `${diagnostics.length} 条 / ${warningCount} 警告`
        : `${diagnostics.length} 条`,
    title,
    tone: warningCount > 0 ? 'warning' : 'normal',
  };
}

function parseRequestMetadata(request: RequestLogSummary): MetadataParseResult {
  if (!request.metadata_json?.trim()) {
    return {
      status: 'empty',
      metadata: null,
      detail: '无 metadata',
    };
  }

  let metadata: unknown;

  try {
    metadata = JSON.parse(request.metadata_json);
  } catch {
    return {
      status: 'invalid',
      metadata: null,
      detail: 'metadata_json 不是有效 JSON',
    };
  }

  if (!isRecord(metadata)) {
    return {
      status: 'invalid',
      metadata: null,
      detail: 'metadata_json 不是对象',
    };
  }

  return {
    status: 'valid',
    metadata: metadata as RequestMetadata,
    detail: '已按 request metadata schema 解析',
  };
}

function metadataDiagnostics(metadataResult: MetadataParseResult): BridgeDiagnostic[] {
  const diagnostics = metadataResult.metadata?.diagnostics;

  if (!Array.isArray(diagnostics)) {
    return [];
  }

  return diagnostics.filter(isRecord) as BridgeDiagnostic[];
}

function metadataBridgeProtocol(metadataResult: MetadataParseResult, request: RequestLogSummary) {
  const bridge = metadataResult.metadata?.bridge;
  const protocolIn = valueOrDash(bridge?.protocol_in ?? request.protocol_in);
  const protocolOut = valueOrDash(bridge?.protocol_out);
  const protocolUpstream = valueOrDash(bridge?.protocol_upstream ?? request.protocol_upstream);

  return `${protocolIn} → ${protocolOut} → ${protocolUpstream}`;
}

function metadataBridgeValue(metadataResult: MetadataParseResult, field: BridgeField) {
  return valueOrDash(metadataResult.metadata?.bridge?.[field]);
}

function streamMetadataItems(metadataResult: MetadataParseResult) {
  const stream = metadataResult.metadata?.stream;

  return [
    { label: 'completed', value: boolOrDash(stream?.completed) },
    { label: 'empty', value: boolOrDash(stream?.empty) },
    { label: 'final_usage_seen', value: boolOrDash(stream?.final_usage_seen) },
    { label: 'stream_error', value: valueOrDash(stream?.stream_error) },
  ];
}

function metadataUpstreamRequestId(
  metadataResult: MetadataParseResult,
  request: RequestLogSummary,
) {
  return valueOrDash(metadataResult.metadata?.upstream?.request_id ?? request.upstream_request_id);
}

function metadataUpstreamErrorExcerpt(metadataResult: MetadataParseResult) {
  return valueOrDash(truncateMetadataText(metadataResult.metadata?.upstream?.error_body_excerpt));
}

function metadataStatusLabel(metadataResult: MetadataParseResult) {
  if (metadataResult.status === 'valid') {
    return '已解析';
  }

  if (metadataResult.status === 'invalid') {
    return '异常';
  }

  return '为空';
}

function metadataStatusDetail(metadataResult: MetadataParseResult) {
  const schema = metadataResult.metadata?.schema;

  if (metadataResult.status === 'valid' && schema) {
    return `schema: ${schema}`;
  }

  return metadataResult.detail;
}

function metadataStatusClass(status: MetadataStatus) {
  if (status === 'valid') {
    return 'border-[#dcebe3] bg-[#f2f8f5] text-[#256047]';
  }

  if (status === 'invalid') {
    return 'border-amber-200 bg-amber-50 text-amber-700';
  }

  return 'border-stone-100 bg-stone-50 text-stone-400';
}

function diagnosticsClass(tone: DiagnosticsTone) {
  if (tone === 'warning') {
    return 'border-amber-200 bg-amber-50 text-amber-700';
  }

  if (tone === 'invalid') {
    return 'border-stone-200 bg-stone-100 text-stone-500';
  }

  if (tone === 'normal') {
    return 'border-[#dcebe3] bg-[#f2f8f5] text-[#256047]';
  }

  return 'border-stone-100 bg-stone-50 text-stone-400';
}

function isWarningDiagnostic(value: unknown) {
  if (!isRecord(value)) {
    return false;
  }

  return ['severity', 'level', 'type', 'status'].some(
    (key) => value[key] === 'warning' || value[key] === 'warn',
  );
}

function diagnosticSeverityClass(value: unknown) {
  return metadataText(value) === 'warning'
    ? 'border-amber-200 bg-amber-50 text-amber-700'
    : 'border-stone-100 bg-stone-50 text-stone-500';
}

function diagnosticValue(value: unknown) {
  return metadataText(value) || '—';
}

function diagnosticMessage(diagnostic: BridgeDiagnostic) {
  const message = truncateMetadataText(diagnostic.message);
  const originalKind = metadataText(diagnostic.original_kind);

  if (!originalKind) {
    return valueOrDash(message);
  }

  return `${valueOrDash(message)}（原始类型：${originalKind}）`;
}

function truncateMetadataText(value: unknown) {
  const text = metadataText(value);

  if (!text) {
    return null;
  }

  return text.length > 240 ? `${text.slice(0, 240)}…` : text;
}

function boolOrDash(value: boolean | null | undefined) {
  if (value === true) {
    return 'true';
  }

  if (value === false) {
    return 'false';
  }

  return '—';
}

function valueOrDash(value: unknown) {
  return metadataText(value) || '—';
}

function metadataText(value: unknown) {
  if (typeof value === 'string') {
    return value.trim();
  }

  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value);
  }

  return '';
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requestMatchesSearch(request: RequestLogSummary) {
  const query = props.searchQuery.trim().toLowerCase();
  if (!query) {
    return true;
  }

  return [
    request.provider_name,
    request.model_requested,
    request.protocol_in,
    request.protocol_upstream,
    request.upstream_request_id,
    request.error_code,
    request.error_message,
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase()
    .includes(query);
}

function formatTokenPair(inputTokens: number, outputTokens: number) {
  return `${formatNumber(inputTokens)} / ${formatNumber(outputTokens)}`;
}

function formatLatency(value: number | null) {
  return value === null ? '—' : `${formatNumber(value)} ms`;
}
</script>
