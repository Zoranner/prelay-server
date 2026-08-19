<script setup lang="ts">
import type { RequestLog } from "~/stores/relay";
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";
import { formatDiagnosticMetadata } from "~/utils/diagnosticMetadata";

const { error, pending, invokeCommand } = useRelayCommand();
const requests = ref<RequestLog[]>([]);
const limit = ref(100);
const statusFilter = ref<"all" | "success" | "failed">("all");
const statusOptions: Array<"all" | "success" | "failed"> = [
  "all",
  "success",
  "failed",
];
const visibleRequests = computed(() =>
  statusFilter.value === "all"
    ? requests.value
    : requests.value.filter((row) => row.status === statusFilter.value),
);
const metadataDetail = (metadata: string | null) =>
  formatDiagnosticMetadata(metadata);
async function load() {
  try {
    requests.value = await invokeCommand<RequestLog[]>("stats_requests", {
      limit: limit.value,
    });
  } catch {
    /* The command composable exposes the error to this view. */
  }
}
onMounted(load);
</script>

<template>
  <PageShell>
    <PageHeader
      title="诊断"
      description="查看当前身份最近请求的协议、错误与延迟。"
    >
      <template #actions
        ><select
          v-model.number="limit"
          class="table-control w-24"
          aria-label="显示条数"
        >
          <option :value="50">50 条</option>
          <option :value="100">100 条</option>
          <option :value="200">200 条</option></select
        ><button class="button-secondary" :disabled="pending" @click="load">
          {{ pending ? "刷新中..." : "刷新" }}
        </button></template
      >
    </PageHeader>
    <div class="flex flex-wrap gap-2">
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
        {{ option === "all" ? "全部" : option === "success" ? "成功" : "失败" }}
      </button>
    </div>
    <p v-if="error" class="notice notice--error">{{ error.message }}</p>
    <SurfacePanel v-else>
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
    </SurfacePanel>
  </PageShell>
</template>
