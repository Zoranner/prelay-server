<script setup lang="ts">
import type { BootstrapState } from "~/stores/relay";
import { useRelayStore } from "~/stores/relay";

const { bootstrap, setBootstrap } = useRelayStore();
const { error, pending, invokeCommand } = useRelayCommand();
const rotatingCredential = ref(false);

async function loadBootstrap() {
  try {
    setBootstrap(await invokeCommand<BootstrapState>("bootstrap"));
  } catch {
    // The command composable keeps the stable error for the view.
  }
}

async function rotateCredential() {
  rotatingCredential.value = true;
  try {
    await invokeCommand("credential_rotate");
    await loadBootstrap();
  } catch {
    // The command composable keeps the stable error for the view.
  } finally {
    rotatingCredential.value = false;
  }
}

onMounted(loadBootstrap);
</script>

<template>
  <main class="page">
    <section class="grid gap-6 lg:grid-cols-[1.25fr_0.75fr]">
      <div>
        <p class="text-sm font-medium text-cyan-300">桌面端管理</p>
        <h1 class="mt-2 text-3xl font-semibold text-white">Provider Relay</h1>
        <p class="mt-3 max-w-2xl text-slate-400">
          在此电脑上管理供应商和接口配置。AI 工具直接使用接口的根 <code>/v1/</code> 地址。
        </p>
        <div class="mt-6 flex flex-wrap gap-3">
          <NuxtLink class="button-primary" to="/providers">管理供应商</NuxtLink>
          <NuxtLink class="button-secondary" to="/interfaces">管理接口</NuxtLink>
        </div>
      </div>
      <section class="panel">
        <h2 class="font-medium text-white">本机身份</h2>
        <p v-if="pending && !bootstrap" class="mt-3 text-sm text-slate-400">正在读取本机状态。</p>
        <template v-else-if="bootstrap">
          <dl class="mt-4 grid gap-3 text-sm">
            <div><dt class="text-slate-500">Windows 账户</dt><dd>{{ bootstrap.username || "未提供" }}</dd></div>
            <div><dt class="text-slate-500">设备凭据</dt><dd>{{ bootstrap.has_device_credential ? "已保存" : "尚未注册" }}</dd></div>
          </dl>
          <button class="button-secondary mt-5" :disabled="rotatingCredential" @click="rotateCredential">
            {{ rotatingCredential ? "轮换中" : "轮换设备凭据" }}
          </button>
        </template>
        <p v-else-if="error" class="mt-3 text-sm text-rose-300">{{ error.message }}</p>
      </section>
    </section>
  </main>
</template>
