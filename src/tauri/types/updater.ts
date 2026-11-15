export interface UpdaterStatusPayload {
  configured: boolean;
  reason: string | null;
  releasesUrl: string;
}

export interface UpdaterCheckPayload extends UpdaterStatusPayload {
  available: boolean;
  currentVersion: string;
  availableVersion?: string | null;
}

export interface UpdaterDownloadProgressPayload {
  received: number;
  total: number | null;
}
