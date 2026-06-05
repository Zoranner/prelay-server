<template>
  <div class="min-h-screen bg-[#f0ede8]">
    <AppHeader />
    <main class="max-w-5xl mx-auto px-6 py-10 space-y-4">
      <section class="bg-white rounded-2xl border border-stone-200 shadow-sm overflow-hidden">
        <div class="px-6 py-5 border-b border-stone-100 flex items-center justify-between gap-4">
          <div>
            <h2 class="text-base font-semibold text-stone-800">请求统计</h2>
            <p class="text-xs text-stone-400 mt-1">汇总代理请求和最近请求明细</p>
          </div>
          <button
            class="shrink-0 rounded-lg border border-stone-200 px-3 py-2 text-sm font-medium text-stone-600 hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-60"
            :disabled="loading"
            @click="loadStats"
          >
            {{ loading ? '刷新中…' : '刷新' }}
          </button>
        </div>

        <div class="px-6 py-5 space-y-5">
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
                    <th class="px-4 py-3 text-left whitespace-nowrap">错误</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">Token</th>
                    <th class="px-4 py-3 text-right whitespace-nowrap">耗时</th>
                  </tr>
                </thead>
                <tbody class="divide-y divide-stone-100 bg-white">
                  <tr v-if="!loading && requestLogs.length === 0">
                    <td colspan="9" class="px-4 py-8 text-center text-stone-400">暂无请求记录</td>
                  </tr>
                  <tr
                    v-for="request in requestLogs"
                    :key="request.id"
                    class="text-stone-600 hover:bg-stone-50/60"
                  >
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
                    <td class="px-4 py-3 max-w-[220px]">
                      <span class="block truncate text-xs" :title="errorTitle(request)">
                        {{ errorLabel(request) }}
                      </span>
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatTokens(request) }}
                    </td>
                    <td class="px-4 py-3 text-right whitespace-nowrap tabular-nums">
                      {{ formatLatency(request.latency_ms) }}
                    </td>
                  </tr>
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
      </section>
    </main>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import AppHeader from '../components/AppHeader.vue';
import { Alert } from '../components/base';
import {
  statsApi,
  type ModelStatsSummary,
  type ProviderStatsSummary,
  type RequestLogSummary,
  type StatsOverview,
} from '../api';

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

const numberFormatter = new Intl.NumberFormat('zh-CN');

const metrics = computed(() => [
  { label: '总请求', value: formatNumber(overview.value.total_requests) },
  { label: '成功', value: formatNumber(overview.value.successful_requests) },
  { label: '失败', value: formatNumber(overview.value.failed_requests) },
  { label: '输入 Token', value: formatNumber(overview.value.input_tokens) },
  { label: '输出 Token', value: formatNumber(overview.value.output_tokens) },
]);

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

function formatTokenPair(inputTokens: number, outputTokens: number) {
  return `${formatNumber(inputTokens)} / ${formatNumber(outputTokens)}`;
}

function formatLatency(value: number | null) {
  return value === null ? '—' : `${formatNumber(value)} ms`;
}
</script>
