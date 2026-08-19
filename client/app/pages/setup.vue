<script setup lang="ts">
const settings = useRelaySettings();
const { error, pending } = useRelayCommand();
const relayUrl = ref("");

async function save() {
  try {
    await settings.save(relayUrl.value);
    await navigateTo("/");
  } catch {
    // The command composable exposes the stable error to this view.
  }
}
</script>

<template>
  <main class="setup-screen">
    <form class="setup-form" @submit.prevent="save">
      <p class="setup-form__eyebrow">Provider Relay</p>
      <h1>连接管理服务</h1>
      <p>
        输入部署的服务地址。供应商配置、接口与请求记录将按当前 Windows
        身份保存在该服务中。
      </p>
      <label class="field">
        服务地址
        <input
          v-model.trim="relayUrl"
          autocomplete="url"
          inputmode="url"
          placeholder="https://relay.example.com"
          required
          type="url"
        />
      </label>
      <p v-if="error" class="notice notice--error">{{ error.message }}</p>
      <button class="button-primary" :disabled="pending" type="submit">
        {{ pending ? "正在保存..." : "继续" }}
      </button>
    </form>
  </main>
</template>
