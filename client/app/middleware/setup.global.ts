export default defineNuxtRouteMiddleware(async (to) => {
  if (import.meta.server) return;

  const settings = useRelaySettings();
  if (!settings.loaded.value) {
    try {
      await settings.load();
    } catch {
      return;
    }
  }

  if (!settings.relayUrl.value && to.path !== "/setup") {
    return navigateTo("/setup");
  }
  if (settings.relayUrl.value && to.path === "/setup") {
    return navigateTo("/");
  }
});
