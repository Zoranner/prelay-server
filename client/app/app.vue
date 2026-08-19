<script setup lang="ts">
import { RefreshCw, WifiOff } from "lucide-vue-next";

const tabs = [
  { label: "统计", path: "/stats" },
  { label: "供应商", path: "/providers" },
  { label: "接口", path: "/interfaces" },
];

const managementApi = useRelayManagementApiStatus();

function reloadApplication() {
  window.location.reload();
}
</script>

<template>
  <div class="app-root">
    <header class="app-header">
      <div
        class="app-frame flex flex-col gap-4 px-6 py-4 lg:flex-row lg:items-center lg:justify-between"
      >
        <NuxtLink class="shrink-0 no-underline" to="/">
          <h1 class="text-xl font-semibold text-stone-900">Provider Relay</h1>
          <p class="mt-1 text-xs tracking-wide text-stone-500">
            大模型服务透传代理
          </p>
        </NuxtLink>
        <nav class="app-nav" aria-label="主导航">
          <NuxtLink
            v-for="tab in tabs"
            :key="tab.path"
            class="app-nav__link"
            :to="tab.path"
          >
            {{ tab.label }}
          </NuxtLink>
        </nav>
      </div>
    </header>
    <main class="min-h-0 flex-1 overflow-hidden">
      <div class="app-frame h-full min-h-0 px-6 py-8">
        <NuxtPage />
      </div>
    </main>
    <section
      v-if="managementApi.error"
      class="app-unavailable"
      role="alert"
      aria-live="assertive"
    >
      <div class="app-unavailable__content">
        <WifiOff :size="32" stroke-width="1.75" aria-hidden="true" />
        <h2>无法连接管理服务</h2>
        <p>当前无法访问 Provider Relay 管理 API。</p>
        <p class="app-unavailable__hint">
          请检查网络连接和服务地址，然后重新加载。
        </p>
        <button class="button-primary" type="button" @click="reloadApplication">
          <RefreshCw :size="16" aria-hidden="true" />
          重新加载
        </button>
      </div>
    </section>
  </div>
</template>
