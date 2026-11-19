import { enableAutoUnmount, mount } from '@vue/test-utils';
import { nextTick } from 'vue';
import { afterEach, describe, expect, it, vi } from 'vitest';
import SubtitleView from '../../src/views/app/SubtitleView.vue';
import TheSubtitlePreferences from '../../src/components/media-view/TheSubtitlePreferences.vue';
import MediaConfigureStep from '../../src/components/media-card/steps/MediaConfigureStep.vue';
import type { Group } from '../../src/tauri/types/group';
import { i18n } from '../../src/i18n';
import { useMediaGroupStore } from '../../src/stores/media/group';
import { useMediaOptionsStore } from '../../src/stores/media/options';
import { useSettingsStore } from '../../src/stores/settings';
import { defaultSettings } from '../../src/tauri/types/config';
import { TrackType, type MediaItem } from '../../src/tauri/types/media';

enableAutoUnmount(afterEach);

describe('global subtitle preferences', () => {
  it('can re-enable saved subtitle preferences without resetting the selected languages', async () => {
    const settingsStore = useSettingsStore();
    settingsStore.settings = {
      ...settingsStore.settings,
      subtitles: { ...defaultSettings.subtitles, enabled: false, chinesePriority: false, languages: ['en'] },
    };
    const patch = vi.spyOn(settingsStore, 'patch').mockResolvedValue(undefined);
    const wrapper = mount(SubtitleView, { global: { plugins: [i18n], stubs: { RouterLink: true } } });
    const enable = wrapper.get<HTMLInputElement>('#enable-subtitles');
    expect(enable.element.checked).toBe(false);
    await enable.setValue(true);
    expect(wrapper.get<HTMLButtonElement>('button[type="submit"]').element.disabled).toBe(false);
    await wrapper.get('form').trigger('submit');
    expect(patch).toHaveBeenCalledWith({ subtitles: expect.objectContaining({ enabled: true, languages: ['en'] }) });
  });

  it('re-enables subtitle defaults when restoring a policy after saving no selection', async () => {
    const settingsStore = useSettingsStore();
    settingsStore.settings = {
      ...settingsStore.settings,
      subtitles: { ...defaultSettings.subtitles, enabled: false, chinesePriority: false, languages: [] },
    };
    const patch = vi.spyOn(settingsStore, 'patch').mockResolvedValue(undefined);
    const wrapper = mount(SubtitleView, { global: { plugins: [i18n], stubs: { RouterLink: true } } });
    await wrapper.get('#chinese-priority').setValue(true);
    await wrapper.get('form').trigger('submit');
    expect(patch).toHaveBeenCalledWith({ subtitles: expect.objectContaining({ enabled: true, chinesePriority: true }) });
  });
});

function createGroupItem(id: string): MediaItem {
  return {
    id,
    url: `https://example.com/${id}`,
    audioCodecs: [],
    formats: [],
    filesize: 0,
    isLeader: true,
    subtitleInventory: {
      manualLanguages: ['en', 'de'],
      autoLanguages: ['en'],
    },
  };
}

