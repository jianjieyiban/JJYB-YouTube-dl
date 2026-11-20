import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import { invoke } from '@tauri-apps/api/core';
import { useUpdaterStore } from '../../src/stores/updater';
import { useSettingsStore } from '../../src/stores/settings';
import { defaultSettings } from '../../src/tauri/types/config';
import SettingsUpdate from '../../src/components/settings/SettingsUpdate.vue';
import TheUpdater from '../../src/components/TheUpdater.vue';

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }));

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
  it('serializes update and restart and ignores duplicate actions and checks', async () => {
    const store = useUpdaterStore();
    store.checkResult = { ...configured, available: true, currentVersion: '1', availableVersion: '2' };
    let finishDownload!: () => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'updater_download') {
        await new Promise<void>((resolve) => {
          finishDownload = resolve;
        });
      }
    });
    const pending = store.updateAndRestart();
    await store.updateAndRestart();
    await store.check();
    expect(store.checkResult?.available).toBe(true);
    expect(store.isUpdating).toBe(true);
    expect(invoke).toHaveBeenCalledTimes(1);
    store.processUpdaterDownloadProgress({ received: 10, total: 10 });
    expect(store.phase).toBe('verifying');
    expect(invoke).not.toHaveBeenCalledWith('updater_install');
    finishDownload();
    await pending;
    expect(vi.mocked(invoke).mock.calls.map(call => call[0])).toEqual(['updater_download', 'updater_install']);
    expect(store.phase).toBe('preparing-restart');
  });

  it('never installs rejected bytes and retries by checking again even with automatic checks off', async () => {
    const store = useUpdaterStore();
    useSettingsStore().settings.update.updateApp = false;
    store.checkResult = { ...configured, available: true, currentVersion: '1', availableVersion: '2' };
    vi.mocked(invoke).mockRejectedValueOnce(new Error('hash rejected'));
    await store.updateAndRestart();
    expect(invoke).not.toHaveBeenCalledWith('updater_install');
    expect(store.phase).toBe('error');
    expect(store.lastError).toContain('hash rejected');
    vi.mocked(invoke).mockResolvedValueOnce(configured)
      .mockResolvedValueOnce({ ...configured, available: true, currentVersion: '1', availableVersion: '2' })
      .mockResolvedValue(undefined);
    await store.updateAndRestart();
    expect(vi.mocked(invoke).mock.calls.map(call => call[0])).toEqual([
      'updater_download', 'updater_status', 'updater_check', 'updater_download', 'updater_install',
    ]);
  });

  it('does not turn a finished event into hidden success or erase an error', () => {
    const store = useUpdaterStore();
    store.processUpdaterError('active tasks');
    store.processUpdaterFinished();
    expect(store.isIgnored).toBe(false);
    expect(store.lastError).toBe('active tasks');
    expect(store.phase).toBe('error');
  });

  it('shows errors with a check action instead of promising a restart', async () => {
    const store = useUpdaterStore();
    store.processUpdaterError('HTTP 403');
    vi.mocked(invoke).mockResolvedValueOnce(configured)
      .mockResolvedValueOnce({ ...configured, available: false, currentVersion: '1', availableVersion: null });
    const wrapper = mount(TheUpdater);
    expect(wrapper.text()).toContain('HTTP 403');
    expect(wrapper.text()).not.toContain('updater.updateAndRestart');
    expect(wrapper.text()).not.toContain('updater.install.subtitle');
    const check = wrapper.findAll('button').find(button => button.text() === 'updater.check');
    expect(check).toBeDefined();
    await check!.trigger('click');
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('updater_check'));
    expect(invoke).not.toHaveBeenCalledWith('updater_download');
    expect(invoke).not.toHaveBeenCalledWith('updater_install');
    wrapper.unmount();
  });

  it('does not offer restart in settings after a failed check', async () => {
    const store = useUpdaterStore();
    vi.mocked(invoke).mockResolvedValueOnce(configured).mockRejectedValueOnce(new Error('HTTP 403'));
    await store.check(true);
    vi.mocked(invoke).mockResolvedValue(configured);
    const wrapper = mount(SettingsUpdate, { props: { modelValue: structuredClone(defaultSettings) } });
    expect(wrapper.text()).toContain('HTTP 403');
    expect(wrapper.text()).toContain('updater.check');
    expect(wrapper.text()).not.toContain('updater.updateAndRestart');
    wrapper.unmount();
  });

  it('offers restart only for a confirmed available update', () => {
    const store = useUpdaterStore();
    store.configuration = configured;
    store.checkResult = { ...configured, available: true, currentVersion: '1', availableVersion: '2' };
    vi.mocked(invoke).mockResolvedValue(configured);
    const alert = mount(TheUpdater);
    const settings = mount(SettingsUpdate, { props: { modelValue: structuredClone(defaultSettings) } });
    expect(alert.text()).toContain('updater.updateAndRestart');
    expect(alert.text()).toContain('updater.install.subtitle');
    expect(settings.text()).toContain('updater.updateAndRestart');
    alert.unmount();
    settings.unmount();
  });

  it('keeps install rejection visible and retries with a fresh check', async () => {
    const store = useUpdaterStore();
    store.checkResult = { ...configured, available: true, currentVersion: '1', availableVersion: '2' };
    vi.mocked(invoke).mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('active tasks'));
    await store.updateAndRestart();
    expect(store.isUpdating).toBe(false);
    expect(store.isNeedingRestart).toBe(false);
    expect(store.phase).toBe('error');
    expect(store.lastError).toContain('active tasks');
    expect(store.isIgnored).toBe(false);
  });

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
