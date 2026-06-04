<template>
  <div class="px-6 py-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <Input v-model="form.alias" label="下游模型别名" placeholder="例如：coder" mono />
      <Input v-model="form.upstream_model" label="上游模型" placeholder="例如：deepseek-chat" mono />
    </div>

    <Input v-model="form.provider_id" label="Provider ID" placeholder="填写已有配置的 ID" mono />

    <Input
      v-model="protocolsText"
      label="下游协议"
      placeholder="responses, chat_completions, anthropic_messages"
      mono
    />

    <Alert v-if="message" :type="message.type">
      {{ message.text }}
    </Alert>

    <Button block :loading="creating" @click="submit">
      {{ creating ? '创建中…' : '创建模型别名' }}
    </Button>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import { configApi } from '../api';
import { Alert, Button, Input } from './base';

const form = ref({
  alias: '',
  provider_id: '',
  upstream_model: '',
});
const protocolsText = ref('responses, chat_completions, anthropic_messages');
const creating = ref(false);
const message = ref<{ type: 'success' | 'error'; text: string } | null>(null);

async function submit() {
  message.value = null;

  if (!form.value.alias.trim() || !form.value.provider_id.trim() || !form.value.upstream_model.trim()) {
    message.value = { type: 'error', text: '请填写别名、Provider ID 和上游模型。' };
    return;
  }

  creating.value = true;
  try {
    const protocols = protocolsText.value
      .split(',')
      .map((protocol) => protocol.trim())
      .filter(Boolean);
    const response = await configApi.createModelAlias({
      alias: form.value.alias.trim(),
      provider_id: form.value.provider_id.trim(),
      upstream_model: form.value.upstream_model.trim(),
      downstream_protocols: protocols,
    });
    message.value = { type: 'success', text: `模型别名 ${response.data.alias} 已创建。` };
    form.value.alias = '';
    form.value.upstream_model = '';
  } catch {
    message.value = { type: 'error', text: '模型别名创建失败，请检查 Provider ID 和别名是否重复。' };
  } finally {
    creating.value = false;
  }
}
</script>