describe('ready card subtitle checkbox', () => {
  function createReadyGroup(): Group {
    const item = createGroupItem('ready');
    return {
      id: 'ready', url: item.url, total: 1, processed: 1, errored: 0,
      isCombined: false, audioCodecs: [], formats: [], filesize: 0, items: { ready: item },
    };
  }

  it.each([undefined, { manualLanguages: [], autoLanguages: [] }])('hides with no available tracks (%j)', (inventory) => {
    const group = createReadyGroup();
    group.items.ready.subtitleInventory = inventory;
    const wrapper = mount(MediaConfigureStep, { props: { group }, global: { plugins: [i18n] } });
    expect(wrapper.find('input[type="checkbox"]').exists()).toBe(false);
  });

  it('shows subtitles-only mode as checked even when the ordinary task opts out', async () => {
    const group = createReadyGroup();
    useMediaGroupStore().createGroup(group);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOptions(group.id, { trackType: TrackType.both, subtitlesOnly: true });
    optionsStore.setOverrides(group.id, { subtitles: { enabled: false } });
    const wrapper = mount(MediaConfigureStep, { props: { group }, global: { plugins: [i18n] } });
    await nextTick();
    const { checked, disabled } = wrapper.get<HTMLInputElement>('input[type="checkbox"]').element;
    expect({ checked, disabled }).toEqual({ checked: true, disabled: true });
    expect(optionsStore.getOverrides(group.id)?.subtitles?.enabled).toBe(false);
  });

  it('uses group preferences, including when only automatic subtitles are available', async () => {
    const group = createReadyGroup();
    group.items.ready.subtitleInventory = { manualLanguages: [], autoLanguages: ['en'] };
    useMediaGroupStore().createGroup(group);
    useSettingsStore().settings.subtitles = structuredClone(defaultSettings.subtitles);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOverrides(group.id, { subtitles: { formatPreference: ['vtt'] }, output: { addMetadata: true } });
    const wrapper = mount(MediaConfigureStep, { props: { group }, global: { plugins: [i18n] } });
    const checkbox = wrapper.get<HTMLInputElement>('input[type="checkbox"]');
    expect(checkbox.element.checked).toBe(true);
    await checkbox.setValue(false);
    expect(optionsStore.getOverrides(group.id)).toEqual({
      subtitles: { enabled: false, formatPreference: ['vtt'] }, output: { addMetadata: true },
    });

    const preferences = mount(TheSubtitlePreferences, { props: { groupId: group.id }, global: { plugins: [i18n] } });
    await preferences.get('#auto-language-en').setValue(true);
    expect(optionsStore.getOverrides(group.id)?.subtitles).toMatchObject({ enabled: false, autoLanguages: ['en'] });
    expect(checkbox.element.checked).toBe(false);
    await checkbox.setValue(true);
    expect(checkbox.element.checked).toBe(true);
    expect(optionsStore.getOverrides(group.id)?.subtitles).toMatchObject({ enabled: true, autoLanguages: ['en'] });
  });
});

