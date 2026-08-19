<script setup lang="ts">
import type { BootstrapState } from "~/stores/relay";
import { useRelayStore } from "~/stores/relay";
import { PageHeader, PageShell, SurfacePanel } from "~/components/base";

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
  <PageShell>
    <PageHeader
      title="Provider Relay"
      description="在此电脑上管理供应商和接口配置。AI 工具使用接口根 /v1/ 地址。"
    >
      <template #actions
        ><NuxtLink class="button-primary no-underline" to="/providers"
          >管理供应商</NuxtLink
        ><NuxtLink class="button-secondary no-underline" to="/interfaces"
          >管理接口</NuxtLink
        ></template
      >
    </PageHeader>
    <SurfacePanel>
      <section class="grid min-h-0 gap-8 p-6 lg:grid-cols-[1.2fr_0.8fr]">
        <div class="space-y-4">
          <h2 class="font-semibold text-stone-800">本机管理状态</h2>
          <p class="max-w-xl text-sm leading-6 text-stone-500">
            供应商密钥仅在保存时通过原生层传递，页面不会保存认证凭据。接口配置仅归属当前
            Windows 账户和这台电脑。
          </p>
        </div>
        <div class="rounded-lg border border-stone-200 bg-stone-50 p-5">
          <h2 class="font-semibold text-stone-800">本机身份</h2>
          <p v-if="pending && !bootstrap" class="mt-3 text-sm text-stone-500">
            正在读取本机状态。
          </p>
          <template v-else-if="bootstrap">
            <dl class="mt-4 grid gap-3 text-sm text-stone-700">
              <div>
                <dt class="text-stone-500">Windows 账户</dt>
                <dd class="mt-1">{{ bootstrap.username || "未提供" }}</dd>
              </div>
              <div>
                <dt class="text-stone-500">设备凭据</dt>
                <dd class="mt-1">
                  {{ bootstrap.has_device_credential ? "已保存" : "尚未注册" }}
                </dd>
              </div>
            </dl>
            <button
              class="button-secondary mt-5"
              :disabled="rotatingCredential"
              @click="rotateCredential"
            >
              {{ rotatingCredential ? "轮换中" : "轮换设备凭据" }}
            </button>
          </template>
          <p v-else-if="error" class="mt-3 text-sm text-red-700">
            {{ error.message }}
          </p>
        </div>
      </section>
    </SurfacePanel>
  </PageShell>
</template>
