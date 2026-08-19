<script setup lang="ts">
import type {
  ModelStats,
  ProviderStats,
  RequestLog,
  StatsOverview,
} from "~/stores/relay";
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";
import { formatDiagnosticMetadata } from "~/utils/diagnosticMetadata";

const { error, pending, invokeCommand } = useRelayCommand();
const overview = ref<StatsOverview | null>(null);
const models = ref<ModelStats[]>([]);
const providers = ref<ProviderStats[]>([]);
const requests = ref<RequestLog[]>([]);
const activeView = ref<"overview" | "requests">("overview");
const limit = ref(100);
const statusFilter = ref<"all" | "success" | "failed">("all");
const statusOptions: Array<"all" | "success" | "failed"> = [
  "all",
  "success",
  "failed",
];

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
const visibleRequests = computed(() =>
  statusFilter.value === "all"
    ? requests.value
    : requests.value.filter((row) => row.status === statusFilter.value),
);
const metadataDetail = (metadata: string | null) =>
  formatDiagnosticMetadata(metadata);

async function loadOverview() {
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
async function loadRequests() {
  try {
    requests.value = await invokeCommand<RequestLog[]>("stats_requests", {
      limit: limit.value,
    });
  } catch {
    /* The command composable exposes the error to this view. */
  }
}
function selectView(view: "overview" | "requests") {
  activeView.value = view;
  void (view === "overview" ? loadOverview() : loadRequests());
}
function refresh() {
  return activeView.value === "overview" ? loadOverview() : loadRequests();
}
onMounted(loadOverview);
</script>

<template>
  <PageShell>
    <PageHeader
      title="活动"
      description="查看当前身份下的请求概览、明细和错误记录。"
      ><template #actions
        ><button class="button-secondary" :disabled="pending" @click="refresh">
          {{ pending ? "刷新中..." : "刷新" }}
        </button></template
      ></PageHeader
    >
    <p v-if="error" class="notice notice--error">{{ error.message }}</p>
    <div class="flex flex-wrap gap-2">
      <button
        class="button-secondary !px-3 !py-1.5 !text-xs"
        :class="
          activeView === 'overview'
            ? '!border-[#b7d8cf] !bg-[#e8f4f0] !text-[#176b5d]'
            : ''
        "
        @click="selectView('overview')"
      >
        概览
      </button>
      <button
        class="button-secondary !px-3 !py-1.5 !text-xs"
        :class="
          activeView === 'requests'
            ? '!border-[#b7d8cf] !bg-[#e8f4f0] !text-[#176b5d]'
            : ''
        "
        @click="selectView('requests')"
      >
        请求明细
      </button>
    </div>
    <SurfacePanel>
      <div v-if="activeView === 'overview'" class="space-y-8 overflow-auto p-5">
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
      <div v-else class="space-y-5 overflow-auto p-5">
        <div class="flex flex-wrap items-center gap-2">
          <select
            v-model.number="limit"
            class="table-control w-24"
            aria-label="显示条数"
          >
            <option :value="50">50 条</option>
            <option :value="100">100 条</option>
            <option :value="200">200 条</option>
          </select>
          <button
            v-for="option in statusOptions"
            :key="option"
            class="button-secondary !px-3 !py-1.5 !text-xs"
            :class="
              statusFilter === option
                ? '!border-[#b7d8cf] !bg-[#e8f4f0] !text-[#176b5d]'
                : ''
            "
            @click="statusFilter = option"
          >
            {{
              option === "all" ? "全部" : option === "success" ? "成功" : "失败"
            }}
          </button>
        </div>
        <div v-if="visibleRequests.length" class="table-scroll">
          <table class="data-table min-w-[70rem]">
            <thead>
              <tr>
                <th class="text-left">时间</th>
                <th class="text-left">协议</th>
                <th class="text-left">供应商 / 模型</th>
                <th class="text-left">状态</th>
                <th class="text-left">延迟</th>
                <th class="text-left">错误</th>
                <th class="text-left">上游请求</th>
                <th class="text-left">元数据</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in visibleRequests"
                :key="row.id"
                class="align-top text-stone-600 hover:bg-stone-50"
              >
                <td class="whitespace-nowrap">
                  {{ new Date(row.created_at).toLocaleString() }}
                </td>
                <td>
                  {{ row.protocol_in ?? "-" }} →
                  {{ row.protocol_upstream ?? "-" }}
                </td>
                <td>
                  {{ row.provider_name ?? "-" }}<br /><span
                    class="font-mono text-xs text-stone-400"
                    >{{ row.model_requested ?? "-" }}</span
                  >
                </td>
                <td>
                  <span
                    class="rounded-full px-2 py-0.5 text-xs"
                    :class="
                      row.status === 'failed'
                        ? 'bg-red-50 text-red-700'
                        : 'bg-[#f2f8f5] text-[#256047]'
                    "
                    >{{ row.http_status ?? row.status }}</span
                  >
                </td>
                <td>{{ row.latency_ms ?? "-" }} ms</td>
                <td>
                  <span class="text-red-700">{{ row.error_code ?? "" }}</span
                  ><br />{{ row.error_message ?? "" }}
                </td>
                <td class="font-mono text-xs text-stone-500">
                  {{ row.upstream_request_id ?? "-" }}
                </td>
                <td class="max-w-sm">
                  <details
                    v-if="metadataDetail(row.metadata_json)"
                    class="text-xs"
                  >
                    <summary class="cursor-pointer text-[#176b5d]">
                      查看元数据
                    </summary>
                    <pre
                      class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-stone-500"
                      >{{ metadataDetail(row.metadata_json) }}</pre>
                  </details>
                  <span v-else>-</span>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <p
          v-else-if="!pending"
          class="flex min-h-40 items-center justify-center text-sm text-stone-400"
        >
          暂无请求记录。
        </p>
      </div>
    </SurfacePanel>
  </PageShell>
</template>
