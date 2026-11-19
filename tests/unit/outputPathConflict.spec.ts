import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { nextTick, ref } from 'vue';
import MediaErrorStep from '../../src/components/media-card/steps/MediaErrorStep.vue';
import TheMediaLogs from '../../src/components/media-view/TheMediaLogs.vue';
import { useMediaDiagnosticsStore } from '../../src/stores/media/diagnostics';
import { i18n } from '../../src/i18n';
import type { Group } from '../../src/tauri/types/group';

vi.mock('../../src/composables/useGroupLog', () => ({
  useGroupLog: () => ({ logText: { value: 'Untouched full download log' } }),
}));

const fatal: MediaFatal = {
  id: 'failed', groupId: 'group', exitCode: 1, internal: false,
  code: null, message: 'Download failed for group group', details: null, timestamp: 10,
};
const conflict: MediaDiagnostic = {
  id: 'failed', groupId: 'group', code: 'outputPathConflict', level: 'error',
  component: null, message: '[WinError 183] File exists',
  raw: 'ERROR: [WinError 183] File exists: destination.mp4', timestamp: 8,
};

function mountError() {
  return mount(MediaErrorStep, {
    props: { group: { id: 'group', url: 'https://example.com' } as Group },
    global: { plugins: [i18n], stubs: { RouterLink: true } },
  });
}

function mountLogs() {
  return mount(TheMediaLogs, { props: { groupId: 'group' }, global: { plugins: [i18n] } });
}

describe('output path conflict', () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    i18n.global.locale.value = 'en';
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue(fatal);
  });

  it.each(['en', 'zh-CN', 'zh-TW'] satisfies Array<'en' | 'zh-CN' | 'zh-TW'>)('shows one localized line of advice for the failed item in %s', (locale) => {
    i18n.global.locale.value = locale;
    vi.spyOn(useMediaDiagnosticsStore(), 'findDiagnosticsByGroupId').mockReturnValue([
      conflict, { ...conflict, id: 'other', code: 'signInRequired', timestamp: 9 },
    ]);
    const wrapper = mountError();
    const shortMessage = i18n.global.t('errors.runner.outputPathConflict.shortMessage');
    const advice = i18n.global.t('errors.runner.outputPathConflict.message');
    expect(shortMessage).not.toContain('errors.runner.');
    expect(wrapper.get('label').text()).toContain(shortMessage);
    expect(wrapper.findAll('p').map(node => node.text())).toEqual([advice]);
    expect(wrapper.text()).not.toContain('destination.mp4');
  });

  it.each([
    { ...conflict, id: 'other' },
    { ...conflict, timestamp: 11 },
    { ...conflict, level: 'warning' as const },
  ])('does not show an unrelated conflict on the failed card: %j', (unrelated) => {
    const store = useMediaDiagnosticsStore();
    vi.spyOn(store, 'findLastDiagnosticByGroupId').mockReturnValue(unrelated);
    vi.spyOn(store, 'findDiagnosticsByGroupId').mockReturnValue([unrelated]);
    expect(mountError().get('p').text()).toBe(fatal.message);
  });

  it('does not use a past conflict when no failed item is recorded', () => {
    const store = useMediaDiagnosticsStore();
    vi.spyOn(store, 'findLastFatalByGroupId').mockReturnValue(undefined);
    vi.spyOn(store, 'findLastDiagnosticByGroupId').mockReturnValue(conflict);
    expect(mountError().get('p').text()).toBe(i18n.global.t('errors.runner.unknown.message'));
  });

  it('localizes native publication collisions and preserves their raw details in logs', () => {
    const nativeFatal: MediaFatal = {
      ...fatal, internal: true, exitCode: null, code: 'outputPathConflict',
      message: 'Output publication failed: destination already exists', details: 'Existing destination: destination.mp4',
    };
    const store = useMediaDiagnosticsStore();
    vi.spyOn(store, 'findLastFatalByGroupId').mockReturnValue(nativeFatal);
    vi.spyOn(store, 'findFatalsByGroupId').mockReturnValue([nativeFatal]);
    const advice = i18n.global.t('errors.runner.outputPathConflict.message');
    expect(mountError().get('p').text()).toBe(advice);
    const logs = mountLogs();
    expect(logs.get('summary').text()).toContain(advice);
    expect(logs.get('details p').text()).toBe(nativeFatal.details);
  });

  it('does not replace a non-collision publication failure with an earlier yt-dlp error', () => {
    const publicationFatal: MediaFatal = {
      ...fatal, internal: true, exitCode: null,
      message: 'Output publication failed: access denied', details: 'Destination is not writable',
    };
    const store = useMediaDiagnosticsStore();
    vi.spyOn(store, 'findLastFatalByGroupId').mockReturnValue(publicationFatal);
    vi.spyOn(store, 'findFatalsByGroupId').mockReturnValue([publicationFatal]);
    vi.spyOn(store, 'findDiagnosticsByGroupId').mockReturnValue([conflict]);
    expect(mountError().get('p').text()).toBe(publicationFatal.message);
    expect(mountLogs().text()).toContain(publicationFatal.details);
  });

  it('merges a generic failure with a matching diagnostic without dropping raw details', async () => {
    const store = useMediaDiagnosticsStore();
    const fatals = ref<MediaFatal[]>([]);
    vi.spyOn(store, 'findFatalsByGroupId').mockImplementation(() => fatals.value);
    vi.spyOn(store, 'findDiagnosticsByGroupId').mockReturnValue([conflict]);
    const wrapper = mountLogs();
    fatals.value = [{ ...fatal, details: 'Exit detail' }];
    await nextTick();
    expect(wrapper.findAll('details')).toHaveLength(1);
    expect(wrapper.get('details').text()).toContain(conflict.raw);
    expect(wrapper.get('details').text()).toContain('Exit detail');
    expect(wrapper.get('[role="log"]').text()).toBe('Untouched full download log');
    expect(wrapper.text()).not.toContain(fatal.message);
  });

  it.each([
    [{ ...fatal }, { ...conflict, id: 'other' }],
    [{ ...fatal }, { ...conflict, timestamp: 11 }],
    [{ ...fatal }, { ...conflict, code: 'unknown' }],
    [{ ...fatal, exitCode: 101 }, conflict],
    [{ ...fatal, message: 'Failed to spawn yt-dlp: missing executable' }, conflict],
  ] as const)('keeps an unmatched or non-generic failure: %j', (unmatched, diagnostic) => {
    const store = useMediaDiagnosticsStore();
    vi.spyOn(store, 'findFatalsByGroupId').mockReturnValue([unmatched]);
    vi.spyOn(store, 'findDiagnosticsByGroupId').mockReturnValue([diagnostic]);
    expect(mountLogs().text()).toContain(unmatched.message);
  });
});
