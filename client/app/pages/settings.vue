<script setup lang="ts">
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";

const settings = useRelaySettings();
const { error, pending } = useRelayCommand();
const relayUrl = ref(settings.relayUrl.value ?? "");
const saved = ref(false);

watch(settings.relayUrl, (value) => {
  relayUrl.value = value ?? "";
});

async function save() {
  saved.value = false;
  try {
    await settings.save(relayUrl.value);
    saved.value = true;
  } catch {
    // The command composable exposes the stable error to this view.
  }
}
</script>

<template>
  <PageShell>
    <PageHeader title="设置" description="维护此客户端连接的管理服务地址。" />
    <SurfacePanel>
      <form class="settings-form" @submit.prevent="save">
        <label class="field">
          管理服务地址
          <input
            v-model.trim="relayUrl"
            autocomplete="url"
            inputmode="url"
            required
            type="url"
          />
        </label>
        <p class="text-sm leading-6 text-stone-500">
          保存后，新的管理请求将使用此地址。当前设备凭据不会显示在页面中。
        </p>
        <p v-if="error" class="notice notice--error">{{ error.message }}</p>
        <p v-else-if="saved" class="notice">服务地址已保存。</p>
        <div>
          <button class="button-primary" :disabled="pending" type="submit">
            {{ pending ? "正在保存..." : "保存服务地址" }}
          </button>
        </div>
      </form>
    </SurfacePanel>
  </PageShell>
</template>
