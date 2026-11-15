import * as Sentry from '@sentry/vue';
import router from './router.ts';
import { App } from 'vue';

export function createSentry(app: App<Element>) {
  const dsn = import.meta.env.VITE_JJYB_SENTRY_DSN?.trim();
  if (__E2E__ || __DEV__ || !dsn) return;
  Sentry.init({
    app,
    dsn,
    integrations: integrations => [
      ...integrations,
      Sentry.browserTracingIntegration({
        router,
        enableHTTPTimings: false,
        enableElementTiming: false,
      }),
    ],
    tracesSampleRate: 0.05,
    sampleRate: 0.25,
    environment: __DEV__ ? 'development' : 'production',
    release: __APP_VERSION__,
    beforeSend(event) {
      if (event.request?.url?.startsWith('ipc://')) return null;
      return event;
    },
    beforeSendTransaction(event) {
      event.spans = event.spans?.filter((span) => {
        if (span.op === 'measure') return false;
        if (span.op?.startsWith('browser')) return false;
        if (span.op?.startsWith('resource')) return false;
        return !span.description?.includes('ipc://');
      });
      if (event.request?.url?.startsWith('ipc://')) return null;
      if (event.spans?.length === 0) return null;
      return event;
    },
  });
}
