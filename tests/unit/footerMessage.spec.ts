import { describe, it, expect } from 'vitest';
import { useMediaGroupStore } from '../../src/stores/media/group';
import { useMediaStateStore, MediaState } from '../../src/stores/media/state';
import TheFooter from '../../src/components/TheFooter.vue';
import { mount, shallowMount } from '@vue/test-utils';
import BaseProgress from '../../src/components/base/BaseProgress.vue';
import en from '../../src/locales/en.json';
import { createI18n } from 'vue-i18n';

describe('TheFooter progress message', () => {
  it.each([
    { states: [MediaState.error, MediaState.error], message: 'Download failed – 2 items failed.', variant: 'error' },
    { states: [MediaState.done, MediaState.error, MediaState.error], message: 'Downloads finished – 1 succeeded, 2 failed.', variant: 'warning' },
    { states: [MediaState.done, MediaState.done], message: 'Download complete – 2 items downloaded.', variant: 'primary' },
  ])('shows accurate terminal results for $states', ({ states, message, variant }) => {
    const groupStore = useMediaGroupStore();
    const stateStore = useMediaStateStore();
    groupStore.createGroup({
      id: 'results', url: 'results', total: states.length + 1, processed: states.length + 1,
      errored: 0, isCombined: true, audioCodecs: [], formats: [], filesize: 0,
      items: {
        leader: { id: 'leader', url: 'leader', audioCodecs: [], formats: [], filesize: 0, isLeader: true },
        ...Object.fromEntries(states.map((state, index) => {
          const id = `item-${index}`;
          stateStore.setState(id, state);
          return [id, { id, url: id, audioCodecs: [], formats: [], filesize: 0 }];
        })),
      },
    });
    // A combined playlist leader must never be counted as a successful download.
    stateStore.setState('leader', MediaState.done);
    const wrapper = shallowMount(TheFooter, {
      global: {
        plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })],
        stubs: { BaseProgress: false },
      },
    });
    const progress = wrapper.findComponent(BaseProgress);
    expect(progress.text()).toBe(message);
    expect(progress.props('variant')).toBe(variant);
    expect(progress.props('value')).toBe(states.length);
    expect(progress.props('max')).toBe(states.length);
    if (states.includes(MediaState.error)) {
      expect(progress.text()).not.toContain('Download complete');
      expect(progress.find('progress').classes()).not.toContain('progress-primary');
    }
  });

  it('shows ready items count excluding completed items', () => {
    const groupStore = useMediaGroupStore();
    const stateStore = useMediaStateStore();

    groupStore.createGroup({
      id: 'g1',
      url: 'g1',
      total: 2,
      processed: 2,
      errored: 0,
      isCombined: true,
      audioCodecs: [],
      formats: [],
      filesize: 0,
      items: {
        a: { id: 'a', url: 'a', title: 'A', audioCodecs: [], formats: [], filesize: 0, isLeader: true },
        b: { id: 'b', url: 'b', title: 'B', audioCodecs: [], formats: [], filesize: 0 },
      },
    });

    stateStore.setState('a', MediaState.done);
    stateStore.setState('b', MediaState.configure);

    const wrapper = mount(TheFooter, {
      global: {
        plugins: [
          createI18n({
            legacy: false,
            locale: 'en',
            fallbackLocale: 'en',
            messages: {
              en: {
                layout: {
                  footer: {
                    format: {
                      trackSelect: {
                        screenReader: 'Selecteer track voor alle video\'s',
                      },
                      formatSelect: {
                        noFormats: 'Geen opties',
                        placeholder: 'Selecteer formaat',
                        screenReader: 'Selecteer formaat voor alle video\'s',
                      },
                    },
                    progress: {
                      downloading: 'Downloading queue - {done} of {total} completed.',
                      completed: 'Download complete – {n} item downloaded. | Download complete – {n} items downloaded.',
                      ready: 'Ready to download! {n} item queued. | Ready to download! {n} items queued.',
                    },
                  },
                },
              },
            },
          }),
        ],
        stubs: {
          BaseProgress: { template: '<div><slot /></div>' },
          BaseButton: { template: '<button><slot /></button>' },
          BaseToggleButton: { template: '<button><slot /><slot name="on"/><slot name="off"/></button>' },
          FolderIcon: true,
          TrashIcon: true,
          LockOpenIcon: true,
          LockClosedIcon: true,
          ChatBubbleBottomCenterIcon: true,
          ChatBubbleBottomCenterTextIcon: true,
        },
      },
    });

    expect(wrapper.text()).toContain('Ready to download! 1 item queued.');
  });
});
