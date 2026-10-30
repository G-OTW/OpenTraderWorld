// Whether the floating Agent shortcut is visible. The backend default is enabled.
export const agentQuickButton = $state({ show: true, loaded: false });

export function hydrateAgentQuickButton(value) {
  agentQuickButton.show = value !== false;
  agentQuickButton.loaded = true;
}
