<script setup lang="ts">
import { Activity, Gauge, Pencil, Search, Trash2 } from "lucide-vue-next";
import type { Provider, UpstreamProtocol } from "~/stores/relay";
import { providerProtocolOptions } from "~/utils/providerCapabilities";
import { DataTableShell, IconButton, SurfacePanel } from "~/components/base";

defineProps<{ providers: Provider[]; pending?: boolean }>();
const emit = defineEmits<{
  edit: [provider: Provider];
  remove: [provider: Provider];
  ping: [provider: Provider];
  discover: [provider: Provider];
  testProtocol: [payload: { provider: Provider; protocol: UpstreamProtocol }];
}>();

const expandedProviderId = ref<string | null>(null);
const protocolSelections = reactive<Record<string, UpstreamProtocol>>({});

function protocolFor(provider: Provider): UpstreamProtocol {
  return (
    protocolSelections[provider.id] ??
    providerProtocolOptions(provider)[0] ??
    "openai"
  );
}

function toggle(providerId: string) {
  expandedProviderId.value =
    expandedProviderId.value === providerId ? null : providerId;
}

function protocolLabel(protocol: UpstreamProtocol) {
  return protocol === "responses"
    ? "Responses"
    : protocol === "anthropic"
      ? "Anthropic"
      : "OpenAI";
}
</script>

<template>
  <SurfacePanel v-if="!providers.length">
    <p
      class="flex min-h-40 items-center justify-center px-6 py-10 text-sm text-stone-400"
    >
      暂无供应商
    </p>
  </SurfacePanel>
  <SurfacePanel v-else>
    <DataTableShell table-class="min-w-[56rem]">
      <thead>
        <tr>
          <th class="text-left">名称</th>
          <th class="text-left">类型</th>
          <th class="text-left">协议</th>
          <th class="text-left">模型</th>
          <th class="text-left">连接</th>
          <th class="text-right">操作</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="provider in providers" :key="provider.id">
          <tr
            class="selectable-row"
            :class="expandedProviderId === provider.id ? 'selected-row' : ''"
            @click="toggle(provider.id)"
          >
            <td>
              <div class="font-semibold text-stone-800">
                {{ provider.name }}
              </div>
              <div
                class="mt-1 max-w-[20rem] truncate font-mono text-xs text-stone-400"
              >
                {{ provider.base_url }}
              </div>
            </td>
            <td>
              <span
                class="rounded-full bg-stone-100 px-2 py-0.5 text-xs font-medium text-stone-600"
                >{{ provider.provider_type }}</span
              >
            </td>
            <td>
              <div class="flex gap-1.5">
                <span
                  v-for="protocol in providerProtocolOptions(provider)"
                  :key="protocol"
                  class="rounded-full border border-stone-200 px-2 py-0.5 text-xs text-stone-600"
                  >{{ protocolLabel(protocol) }}</span
                >
              </div>
            </td>
            <td class="text-stone-600">
              {{ provider.models.length || "待添加" }}
            </td>
            <td>
              <button
                class="button-secondary !px-3 !py-1.5 !text-xs"
                :disabled="pending"
                @click.stop="emit('ping', provider)"
              >
                <Activity class="h-3.5 w-3.5" /> 连通性
              </button>
            </td>
            <td>
              <div class="flex justify-end gap-2">
                <IconButton
                  label="发现模型"
                  :disabled="pending"
                  @click.stop="emit('discover', provider)"
                  ><Search class="h-4 w-4"
                /></IconButton>
                <IconButton
                  label="编辑供应商"
                  :disabled="pending"
                  @click.stop="emit('edit', provider)"
                  ><Pencil class="h-4 w-4"
                /></IconButton>
                <IconButton
                  label="删除供应商"
                  variant="danger"
                  :disabled="pending"
                  @click.stop="emit('remove', provider)"
                  ><Trash2 class="h-4 w-4"
                /></IconButton>
              </div>
            </td>
          </tr>
          <tr v-if="expandedProviderId === provider.id" class="expanded-row">
            <td colspan="6">
              <div class="flex flex-wrap items-center gap-2">
                <span
                  v-for="model in provider.models"
                  :key="model.id"
                  class="rounded-full border border-stone-200 bg-white px-3 py-1 font-mono text-xs text-stone-700"
                  >{{ model.model_name }}</span
                >
                <span
                  v-if="!provider.models.length"
                  class="text-sm text-stone-400"
                  >暂无模型。编辑供应商后添加模型。</span
                >
                <span
                  class="rounded-full border border-stone-200 bg-white px-3 py-1 font-mono text-xs text-stone-500"
                  >密钥：{{ provider.api_key_masked }}</span
                >
                <div class="ml-auto flex items-center gap-2">
                  <select
                    v-model="protocolSelections[provider.id]"
                    class="table-control w-32"
                    :aria-label="`${provider.name} 上游协议`"
                  >
                    <option
                      v-for="protocol in providerProtocolOptions(provider)"
                      :key="protocol"
                      :value="protocol"
                    >
                      {{ protocolLabel(protocol) }}
                    </option>
                  </select>
                  <button
                    class="button-teal !px-3 !py-2 !text-xs"
                    :disabled="pending"
                    @click="
                      emit('testProtocol', {
                        provider,
                        protocol: protocolFor(provider),
                      })
                    "
                  >
                    <Gauge class="h-3.5 w-3.5" />测试协议
                  </button>
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </DataTableShell>
  </SurfacePanel>
</template>
