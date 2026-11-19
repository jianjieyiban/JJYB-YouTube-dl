import * as Sentry from '@sentry/browser';
import { Scope } from '@sentry/core';
import { computed, Ref, ref } from 'vue';
import { useToastStore } from '../stores/toast.ts';
import { useI18n } from 'vue-i18n';
import { useMediaDiagnosticsStore } from '../stores/media/diagnostics.ts';
import { firstSentence } from '../helpers/diagnostics.ts';
import { useLinkify } from './useLinkify.ts';

type DisplayDiagnostic = {
  shortMessage: string;
  message: string;
  messageHtml: string;
  rawHtml: string;
};

export function useDiagnostic(diagnostic: Ref<MediaDiagnostic>) {
  const toastStore = useToastStore();
  const diagnosticsStore = useMediaDiagnosticsStore();

  const i18n = useI18n();
  const t = i18n.t;

  const { linkify } = useLinkify();

  const diagnosticDisplay = computed((): DisplayDiagnostic => {
    const i18nValues: Record<string, Record<string, string>> = i18n.tm('errors.runner');
    const code = diagnostic.value?.code;
    const localized = code && code !== 'unknown' ? i18nValues[code] : undefined;
    const raw = diagnostic.value?.raw ?? '';
    const originalMessage = diagnostic.value?.message?.trim() || raw.trim();
    const message = localized?.message ?? (originalMessage || t('errors.runner.unknown.message'));
    const shortMessage = localized?.shortMessage ?? (originalMessage
      ? firstSentence(originalMessage)
      : t('errors.runner.unknown.shortMessage'));

    return {
      message,
      shortMessage,
      messageHtml: linkify(message),
      rawHtml: linkify(raw),
    };
  });

  function report() {
    isReporting.value = true;
    try {
      Sentry.captureMessage(diagnosticDisplay.value.message, (scope: Scope) => {
        scope.setLevel('error');
        scope.setTag('user-reported', 'true');
        return scope;
      });
      toastStore.showToast(t('media.view.logs.toasts.reported'), { style: 'success' });
    } catch (e) {
      toastStore.showToast(t('media.view.logs.toasts.error'), { style: 'error' });
      console.error(e);
    } finally {
      isReporting.value = false;
      hasReported.value = true;
    }
  }

  const isReporting = ref(false);
  const hasReported = ref(false);

  const isReportable = computed(() => {
    return diagnostic.value.level === 'error' && diagnostic.value.code === 'unknown';
  });

  const relatedFatal = computed(() => {
    const fatals = diagnosticsStore.findFatalsByGroupId(diagnostic.value.groupId);
    return fatals.find(fatal => fatal.id === diagnostic.value?.id);
  });

  return { report, relatedFatal, isReportable, diagnosticDisplay, isReporting, hasReported };
}
