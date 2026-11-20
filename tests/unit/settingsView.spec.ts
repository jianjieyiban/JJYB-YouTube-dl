import { flushPromises, mount } from '@vue/test-utils';
import { createMemoryHistory, createRouter } from 'vue-router';
import { defineComponent, nextTick, reactive } from 'vue';
import { describe, expect, it, vi } from 'vitest';
import SettingsView from '../../src/views/app/SettingsView.vue';
import SettingsDownloadsTab from '../../src/views/app/settings/SettingsDownloadsTab.vue';
import SettingsAppTab from '../../src/views/app/settings/SettingsAppTab.vue';
import SettingsNetworkTab from '../../src/views/app/settings/SettingsNetworkTab.vue';
import SettingsSystemTab from '../../src/views/app/settings/SettingsSystemTab.vue';
import SettingsAboutTab from '../../src/views/app/settings/SettingsAboutTab.vue';
import SettingsUpdate from '../../src/components/settings/SettingsUpdate.vue';
import { i18n } from '../../src/i18n';
import { defaultSettings, type Settings } from '../../src/tauri/types/config';
import { TranscodePolicy } from '../../src/tauri/types/media';

const patch = vi.fn();
const reset = vi.fn();
const showToast = vi.fn();
const setTheme = vi.fn();
const openInternalPath = vi.fn();
const checkBinaryUpdates = vi.fn();

vi.mock('../../src/stores/binaries', () => ({
  useBinariesStore: () => ({ checkUpdates: checkBinaryUpdates }),
}));

vi.mock('../../src/stores/updater', () => ({
  useUpdaterStore: () => ({ loadConfiguration: vi.fn() }),
}));

const settingsState = reactive<Settings>(structuredClone(defaultSettings));

vi.mock('../../src/stores/settings', () => ({
  useSettingsStore: () => ({
    settings: settingsState,
    patch,
    reset,
  }),
}));

vi.mock('../../src/stores/toast', () => ({
  useToastStore: () => ({
    showToast,
  }),
}));

vi.mock('../../src/composables/useTheme', () => ({
  useTheme: () => ({
    setTheme,
  }),
}));

vi.mock('../../src/composables/useOpener', () => ({
  useOpener: () => ({
    openInternalPath,
  }),
}));

function createRoot() {
  return defineComponent({
    template: '<router-view />',
  });
}

function createBlankView() {
  return defineComponent({
    template: '<div />',
  });
}

function createSettingsRouter() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      {
        path: '/',
        name: 'home',
        component: createBlankView(),
      },
      {
        path: '/location',
        name: 'location',
        component: createBlankView(),
      },
      {
        path: '/subtitles',
        name: 'subtitles',
        component: createBlankView(),
      },
      {
        path: '/authentication',
        name: 'authentication',
        component: createBlankView(),
      },
      {
        path: '/',
        component: createRoot(),
        children: [
          {
            path: 'settings',
            component: SettingsView,
            children: [
              {
                path: '',
                name: 'settings.downloads',
                component: SettingsDownloadsTab,
              },
              {
                path: 'app',
                name: 'settings.app',
                component: SettingsAppTab,
              },
              {
                path: 'network',
                name: 'settings.network',
                component: SettingsNetworkTab,
              },
              {
                path: 'system',
                name: 'settings.system',
                component: SettingsSystemTab,
              },
              {
                path: 'about',
                name: 'settings.about',
                component: SettingsAboutTab,
              },
            ],
          },
        ],
      },
    ],
  });
  return router;
}

