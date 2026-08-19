type RelaySettingsResponse = {
  relay_url: string | null;
};

export function useRelaySettings() {
  const relayUrl = useState<string | null>("relay-settings-url", () => null);
  const loaded = useState("relay-settings-loaded", () => false);
  const { invokeCommand } = useRelayCommand();

  async function load() {
    const settings =
      await invokeCommand<RelaySettingsResponse>("relay_settings_get");
    relayUrl.value = settings.relay_url;
    loaded.value = true;
    return settings;
  }

  async function save(value: string) {
    const settings = await invokeCommand<RelaySettingsResponse>(
      "relay_settings_save",
      { relayUrl: value },
    );
    relayUrl.value = settings.relay_url;
    loaded.value = true;
    return settings;
  }

  return { relayUrl, loaded, load, save };
}
