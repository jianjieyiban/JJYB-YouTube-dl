import { IPCHandler } from '../tauriMock';
import { emit } from '@tauri-apps/api/event';
import { UpdaterCheckPayload, UpdaterDownloadProgressPayload } from '../../../src/tauri/types/updater';

export type UpdaterMockData = {
  configured?: boolean;
  reason?: string | null;
  available?: boolean;
  currentVersion?: string;
  availableVersion?: string;
};

export const updateHandlers: Record<string, IPCHandler> = {
  updater_status: () => ({
    configured: window.E2E.updater?.configured ?? false,
    reason: window.E2E.updater?.configured ? null : (window.E2E.updater?.reason ?? '未配置自有更新公钥'),
    releasesUrl: 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases',
  }),
  updater_check: (): UpdaterCheckPayload => {
    return {
      configured: window.E2E.updater?.configured ?? false,
      reason: window.E2E.updater?.configured ? null : '未配置自有更新公钥',
      releasesUrl: 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases',
      available: !!window.E2E.updater?.configured && !!window.E2E.updater?.available,
      currentVersion: window.E2E.updater?.currentVersion ?? '1.0.0',
      availableVersion: window.E2E.updater?.availableVersion,
    };
  },
  updater_download: async (): Promise<void> => {
    if (window.E2E.updater?.available) {
      return new Promise((resolve): void => {
        setTimeout((): void => {
          void emit<UpdaterDownloadProgressPayload>('updater_download_progress', {
            received: 1024 * 1024 * 5,
            total: 1024 * 1024 * 10,
          });
        }, 1000);
        setTimeout((): void => {
          void emit<UpdaterDownloadProgressPayload>('updater_download_progress', {
            received: 1024 * 1024 * 10,
            total: 1024 * 1024 * 10,
          });
        }, 2000);
        setTimeout(resolve, 3000);
      });
    } else {
      return Promise.resolve();
    }
  },
  updater_install: (): void => {
    void emit<void>('updater_finished');
  },
};
