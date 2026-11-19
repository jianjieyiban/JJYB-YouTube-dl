interface MediaDiagnostic {
  id: string;
  groupId: string;
  attemptId?: string;
  level: 'error' | 'warning';
  code: string;
  component: string | null;
  message: string;
  raw: string;
  timestamp: number;
}

interface MediaFatal {
  id: string;
  groupId: string;
  attemptId?: string;
  exitCode: number | null;
  internal: boolean;
  code?: 'noSubtitles' | 'outputPathConflict' | null;
  message: string;
  details: string | null;
  timestamp: number;
}
