import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import { invoke } from '@tauri-apps/api/core';
import { useUpdaterStore } from '../../src/stores/updater';
import { useSettingsStore } from '../../src/stores/settings';
import { defaultSettings } from '../../src/tauri/types/config';
import SettingsUpdate from '../../src/components/settings/SettingsUpdate.vue';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../../src/i18n', () => ({ getDefaultLocale: () => ('zh-CN'), i18n: { global: { locale: { value: 'zh-CN' } } } }));
vi.mock('vue-i18n', async importOriginal => ({
  ...await importOriginal<typeof import('vue-i18n')>(),
  useI18n: () => ({ t: (key: string) => key }),
}));
const releaseUrl = 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases';
const configured = { configured: true, reason: null, releasesUrl: releaseUrl };

beforeEach(() => {
  setActivePinia(createPinia());
  vi.mocked(invoke).mockReset();
  useSettingsStore().settings = structuredClone(defaultSettings);
});

describe('application updater', () => {
  it('loads configuration even when automatic checks are disabled', async () => {
    useSettingsStore().settings.update.updateApp = false;
    vi.mocked(invoke).mockResolvedValue({ ...configured, configured: false, reason: '未配置自有更新公钥' });
    const store = useUpdaterStore();
    await store.check();
    expect(invoke).toHaveBeenCalledWith('updater_status');
    expect(invoke).not.toHaveBeenCalledWith('updater_check');
    expect(store.configuration?.configured).toBe(false);
  });

  it('never checks an unconfigured updater and disables only application updates', async () => {
    vi.mocked(invoke).mockResolvedValue({ ...configured, configured: false, reason: '未配置自有更新公钥' });
    const store = useUpdaterStore();
    await store.check();
    const wrapper = mount(SettingsUpdate, { props: { modelValue: structuredClone(defaultSettings) } });
    expect(wrapper.get('#updateApp').attributes('disabled')).toBeDefined();
    expect(wrapper.get('#updateBinaries').attributes('disabled')).toBeUndefined();
    expect(wrapper.text()).toContain('未配置自有更新公钥');
    expect(wrapper.get('a').attributes('href')).toBe(releaseUrl);
    expect(invoke).not.toHaveBeenCalledWith('updater_check');
  });

  it('clears stale frontend state on check failure', async () => {
    useSettingsStore().settings.update.updateApp = true;
    const store = useUpdaterStore();
    store.checkResult = { ...configured, available: true, currentVersion: '1.0.0', availableVersion: '2.0.0' };
    store.isNeedingRestart = true;
    vi.mocked(invoke).mockResolvedValueOnce(configured).mockRejectedValueOnce(new Error('offline'));
    await store.check();
    expect(store.checkResult).toBeUndefined();
    expect(store.isNeedingRestart).toBe(false);
    expect(store.lastError).toContain('offline');
  });

  it('checks and downloads a configured update before installation', async () => {
    useSettingsStore().settings.update.updateApp = true;
    vi.mocked(invoke).mockResolvedValueOnce(configured)
      .mockResolvedValueOnce({ ...configured, available: true, currentVersion: '1.0.0', availableVersion: '2.0.0' })
      .mockResolvedValue(undefined);
    const store = useUpdaterStore();
    await store.check();
    expect(store.checkResult?.available).toBe(true);
    await store.download();
    expect(store.isNeedingRestart).toBe(true);
    await store.install();
    expect(invoke).toHaveBeenCalledWith('updater_install');
    expect(store.isNeedingRestart).toBe(false);
  });

  it('clears pending restart when no update is available', async () => {
    useSettingsStore().settings.update.updateApp = true;
    vi.mocked(invoke).mockResolvedValueOnce(configured)
      .mockResolvedValueOnce({ ...configured, available: false, currentVersion: '1.0.0', availableVersion: null });
    const store = useUpdaterStore();
    store.isNeedingRestart = true;
    await store.check();
    expect(store.checkResult?.available).toBe(false);
    expect(store.isNeedingRestart).toBe(false);
  });

  it('clears stale state when installation fails', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('install failed'));
    const store = useUpdaterStore();
    store.isNeedingRestart = true;
    await store.install();
    expect(store.isNeedingRestart).toBe(false);
    expect(store.checkResult).toBeUndefined();
    expect(store.lastError).toContain('install failed');
  });

  it('enables the application toggle only after configuration is confirmed', async () => {
    vi.mocked(invoke).mockResolvedValue(configured);
    const store = useUpdaterStore();
    await store.loadConfiguration();
    const settings = structuredClone(defaultSettings);
    const wrapper = mount(SettingsUpdate, { props: { modelValue: settings } });
    expect(wrapper.get('#updateApp').attributes('disabled')).toBeUndefined();
    await wrapper.get('#updateApp').setValue(false);
    expect(settings.update.updateApp).toBe(false);
  });

  it('reports download failure and never leaves stale bytes installable in UI', async () => {
    const store = useUpdaterStore();
    store.isNeedingRestart = true;
    vi.mocked(invoke).mockRejectedValue(new Error('signature rejected'));
    await store.download();
    expect(store.isNeedingRestart).toBe(false);
    expect(store.lastError).toContain('signature rejected');
    expect(store.isUpdating).toBe(false);
  });
});
