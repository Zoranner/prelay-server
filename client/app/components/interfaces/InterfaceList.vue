<script setup lang="ts">
import { Copy, KeyRound, Pencil, Trash2 } from "lucide-vue-next";
import { DataTableShell, IconButton, SurfacePanel } from "~/components/base";
import type { RelayInterface } from "~/stores/relay";

defineProps<{
  interfaces: RelayInterface[];
  endpoint: string;
  pending?: boolean;
}>();
const emit = defineEmits<{
  edit: [item: RelayInterface];
  remove: [item: RelayInterface];
  regenerate: [item: RelayInterface];
  copy: [value: string];
}>();

const expandedInterfaceId = ref<string | null>(null);
function toggle(interfaceId: string) {
  expandedInterfaceId.value =
    expandedInterfaceId.value === interfaceId ? null : interfaceId;
}
</script>

<template>
  <SurfacePanel v-if="!interfaces.length">
    <p
      class="flex min-h-40 items-center justify-center px-6 py-10 text-sm text-stone-400"
    >
      暂无接口
    </p>
  </SurfacePanel>
  <SurfacePanel v-else>
    <DataTableShell table-class="min-w-[48rem]">
      <thead>
        <tr>
          <th class="text-left">名称</th>
          <th class="text-left">协议</th>
          <th class="text-left">模型映射</th>
          <th class="text-left">访问凭据</th>
          <th class="text-right">操作</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="item in interfaces" :key="item.id">
          <tr
            class="selectable-row"
            :class="expandedInterfaceId === item.id ? 'selected-row' : ''"
            @click="toggle(item.id)"
          >
            <td class="font-semibold text-stone-800">{{ item.name }}</td>
            <td>
              <span
                class="rounded-full bg-[#e8f4f0] px-2 py-0.5 text-xs font-medium text-[#176b5d]"
                >{{ item.protocol }}</span
              >
            </td>
            <td class="text-stone-600">{{ item.models.length }} 个模型</td>
            <td>
              <button
                class="copy-tag font-mono text-xs text-stone-600"
                @click.stop="emit('copy', item.token)"
              >
                <Copy class="h-3.5 w-3.5" />复制 Token
              </button>
            </td>
            <td>
              <div class="flex justify-end gap-2">
                <IconButton
                  label="编辑接口"
                  :disabled="pending"
                  @click.stop="emit('edit', item)"
                  ><Pencil class="h-4 w-4" /></IconButton
                ><IconButton
                  label="重置 Token"
                  :disabled="pending"
                  @click.stop="emit('regenerate', item)"
                  ><KeyRound class="h-4 w-4" /></IconButton
                ><IconButton
                  label="删除接口"
                  variant="danger"
                  :disabled="pending"
                  @click.stop="emit('remove', item)"
                  ><Trash2 class="h-4 w-4"
                /></IconButton>
              </div>
            </td>
          </tr>
          <tr v-if="expandedInterfaceId === item.id" class="expanded-row">
            <td colspan="5">
              <div class="grid gap-3 sm:grid-cols-2">
                <button
                  class="copy-tag font-mono text-xs text-stone-700"
                  @click="emit('copy', endpoint)"
                >
                  <Copy class="h-3.5 w-3.5" />{{ endpoint }}
                </button>
                <div class="flex flex-wrap gap-2">
                  <span
                    v-for="model in item.models"
                    :key="
                      model.id ?? `${model.provider_id}-${model.upstream_model}`
                    "
                    class="rounded-full border border-stone-200 bg-white px-3 py-1 font-mono text-xs text-stone-700"
                    >{{ model.model_name }}</span
                  >
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </DataTableShell>
  </SurfacePanel>
</template>
