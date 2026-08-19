<script setup lang="ts">
import type { ModelStats, ProviderStats, StatsOverview } from "~/stores/relay";
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";

const { error, pending, invokeCommand } = useRelayCommand();
const overview = ref<StatsOverview | null>(null);
const models = ref<ModelStats[]>([]);
const providers = ref<ProviderStats[]>([]);

const metrics = computed(() =>
  overview.value
    ? [
        ["总请求", overview.value.total_requests],
        ["成功", overview.value.successful_requests],
        ["失败", overview.value.failed_requests],
        ["输入 Token", overview.value.input_tokens],
        ["输出 Token", overview.value.output_tokens],
      ]
    : [],
);

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
    /* The command composable exposes the error to this view. */
  }
}
onMounted(load);
</script>

<template>
  <PageShell>
    <PageHeader
      title="统计"
      description="仅聚合当前电脑与 Windows 账户身份下的请求。"
      ><template #actions
        ><button class="button-secondary" :disabled="pending" @click="load">
          {{ pending ? "刷新中..." : "刷新" }}
        </button></template
      ></PageHeader
    >
    <p v-if="error" class="notice notice--error">{{ error.message }}</p>
    <SurfacePanel>
      <div class="space-y-8 overflow-auto p-5">
        <div
          v-if="metrics.length"
          class="grid gap-3 sm:grid-cols-2 xl:grid-cols-5"
        >
          <div
            v-for="[label, value] in metrics"
            :key="String(label)"
            class="rounded-lg border border-stone-200 bg-stone-50 p-4"
          >
            <p class="text-sm text-stone-500">{{ label }}</p>
            <strong class="mt-2 block text-2xl font-semibold text-stone-800">{{
              value
            }}</strong>
          </div>
        </div>
        <p
          v-else-if="!pending"
          class="py-10 text-center text-sm text-stone-400"
        >
          暂无统计数据。
        </p>
        <div class="grid gap-8 lg:grid-cols-2">
          <section>
            <div class="mb-3 flex items-center justify-between">
              <h2 class="text-sm font-semibold text-stone-700">模型统计</h2>
              <span class="text-xs text-stone-400">按请求模型聚合</span>
            </div>
            <div class="overflow-x-auto rounded-lg border border-stone-200">
              <table class="data-table min-w-[32rem]">
                <thead>
                  <tr>
                    <th class="text-left">模型</th>
                    <th class="text-right">请求</th>
                    <th class="text-right">成功率</th>
                    <th class="text-right">平均延迟</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-if="!models.length">
                    <td colspan="4" class="text-center text-stone-400">
                      暂无模型统计
                    </td>
                  </tr>
                  <tr
                    v-for="row in models"
                    :key="row.model_requested ?? 'unknown'"
                    class="text-stone-600 hover:bg-stone-50"
                  >
                    <td class="font-mono text-xs">
                      {{ row.model_requested ?? "-" }}
                    </td>
                    <td class="text-right">{{ row.total_requests }}</td>
                    <td class="text-right">
                      {{
                        percentage(row.successful_requests, row.total_requests)
                      }}
                    </td>
                    <td class="text-right">
                      {{ row.average_latency_ms?.toFixed(0) ?? "-" }} ms
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>
          <section>
            <div class="mb-3 flex items-center justify-between">
              <h2 class="text-sm font-semibold text-stone-700">
                Provider 统计
              </h2>
              <span class="text-xs text-stone-400">按上游聚合</span>
            </div>
            <div class="overflow-x-auto rounded-lg border border-stone-200">
              <table class="data-table min-w-[32rem]">
                <thead>
                  <tr>
                    <th class="text-left">Provider</th>
                    <th class="text-right">请求</th>
                    <th class="text-right">成功率</th>
                    <th class="text-right">首 Token</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-if="!providers.length">
                    <td colspan="4" class="text-center text-stone-400">
                      暂无 Provider 统计
                    </td>
                  </tr>
                  <tr
                    v-for="row in providers"
                    :key="row.provider_id ?? 'unknown'"
                    class="text-stone-600 hover:bg-stone-50"
                  >
                    <td>{{ row.provider_name ?? "-" }}</td>
                    <td class="text-right">{{ row.total_requests }}</td>
                    <td class="text-right">
                      {{
                        percentage(row.successful_requests, row.total_requests)
                      }}
                    </td>
                    <td class="text-right">
                      {{ row.average_first_token_ms?.toFixed(0) ?? "-" }} ms
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>
        </div>
      </div>
    </SurfacePanel>
  </PageShell>
</template>