describe('subtitle override preferences', () => {
  it('stores per-group subtitle overrides and clears them when reset', async () => {
    const groupId = 'group-subtitles';
    const settingsStore = useSettingsStore();
    settingsStore.settings.subtitles = { ...structuredClone(defaultSettings.subtitles), chinesePriority: false, languages: ['en'] };

    const groupStore = useMediaGroupStore();
    groupStore.createGroup({
      id: groupId,
      url: 'https://example.com',
      total: 1,
      processed: 1,
      errored: 0,
      isCombined: false,
      audioCodecs: [],
      formats: [],
      filesize: 0,
      items: {
        leader: createGroupItem('leader'),
      },
    });

    const optionsStore = useMediaOptionsStore();
    const wrapper = mount(TheSubtitlePreferences, {
      props: {
        groupId,
      },
      global: {
        plugins: [i18n],
        stubs: {
          RouterLink: true,
        },
      },
    });

    await nextTick();
    await wrapper.get('#manual-language-de').setValue(true);
    await nextTick();

    expect(optionsStore.getOverrides(groupId)?.subtitles).toEqual({
      languages: ['en', 'de'],
      manualLanguages: ['en', 'de'],
    });

    await wrapper.get('#auto-language-en').setValue(true);
    await nextTick();

    expect(optionsStore.getOverrides(groupId)?.subtitles).toEqual({
      includeAutoGenerated: true,
      autoLanguages: ['en'],
      languages: ['en', 'de'],
      manualLanguages: ['en', 'de'],
    });

    await wrapper.get('#manual-language-en').setValue(false);
    await nextTick();
    await wrapper.get('#manual-language-de').setValue(false);
    await nextTick();
    await wrapper.get('#auto-language-en').setValue(false);
    await nextTick();

    expect(optionsStore.getOverrides(groupId)?.subtitles).toEqual({
      languages: [],
      manualLanguages: [],
    });
    await wrapper.get('#manual-language-en').setValue(true);
    expect(optionsStore.getOverrides(groupId)?.subtitles).toBeUndefined();
  });

  it('lets explicit tracks replace Chinese priority and restores the policy after download-all', async () => {
    const groupId = 'subtitle-policy';
    useSettingsStore().settings.subtitles = {
      ...structuredClone(defaultSettings.subtitles), chinesePriority: true, languages: [], includeAutoGenerated: false,
    };
    useMediaGroupStore().createGroup({
      id: groupId, url: 'https://example.com', total: 1, processed: 1, errored: 0,
      isCombined: false, audioCodecs: [], formats: [], filesize: 0,
      items: { leader: createGroupItem('policy') },
    });
    const wrapper = mount(TheSubtitlePreferences, { props: { groupId }, global: { plugins: [i18n] } });
    const optionsStore = useMediaOptionsStore();
    await nextTick();

    await wrapper.get('#manual-language-de').setValue(true);
    expect(optionsStore.getOverrides(groupId)?.subtitles).toMatchObject({
      chinesePriority: false, languages: ['de'], manualLanguages: ['de'],
    });
    await wrapper.get('#override-all-subtitles').setValue(true);
    await wrapper.get('#override-chinese-priority').setValue(true);
    expect(optionsStore.getOverrides(groupId)?.subtitles?.languages).toBeUndefined();
    expect(optionsStore.getOverrides(groupId)?.subtitles?.manualLanguages).toBeUndefined();
    expect((wrapper.get('#override-chinese-priority').element as HTMLInputElement).checked).toBe(true);
    expect((wrapper.get('#override-all-subtitles').element as HTMLInputElement).checked).toBe(false);
  });

  it('toggles subtitle-only without a format and disables embedding', async () => {
    const groupId = 'subtitle-mode';
    useMediaGroupStore().createGroup({ id: groupId, url: 'https://example.com', total: 1, processed: 1, errored: 0,
      isCombined: false, audioCodecs: [], formats: [], filesize: 0, items: { leader: createGroupItem('mode') } });
    const wrapper = mount(TheSubtitlePreferences, { props: { groupId }, global: { plugins: [i18n] } });
    await wrapper.get('#subtitles-only').setValue(true);
    expect(useMediaOptionsStore().getOptions(groupId)?.subtitlesOnly).toBe(true);
    expect((wrapper.get('#override-embed-subtitles').element as HTMLInputElement).disabled).toBe(true);
    expect(wrapper.text()).toContain(i18n.global.t('subtitles.task.platformHint'));
    await wrapper.get('#subtitles-only').setValue(false);
    expect((wrapper.get('#override-embed-subtitles').element as HTMLInputElement).disabled).toBe(false);
  });

  it('does not mark download-all as enabled when all visible boxes happen to be selected manually', async () => {
    const groupId = 'group-subtitles-manual-selection';
    const settingsStore = useSettingsStore();
    settingsStore.settings.subtitles = { ...structuredClone(defaultSettings.subtitles), chinesePriority: false, languages: ['en'] };

    const groupStore = useMediaGroupStore();
    groupStore.createGroup({
      id: groupId,
      url: 'https://example.com',
      total: 1,
      processed: 1,
      errored: 0,
      isCombined: false,
      audioCodecs: [],
      formats: [],
      filesize: 0,
      items: {
        leader: {
          ...createGroupItem('leader-manual-selection'),
          subtitleInventory: {
            manualLanguages: ['en'],
            autoLanguages: ['en'],
          },
        },
      },
    });

    const wrapper = mount(TheSubtitlePreferences, {
      props: {
        groupId,
      },
      global: {
        plugins: [i18n],
        stubs: {
          RouterLink: true,
        },
      },
    });

    await nextTick();
    expect((wrapper.get('#override-all-subtitles').element as HTMLInputElement).checked).toBe(false);

    await wrapper.get('#auto-language-en').setValue(true);
    await nextTick();

    expect((wrapper.get('#override-all-subtitles').element as HTMLInputElement).checked).toBe(false);
  });
});
