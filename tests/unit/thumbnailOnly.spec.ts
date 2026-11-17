import { enableAutoUnmount, mount } from '@vue/test-utils';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ref } from 'vue';
import MediaCardActions from '../../src/components/media-card/actions/MediaCardActions.vue';
import OutputSettingsEditor from '../../src/components/output/OutputSettingsEditor.vue';
import { defaultSettings } from '../../src/tauri/types/config';
import type { Group } from '../../src/tauri/types/group';
import { i18n } from '../../src/i18n';

const { downloadThumbnail, state } = vi.hoisted(() => ({ downloadThumbnail: vi.fn(), state: { value: 'configure' } }));
vi.mock('../../src/stores/media/media', () => ({ useMediaStore: () => ({ downloadThumbnail }) }));
vi.mock('../../src/stores/media/state', () => ({
  MediaState: { configure: 'configure', downloading: 'downloading', downloadingList: 'downloadingList', paused: 'paused', pausedList: 'pausedList', done: 'done', error: 'error', fetching: 'fetching', playlistSelection: 'playlistSelection' },
  useMediaStateStore: () => ({ getGroupState: () => state.value }),
}));
vi.mock('../../src/composables/useOpener', () => ({ useOpener: () => ({ openUrl: vi.fn() }) }));
enableAutoUnmount(afterEach);

describe('thumbnail-only download controls', () => {
  it('offers a clearly labelled separate thumbnail action calling the group method', async () => {
    state.value = 'configure';
    const wrapper = mount(MediaCardActions, {
      props: { group: { id: 'cover-group', url: 'https://example.com/video' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    const action = wrapper.findAll('button').find(button => button.attributes('title') === i18n.global.t('media.card.actions.downloadThumbnail'));
    expect(action).toBeDefined();
    await action!.trigger('click');
    expect(downloadThumbnail).toHaveBeenCalledWith('cover-group');
  });

  it('disables independent thumbnail downloads while a group is downloading', () => {
    state.value = 'downloading';
    const wrapper = mount(MediaCardActions, {
      props: { group: { id: 'cover-group', url: 'https://example.com/video' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    const action = wrapper.findAll('button').find(button => button.attributes('title') === i18n.global.t('media.card.actions.downloadThumbnail'));
    expect(action).toBeDefined();
    expect(action!.attributes('disabled')).toBeDefined();
  });

  it('associates the save-thumbnail label with its own checkbox', () => {
    const output = ref(structuredClone(defaultSettings.output));
    const wrapper = mount(OutputSettingsEditor, {
      props: { modelValue: output.value, idPrefix: 'cover' },
      global: { plugins: [i18n] },
    });
    const label = wrapper.findAll('label').find(item => item.text() === i18n.global.t('settings.output.saveThumbnail.label'));
    expect(label?.attributes('for')).toBe('cover-save-thumbnail');
    expect(wrapper.get('#cover-save-thumbnail').element.tagName).toBe('INPUT');
  });
});
