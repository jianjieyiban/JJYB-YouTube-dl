import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MediaErrorStep from '../../src/components/media-card/steps/MediaErrorStep.vue';
import TheMediaLogs from '../../src/components/media-view/TheMediaLogs.vue';
import { useMediaDiagnosticsStore } from '../../src/stores/media/diagnostics';
import { i18n } from '../../src/i18n';
import type { Group } from '../../src/tauri/types/group';

vi.mock('../../src/composables/useGroupLog', () => ({
  useGroupLog: () => ({ logText: { value: '' } }),
}));

const fatal: MediaFatal = {
  id: 'media', groupId: 'group', exitCode: null, internal: true,
  code: 'noSubtitles', message: 'No subtitle tracks were downloaded', details: null, timestamp: 1,
};

describe('subtitle business diagnostic', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    i18n.global.locale.value = 'zh-CN';
  });

  for (const view of ['card', 'logs']) {
    it(`${view} preserves noSubtitles and hides reporting`, () => {
      vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue(fatal);
      vi.spyOn(useMediaDiagnosticsStore(), 'findFatalsByGroupId').mockReturnValue([fatal]);
      const wrapper = view === 'card'
        ? mount(MediaErrorStep, {
            props: { group: { id: 'group', url: 'https://example.com' } as Group },
            global: { plugins: [i18n], stubs: { RouterLink: true } },
          })
        : mount(TheMediaLogs, {
            props: { groupId: 'group' },
            global: { plugins: [i18n] },
          });
      expect(wrapper.text()).toContain('找不到可下载的字幕，无法下载');
      expect(wrapper.findAll('button').some(button => button.text().includes('上报'))).toBe(false);
    });
  }
});
