<script setup lang="ts">
import type { RelayInterface } from "~/stores/relay";

defineProps<{ interfaces: RelayInterface[]; endpoint: string; pending?: boolean }>();
const emit = defineEmits<{
  edit: [item: RelayInterface];
  remove: [item: RelayInterface];
  regenerate: [item: RelayInterface];
  copy: [value: string];
}>();
</script>

<template>
  <div v-if="interfaces.length" class="divide-y divide-slate-800 border-y border-slate-800">
    <article v-for="item in interfaces" :key="item.id" class="py-4">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 class="font-medium">{{ item.name }}</h3>
          <p class="mt-1 text-sm text-slate-400">{{ item.protocol }} · {{ item.models.length }} 个模型映射</p>
        </div>
        <div class="flex flex-wrap gap-2">
          <button class="button-secondary" :disabled="pending" @click="emit('edit', item)">编辑</button>
          <button class="button-secondary" :disabled="pending" @click="emit('regenerate', item)">重置 Token</button>
          <button class="button-danger" :disabled="pending" @click="emit('remove', item)">删除</button>
        </div>
      </div>
      <div class="mt-3 grid gap-2 text-sm sm:grid-cols-2">
        <button class="copy-value" @click="emit('copy', item.token)">Token：{{ item.token }}</button>
        <button class="copy-value" @click="emit('copy', endpoint)">地址：{{ endpoint }}</button>
      </div>
    </article>
  </div>
  <p v-else class="empty-state">尚未创建接口。</p>
</template>
