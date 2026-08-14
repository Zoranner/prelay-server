<script setup lang="ts">
import type { ModelStats, ProviderStats, StatsOverview } from "~/stores/relay";

const { error, pending, invokeCommand } = useRelayCommand();
const overview = ref<StatsOverview | null>(null);
const models = ref<ModelStats[]>([]);
const providers = ref<ProviderStats[]>([]);

function percentage(part: number, total: number) {
  return total ? `${((part / total) * 100).toFixed(1)}%` : "-";
}

async function load() {
  try {
    const [overviewValue, modelRows, providerRows] = await Promise.all([
      invokeCommand<StatsOverview>("stats_overview"),
      invokeCommand<ModelStats[]>("stats_models"),
      invokeCommand<ProviderStats[]>("stats_providers"),
    ]);
    overview.value = overviewValue;
    models.value = modelRows;
    providers.value = providerRows;
  } catch {
    // The command composable exposes the error to this view.
  }
}

onMounted(load);
</script>

<template>
  <main class="page">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="page-heading">统计</h1>
        <p class="page-subheading">仅聚合当前电脑与 Windows 账户身份下的请求。</p>
      </div>
      <button class="button-secondary" :disabled="pending" @click="load">刷新</button>
    </div>
    <p v-if="error" class="mt-5 border border-rose-900 bg-rose-950/40 p-3 text-sm text-rose-200">{{ error.message }}</p>

    <section v-if="overview" class="mt-6 grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
      <StatsMetricCard label="总请求" :value="overview.total_requests" />
      <StatsMetricCard label="成功请求" :value="overview.successful_requests" />
      <StatsMetricCard label="失败请求" :value="overview.failed_requests" />
      <StatsMetricCard label="输入 Token" :value="overview.input_tokens" />
      <StatsMetricCard label="输出 Token" :value="overview.output_tokens" />
    </section>
    <p v-else-if="!pending" class="empty-state mt-6">暂无统计数据。</p>

    <section class="mt-9 grid gap-8 lg:grid-cols-2">
      <div>
        <h2 class="mb-3 font-medium text-white">按模型</h2>
        <div v-if="models.length" class="overflow-x-auto border border-slate-800">
          <table class="w-full min-w-[32rem] text-left text-sm">
            <thead class="bg-slate-900 text-slate-400"><tr><th>模型</th><th>请求</th><th>成功率</th><th>平均延迟</th></tr></thead>
            <tbody><tr v-for="row in models" :key="row.model_requested ?? 'unknown'" class="border-t border-slate-800"><td>{{ row.model_requested ?? "未标记" }}</td><td>{{ row.total_requests }}</td><td>{{ percentage(row.successful_requests, row.total_requests) }}</td><td>{{ row.average_latency_ms?.toFixed(0) ?? "-" }} ms</td></tr></tbody>
          </table>
        </div>
        <p v-else class="empty-state">暂无模型统计。</p>
      </div>
      <div>
        <h2 class="mb-3 font-medium text-white">按供应商</h2>
        <div v-if="providers.length" class="overflow-x-auto border border-slate-800">
          <table class="w-full min-w-[32rem] text-left text-sm">
            <thead class="bg-slate-900 text-slate-400"><tr><th>供应商</th><th>请求</th><th>成功率</th><th>首 Token</th></tr></thead>
            <tbody><tr v-for="row in providers" :key="row.provider_id ?? 'unknown'" class="border-t border-slate-800"><td>{{ row.provider_name ?? "未标记" }}</td><td>{{ row.total_requests }}</td><td>{{ percentage(row.successful_requests, row.total_requests) }}</td><td>{{ row.average_first_token_ms?.toFixed(0) ?? "-" }} ms</td></tr></tbody>
          </table>
        </div>
        <p v-else class="empty-state">暂无供应商统计。</p>
      </div>
    </section>
  </main>
</template>