describe('SettingsUpdate tool translations', () => {
  it.each([
    { scenario: 'button', result: [], expected: 'Check for tool updates' },
    { scenario: 'up-to-date status', result: [], expected: 'Tools are up to date' },
    { scenario: 'failure status', error: new Error('offline'), expected: 'Failed to check for tool updates: Error: offline' },
  ])('renders the $scenario in English', async ({ scenario, result, error, expected }) => {
    const previousLocale = i18n.global.locale.value;
    i18n.global.locale.value = 'en';
    checkBinaryUpdates.mockReset();
    if (error) {
      checkBinaryUpdates.mockRejectedValueOnce(error);
    } else {
      checkBinaryUpdates.mockResolvedValueOnce(result);
    }
    const router = createSettingsRouter();
    await router.push('/');
    await router.isReady();
    const wrapper = mount(SettingsUpdate, {
      props: { modelValue: structuredClone(defaultSettings) },
      global: { plugins: [router, i18n] },
    });
    try {
      const button = wrapper.get('button');
      if (scenario === 'button') {
        expect(button.text()).toBe(expected);
      } else {
        await button.trigger('click');
        await flushPromises();
        expect(checkBinaryUpdates).toHaveBeenCalledTimes(1);
        expect(wrapper.get('[role="status"]').text()).toBe(expected);
      }
    } finally {
      wrapper.unmount();
      i18n.global.locale.value = previousLocale;
    }
  });
});

describe('SettingsView', () => {
  it('keeps native language names visible in Chinese and saves the selected language', async () => {
    const previousLocale = i18n.global.locale.value;
    i18n.global.locale.value = 'zh-CN';
    patch.mockReset();
    Object.assign(settingsState, structuredClone(defaultSettings));
    const router = createSettingsRouter();
    await router.push('/settings/app');
    await router.isReady();
    const wrapper = mount(createRoot(), { global: { plugins: [router, i18n] } });
    try {
      const languages = wrapper.get('#language');
      expect(languages.find('option[value="en"]').text()).toContain('English');
      expect(languages.find('option[value="zh-CN"]').text()).toContain('简体中文');
      expect(wrapper.get('#language-hint').text()).toContain('保存');
      expect(languages.findAll('option')).toHaveLength(14);
      await languages.setValue('en');
      await wrapper.get('form').trigger('submit');
      expect(patch.mock.calls[0][0].appearance.language).toBe('en');
    } finally {
      wrapper.unmount();
      i18n.global.locale.value = previousLocale;
    }
  });

  it('renders the router-link tabs used by the media views', async () => {
    const router = createSettingsRouter();
    await router.push('/settings');
    await router.isReady();

    const wrapper = mount(createRoot(), {
      global: {
        plugins: [router, i18n],
      },
    });

    await nextTick();

    const tabs = wrapper.findAll('a[role="tab"]');
    expect(tabs.map(tab => tab.text())).toEqual([
      'Downloads',
      'App',
      'Network',
      'System',
      'About',
    ]);
    expect(tabs[0].classes()).toContain('tab-active');

    await router.push('/settings/app');
    await nextTick();

    const updatedTabs = wrapper.findAll('a[role="tab"]');
    expect(updatedTabs[0].classes()).not.toContain('tab-active');
    expect(updatedTabs[1].classes()).toContain('tab-active');
  });

  it('keeps save and reset operating on the same draft settings object', async () => {
    patch.mockReset();
    reset.mockReset();
    showToast.mockReset();
    setTheme.mockReset();
    openInternalPath.mockReset();
    Object.assign(settingsState, structuredClone(defaultSettings));

    patch.mockImplementation(async (draft: Settings) => {
      Object.assign(settingsState, JSON.parse(JSON.stringify(draft)) as Settings);
    });
    reset.mockImplementation(async () => {
      const next = structuredClone(defaultSettings);
      Object.assign(settingsState, next);
      return next;
    });

    const router = createSettingsRouter();
    await router.push('/settings');
    await router.isReady();

    const wrapper = mount(createRoot(), {
      global: {
        plugins: [router, i18n],
      },
    });

    await nextTick();
    await wrapper.get('#keep-original-streams-video').setValue(true);
    await wrapper.get('form').trigger('submit');
    await nextTick();

    expect(patch).toHaveBeenCalledTimes(1);
    expect(patch.mock.calls[0][0].output.video.policy).toBe(TranscodePolicy.never);
    expect(showToast).toHaveBeenCalledWith('Settings saved!', { style: 'success' });

    const resetButton = wrapper
      .findAll('button')
      .find(button => button.text().includes('Reset'));

    expect(resetButton).toBeTruthy();
    await resetButton!.trigger('click');
    await nextTick();

    expect(reset).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledWith('Reset settings to defaults.', { style: 'success' });
    expect(wrapper.get('#keep-original-streams-video').element).toBeInstanceOf(HTMLInputElement);
  });
});
