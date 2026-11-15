import { ensureInvokeArgsObject, IPCHandler } from '../tauriMock';
import { InvokeArgs } from '@tauri-apps/api/core';
import { defaultSettings } from '../../../src/tauri/types/config.ts';

export const configHandlers: Record<string, IPCHandler> = {
  config_get: () => {
    return {
      ...defaultSettings,
      appearance: { ...defaultSettings.appearance, ...window.E2E.settings?.appearance },
      update: { ...defaultSettings.update, ...window.E2E.settings?.update },
    };
  },
  config_set: (_cmd, args: InvokeArgs | undefined) => {
    const { patch } = ensureInvokeArgsObject<{ patch: Record<string, unknown> }>(args, 'config_set');
    return { defaultSettings, ...patch };
  },
};
