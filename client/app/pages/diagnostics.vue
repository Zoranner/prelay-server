<script setup lang="ts">
import type { RequestLog } from "~/stores/relay";
import { formatDiagnosticMetadata } from "~/utils/diagnosticMetadata";

const { error, pending, invokeCommand } = useRelayCommand();
const requests = ref<RequestLog[]>([]);
const limit = ref(100);
const statusFilter = ref<"all" | "success" | "failed">("all");
const statusOptions: Array<"all" | "success" | "failed"> = ["all", "success", "failed"];

const visibleRequests = computed(() =>
  statusFilter.value === "all" ? requests.value : requests.value.filter((row) => row.status === statusFilter.value),
);

function metadataDetail(metadata: string | null): string | null {
  return formatDiagnosticMetadata(metadata);
}

async function load() {
  try {
    requests.value = await invokeCommand<RequestLog[]>("stats_requests", { limit: limit.value });
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
        <h1 class="page-heading">诊断</h1>
        <p class="page-subheading">查看当前身份最近请求的协议、错误与延迟。</p>
      </div>
      <div class="flex gap-2">
        <select v-model.number="limit" aria-label="显示条数"><option :value="50">50 条</option><option :value="100">100 条</option><option :value="200">200 条</option></select>
        <button class="button-secondary" :disabled="pending" @click="load">刷新</button>
      </div>
    </div>
    <div class="mt-5 flex flex-wrap gap-2">
      <button v-for="option in statusOptions" :key="option" class="button-secondary" :class="{ 'border-cyan-500 text-cyan-200': statusFilter === option }" @click="statusFilter = option">
        {{ option === "all" ? "全部" : option === "success" ? "成功" : "失败" }}
      </button>
    </div>
    <p v-if="error" class="mt-5 border border-rose-900 bg-rose-950/40 p-3 text-sm text-rose-200">{{ error.message }}</p>
    <div v-else-if="visibleRequests.length" class="mt-6 overflow-x-auto border border-slate-800">
      <table class="w-full min-w-[65rem] text-left text-sm">
        <thead class="bg-slate-900 text-slate-400"><tr><th>时间</th><th>协议</th><th>供应商 / 模型</th><th>状态</th><th>延迟</th><th>错误</th><th>上游请求</th><th>元数据</th></tr></thead>
        <tbody>
          <tr v-for="row in visibleRequests" :key="row.id" class="border-t border-slate-800 align-top">
            <td>{{ new Date(row.created_at).toLocaleString() }}</td>
            <td>{{ row.protocol_in ?? "-" }} → {{ row.protocol_upstream ?? "-" }}</td>
            <td>{{ row.provider_name ?? "-" }}<br /><span class="text-slate-500">{{ row.model_requested ?? "-" }}</span></td>
            <td :class="row.status === 'failed' ? 'text-rose-300' : 'text-emerald-300'">{{ row.http_status ?? row.status }}</td>
            <td>{{ row.latency_ms ?? "-" }} ms</td>
            <td><span class="text-rose-300">{{ row.error_code ?? "" }}</span><br />{{ row.error_message ?? "" }}</td>
            <td class="font-mono text-xs text-slate-400">{{ row.upstream_request_id ?? "-" }}</td>
            <td class="max-w-sm">
              <details v-if="metadataDetail(row.metadata_json)" class="text-xs">
                <summary class="cursor-pointer text-cyan-300">查看元数据</summary>
                <pre class="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-slate-400">{{ metadataDetail(row.metadata_json) }}</pre>
              </details>
              <span v-else>-</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <p v-else-if="!pending" class="empty-state mt-6">暂无请求记录。</p>
  </main>
</template>
