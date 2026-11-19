import { mount } from '@vue/test-utils';
import * as Sentry from '@sentry/browser';
import DiagnosticCard from '../../src/components/media-view/DiagnosticCard.vue';
import { defineComponent, ref } from 'vue';
import { useDiagnostic } from '../../src/composables/useDiagnostic';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import MediaErrorStep from '../../src/components/media-card/steps/MediaErrorStep.vue';
import TheMediaLogs from '../../src/components/media-view/TheMediaLogs.vue';
import { useMediaDiagnosticsStore } from '../../src/stores/media/diagnostics';
import { i18n } from '../../src/i18n';
import type { Group } from '../../src/tauri/types/group';
import diagnosticRules from '../../src-tauri/src/diagnostic_rules.json';
import en from '../../src/locales/en.json';
import zhCN from '../../src/locales/zh-CN.json';
import zhTW from '../../src/locales/zh-TW.json';

vi.mock('@sentry/browser', { spy: true });

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

  it('card preserves the specific unclassified error', () => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue({ ...fatal, internal: false });
    vi.spyOn(useMediaDiagnosticsStore(), 'findDiagnosticsByGroupId').mockReturnValue([{
      id: 'media', groupId: 'group', code: 'unknown', message: 'Custom extractor failed. Please retry.',
      level: 'error', raw: '', component: null, timestamp: 1,
    }]);
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain('Custom extractor failed. Please retry.');
  });

  it('card shows a diagnostic without a fatal', () => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue(undefined);
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastDiagnosticByGroupId').mockReturnValue({
      id: 'media', groupId: 'group', code: 'unknown', message: 'Standalone extractor failure',
      level: 'error', raw: '', component: null, timestamp: 1,
    });
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain('Standalone extractor failure');
  });

  it.each([
    ['External extractor failure', null, 'External extractor failure'],
    ['', 'ERROR: External transport failed', 'ERROR: External transport failed'],
    ['', null, null],
  ])('card shows external fatal message %j or details %j without a diagnostic', (message, details, expected) => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue({
      ...fatal, internal: false, exitCode: 1, code: null, message, details,
    });
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastDiagnosticByGroupId').mockReturnValue(undefined);
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain(expected ?? i18n.global.t('errors.runner.unknown.message'));
  });

  it('card uses localized guidance when no error records exist', () => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue(undefined);
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastDiagnosticByGroupId').mockReturnValue(undefined);
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain(i18n.global.t('errors.runner.unknown.message'));
    expect(wrapper.text()).toContain(i18n.global.t('errors.runner.unknown.shortMessage'));
  });

  it('card preserves exit code 101 filter rejection without a diagnostic', () => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue({
      ...fatal, internal: false, exitCode: 101,
    });
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastDiagnosticByGroupId').mockReturnValue(undefined);
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain(i18n.global.t('errors.runner.rejectedByFilter.message'));
    expect(wrapper.text()).toContain(i18n.global.t('errors.runner.rejectedByFilter.shortMessage'));
  });

  it('shows external diagnostic text literally on the compact card and in reports', async () => {
    const message = 'Unable to read <track> & retry at https://example.com/help?a=1&b=2';
    const diagnostic: MediaDiagnostic = { ...fatal, code: 'unknown', level: 'error', component: null, message, raw: '' };
    const capture = vi.spyOn(Sentry, 'captureMessage').mockReturnValue('event-id');
    try {
      const store = useMediaDiagnosticsStore();
      vi.spyOn(store, 'findLastDiagnosticByGroupId').mockReturnValue(diagnostic);
      const wrapper = mount(MediaErrorStep, {
        props: { group: { id: 'group', url: 'https://example.com' } as Group },
        global: { plugins: [i18n], stubs: { RouterLink: true } },
      });
      expect(wrapper.get('p').text()).toBe(message);
      expect(wrapper.find('p a, p track').exists()).toBe(false);
      await wrapper.get('button').trigger('click');
      expect(capture).toHaveBeenCalledWith(message, expect.any(Function));
    } finally {
      capture.mockRestore();
    }
  });

  it('links URLs in the expanded diagnostic without treating message or raw text as markup', () => {
    const url = 'https://example.com/help?a=1&b=2';
    const diagnostic: MediaDiagnostic = {
      ...fatal, code: 'unknown', level: 'error', component: null,
      message: `Failed: <track> & ${url}`, raw: `ERROR: <track> & ${url}`,
    };
    const wrapper = mount(DiagnosticCard, { props: { diagnostic }, global: { plugins: [i18n] } });
    for (const [selector, text] of [['summary', diagnostic.message], ['p', diagnostic.raw]]) {
      const section = wrapper.get(selector);
      expect(section.text()).toBe(text);
      expect(section.find('track').exists()).toBe(false);
      expect(section.get('a').attributes('href')).toBe(url);
    }
  });

  it.each([
    ['', 'ERROR: Custom transport failed', 'ERROR: Custom transport failed'],
    ['  \n ', '  ERROR: Custom transport failed\n', 'ERROR: Custom transport failed'],
    ['', '', null],
    [' \n ', '\t ', null],
    [null, null, null],
    ['<img src=x onerror=alert(1)>', '', '<img src=x onerror=alert(1)>'],
    ['', '<script>alert(1)</script>', '<script>alert(1)</script>'],
  ])('falls back safely for message %j and raw %j', (message, raw, expected) => {
    const diagnostic = { ...fatal, code: 'unknown', level: 'error', message, raw } as unknown as MediaDiagnostic;
    const Probe = defineComponent({
      setup: () => useDiagnostic(ref(diagnostic)),
      template: '<div v-html="diagnosticDisplay.messageHtml"></div>',
    });
    const wrapper = mount(Probe, { global: { plugins: [i18n] } });
    const display = wrapper.vm.diagnosticDisplay;
    expect(display.message).toBe(expected ?? i18n.global.t('errors.runner.unknown.message'));
    expect(display.shortMessage).toBe(expected ? (message?.trim() || raw?.trim()) : i18n.global.t('errors.runner.unknown.shortMessage'));
    expect(wrapper.find('script').exists()).toBe(false);
    expect(wrapper.find('img').exists()).toBe(false);
  });

  it.each([
    ['Could not copy Chrome cookie database. See issue 7271', 'cookieDatabaseCopyFailed'],
    ['Could not copy Chromium cookie database: [Errno 13] Permission denied', 'cookieDatabaseCopyFailed'],
    ['Failed to decrypt with DPAPI', 'cookieDecryptionFailed'],
    ['The read operation timed out', 'networkTimeout'],
    ['Connection timed out', 'networkTimeout'],
    ['ReadTimeoutError: Read timed out', 'networkTimeout'],
    ['Unable to download webpage: [Errno 10061] Connection refused', 'networkConnectionFailed'],
    ['Connection reset by peer', 'networkConnectionFailed'],
    ['[WinError 10060] A connection attempt failed', 'networkTimeout'],
    ['[WinError 10054] An existing connection was forcibly closed', 'networkConnectionFailed'],
    ['[Errno 28] No space left on device', 'diskSpaceInsufficient'],
    ['[WinError 112] There is not enough space on the disk', 'diskSpaceInsufficient'],
    ['HTTP Error 403: Forbidden (connection timed out)', 'accessForbidden403'],
    ['getaddrinfo failed', 'networkNameResolutionFailed'],
    ['Unable to connect to proxy: Connection refused', 'unableToConnectToProxy'],
    ['cookie database format unsupported', 'unknown'],
    ['Failed to decrypt unrelated payload', 'unknown'],
    ['Timeout setting is invalid', 'unknown'],
    ['Connection setting is invalid', 'unknown'],
    ['Disk metadata unavailable', 'unknown'],
  ])('classifies %s precisely as %s', (message, expected) => {
    // Mirror the consumer: first matching error rule, case-insensitive substrings.
    const rule = diagnosticRules.rules.find(rule => rule.appliesTo === 'error' && !rule.component && rule.patterns.some((pattern) => {
      if (pattern.kind === 'substr') return message.toLowerCase().includes(pattern.value.toLowerCase());
      return new RegExp(pattern.value.replace(/^\(\?i\)/, ''), pattern.value.startsWith('(?i)') ? 'i' : '').test(message);
    }));
    expect(rule?.code ?? diagnosticRules.fallbackCode).toBe(expected);
  });

  it.each(['cookieDatabaseCopyFailed', 'cookieDecryptionFailed', 'networkTimeout', 'networkConnectionFailed', 'diskSpaceInsufficient'])('provides specific three-language guidance for %s', (code) => {
    for (const locale of [en, zhCN, zhTW]) {
      const errors: Record<string, { shortMessage: string; message: string }> = locale.errors.runner;
      expect(errors[code]?.shortMessage).toBeTruthy();
      expect(errors[code]?.message.length).toBeGreaterThan(40);
    }
  });

  it.each([
    ['cookieDatabaseCopyFailed', 'Cookie 数据库复制失败', '可能', '关闭'],
    ['cookieDecryptionFailed', 'Cookie 解密失败', '可能', '更新'],
  ])('card shows Chinese cause and advice for %s', (code, shortMessage, cause, advice) => {
    vi.spyOn(useMediaDiagnosticsStore(), 'findLastFatalByGroupId').mockReturnValue({ ...fatal, internal: false });
    vi.spyOn(useMediaDiagnosticsStore(), 'findDiagnosticsByGroupId').mockReturnValue([{
      id: 'media', groupId: 'group', code, message: 'Original cookie error',
      level: 'error', raw: 'ERROR: Original cookie error', component: null, timestamp: 1,
    }]);
    const wrapper = mount(MediaErrorStep, {
      props: { group: { id: 'group', url: 'https://example.com' } as Group },
      global: { plugins: [i18n], stubs: { RouterLink: true } },
    });
    expect(wrapper.text()).toContain(shortMessage);
    expect(wrapper.text()).toContain(cause);
    expect(wrapper.text()).toContain(advice);
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
