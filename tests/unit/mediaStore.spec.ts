import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { clearMocks } from '@tauri-apps/api/mocks';
import { reactive } from 'vue';
import { useMediaDiagnosticsStore } from '../../src/stores/media/diagnostics';
import { installTauriMock } from '../utils/tauriMock.ts';
import { useMediaStore } from '../../src/stores/media/media';
import { useMediaGroupStore } from '../../src/stores/media/group';
import { useMediaOptionsStore } from '../../src/stores/media/options';
import { useMediaStateStore, MediaState } from '../../src/stores/media/state';
import { useSettingsStore } from '../../src/stores/settings';
import { defaultSettings } from '../../src/tauri/types/config';
import { TrackType, type EntryItem, type MediaAddPayload } from '../../src/tauri/types/media';
import type { Group } from '../../src/tauri/types/group';

const { notify, notifyGroup } = vi.hoisted(() => ({
  notify: vi.fn(),
  notifyGroup: vi.fn(),
}));

vi.mock('../../src/tauri/notifications', () => ({
  notify,
  notifyGroup,
}));

function createEntries(total: number, prefix = 'https://example.com/playlist'): EntryItem[] {
  return Array.from({ length: total }, (_, index) => ({
    index,
    videoUrl: `${prefix}?v=${index + 1}`,
  }));
}

function createPlaylistGroup(groupId: string, total: number): Group {
  const entries = createEntries(total);
  return {
    id: groupId,
    url: 'https://example.com/playlist',
    title: 'Playlist',
    total,
    processed: 0,
    errored: 0,
    isCombined: false,
    audioCodecs: [],
    videoCodecs: [],
    audioTracks: [],
    videoTracks: [],
    formats: [],
    filesize: 0,
    entries,
    items: {
      leader: {
        id: 'leader',
        isLeader: true,
        url: 'https://example.com/playlist',
        title: 'Playlist',
        audioCodecs: [],
        videoCodecs: [],
        audioTracks: [],
        videoTracks: [],
        formats: [],
        filesize: 0,
        entries: [...entries],
        playlistCount: total,
      },
    },
  };
}

function flushAsync() {
  return new Promise(resolve => setTimeout(resolve, 0));
}

describe('media store', () => {
  beforeEach(() => {
    notify.mockReset();
    notifyGroup.mockReset();
    clearMocks();
  });

  afterEach(() => {
    clearMocks();
  });

  it('downloads options containing nested Vue proxies from the format selector', async () => {
    const download = vi.fn(() => 'ok');
    installTauriMock({ media_download: download });
    const group = createPlaylistGroup('nested-options', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const videoCodecs = reactive([{ id: 'avc1', label: 'H.264' }]);
    await useMediaStore().downloadGroup(group.id, { trackType: TrackType.both, videoCodecs });
    expect(download).toHaveBeenCalledOnce();
    expect(useMediaStateStore().getGroupState(group.id)).toBe(MediaState.downloading);
  });

  it('reports preparation failures visibly without leaving tasks downloading', async () => {
    installTauriMock({});
    const group = createPlaylistGroup('prepare-error', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const options = { trackType: TrackType.both, get height(): number {
      throw new Error('Invalid format');
    } };
    await expect(useMediaStore().downloadGroup(group.id, options)).resolves.toBeUndefined();
    expect(useMediaStateStore().getGroupState(group.id)).toBe(MediaState.error);
    expect(useMediaDiagnosticsStore().findLastFatalByGroupId(group.id)?.message).toContain('Invalid format');
  });

  it('does not submit the same running task twice', async () => {
    const download = vi.fn(() => 'ok');
    installTauriMock({ media_download: download });
    const group = createPlaylistGroup('duplicate-download', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    await useMediaStore().downloadGroup(group.id, { trackType: TrackType.both });
    await useMediaStore().downloadGroup(group.id, { trackType: TrackType.both });
    expect(download).toHaveBeenCalledOnce();
  });

  it('marks every item failed when a combined task cannot reach the backend', async () => {
    installTauriMock({ media_download: () => {
      throw new Error('IPC unavailable');
    } });
    const group = createPlaylistGroup('combined-ipc-error', 2);
    group.isCombined = true;
    group.items.child = { ...group.items.leader, id: 'child', entries: undefined, isLeader: false };
    useMediaGroupStore().createGroup(group);
    await useMediaStore().downloadGroup(group.id, { trackType: TrackType.both });
    expect(useMediaStateStore().getState('child')).toBe(MediaState.error);
    expect(useMediaStateStore().getGroupState(group.id)).toBe(MediaState.error);
  });

  it('submits a standalone thumbnail task without changing ordinary saved options', async () => {
    let payload: unknown;
    installTauriMock({ media_download: (_cmd, args) => {
      payload = args;
    } });
    const group = createPlaylistGroup('thumbnail-only', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOptions(group.id, { trackType: TrackType.both, height: 720 });
    await useMediaStore().downloadThumbnail(group.id);
    expect(payload).toMatchObject({ items: [{ thumbnailOnly: true, subtitlesOnly: false, format: { thumbnailOnly: true, subtitlesOnly: false } }] });
    expect(optionsStore.getOptions(group.id)).toEqual({ trackType: TrackType.both, height: 720 });
  });

  it.each(['ordinary', 'bulk'])('keeps the %s job subtitle choice when resuming and retrying', async (mode) => {
    const group = createPlaylistGroup('subtitle-job', 1);
    delete group.entries;
    delete group.items.leader.entries;
    group.items.leader.subtitleInventory = { manualLanguages: ['ja'], autoLanguages: [] };
    useMediaGroupStore().createGroup(group);
    const stateStore = useMediaStateStore();
    stateStore.setGroupState(group.id, MediaState.configure);
    const optionsStore = useMediaOptionsStore();
    const overrides = { subtitles: { enabled: false, languages: ['ja'] } };
    optionsStore.setOverrides(group.id, overrides);
    const mediaStore = useMediaStore();
    const download = vi.fn(() => 'ok');
    installTauriMock({
      media_download: download,
      media_info: (_cmd, args) => {
        const { groupId, id } = args as { groupId: string; id: string };
        mediaStore.processMediaAddPayload({ groupId, total: 1, item: { ...group.items.leader, id } });
        return groupId;
      },
    });

    if (mode === 'bulk') await mediaStore.downloadAllGroups();
    else await mediaStore.downloadGroup(group.id, { trackType: TrackType.both });
    await mediaStore.pauseGroup(group.id);
    optionsStore.setOverrides(group.id, { subtitles: { enabled: true, languages: ['en'] } });
    await mediaStore.resumeGroup(group.id);
    stateStore.setGroupState(group.id, MediaState.error);
    await mediaStore.retryGroup(group.id);

    expect(download).toHaveBeenCalledTimes(3);
    for (let call = 1; call <= 3; call++) {
      expect(download).toHaveBeenNthCalledWith(call, 'media_download', expect.objectContaining({
        items: [expect.objectContaining({ overrides })],
      }));
    }
  });

  it('waits for submission before pausing and for pause before resuming', async () => {
    const group = createPlaylistGroup('rapid-pause', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const mediaStore = useMediaStore();
    let finishSubmission!: (value: string) => void;
    let finishPause!: () => void;
    const submission = new Promise<string>((resolve) => {
      finishSubmission = resolve;
    });
    const pause = new Promise<void>((resolve) => {
      finishPause = resolve;
    });
    const download = vi.fn().mockReturnValueOnce(submission).mockReturnValue('ok');
    const cancel = vi.fn(() => pause);
    installTauriMock({ media_download: download, group_cancel: cancel });

    const downloading = mediaStore.downloadGroup(group.id, { trackType: TrackType.both });
    const pausing = mediaStore.pauseGroup(group.id);
    const resuming = mediaStore.resumeGroup(group.id);
    expect(cancel).not.toHaveBeenCalled();
    expect(download).toHaveBeenCalledTimes(1);
    finishSubmission('ok');
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith('group_cancel', { groupId: group.id, retainPartial: true }));
    expect(download).toHaveBeenCalledTimes(1);
    finishPause();
    await Promise.all([downloading, pausing, resuming]);
    expect(download).toHaveBeenCalledTimes(2);

    mediaStore.deleteGroup(group.id);
    expect(cancel).toHaveBeenLastCalledWith('group_cancel', { groupId: group.id, retainPartial: false });
  });

  it('does not resume an item whose publication won the pause race', async () => {
    const group = createPlaylistGroup('publication-won', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const mediaStore = useMediaStore();
    const download = vi.fn().mockReturnValue('ok');
    installTauriMock({
      media_download: download,
      group_cancel: () => ({ completed: [{ id: 'leader', destination: 'saved.mp4' }], failed: [] }),
    });
    await mediaStore.downloadGroup(group.id, { trackType: TrackType.both });
    await Promise.all([mediaStore.pauseGroup(group.id), mediaStore.resumeGroup(group.id)]);
    expect(download).toHaveBeenCalledTimes(1);
    expect(useMediaStateStore().getState('leader')).toBe(MediaState.done);
  });

  it.each(['single', 'all'])('discards prior attempt diagnostics when resuming via %s', async (mode) => {
    const group = createPlaylistGroup('resume-diagnostics', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const mediaStore = useMediaStore();
    const diagnostics = useMediaDiagnosticsStore();
    const download = vi.fn().mockReturnValue('ok');
    installTauriMock({ media_download: download, group_cancel: () => undefined });

    await mediaStore.downloadGroup(group.id, { trackType: TrackType.both });
    await mediaStore.pauseGroup(group.id);
    diagnostics.diagnostics.leader = [{} as MediaDiagnostic];
    diagnostics.fatals[group.id] = {} as MediaFatal;

    if (mode === 'single') await mediaStore.resumeGroup(group.id);
    else mediaStore.resumeAllGroups();
    await vi.waitFor(() => expect(download).toHaveBeenCalledTimes(2));
    expect(diagnostics.diagnostics.leader).toBeUndefined();
    expect(diagnostics.fatals[group.id]).toBeUndefined();
  });

  it('ignores a late fatal from the attempt that was paused', async () => {
    const group = createPlaylistGroup('late-fatal', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const download = vi.fn<(_cmd: string, _args?: unknown) => string>().mockReturnValue('ok');
    installTauriMock({ media_download: download });
    const mediaStore = useMediaStore();
    await mediaStore.downloadGroup(group.id, { trackType: TrackType.both });
    const oldAttempt = (download.mock.calls[0][1] as { attemptId?: string }).attemptId;
    await mediaStore.pauseGroup(group.id);
    await mediaStore.resumeGroup(group.id);
    const newAttempt = (download.mock.calls[1][1] as { attemptId?: string }).attemptId;
    const fatal = { groupId: group.id, id: 'leader', exitCode: 1, internal: true, message: 'failure', details: null, timestamp: Date.now() };
    const diagnostics = useMediaDiagnosticsStore();
    diagnostics.processMediaFatalPayload({ ...fatal, attemptId: oldAttempt ?? 'stale' });
    expect(useMediaStateStore().getGroupState(group.id)).toBe(MediaState.downloading);
    expect(diagnostics.findLastFatalByGroupId(group.id)).toBeUndefined();
    expect(oldAttempt).toBeTruthy();
    expect(newAttempt).not.toBe(oldAttempt);
    diagnostics.processMediaFatalPayload({ ...fatal, attemptId: newAttempt });
    expect(useMediaStateStore().getGroupState(group.id)).toBe(MediaState.error);
  });

  it('submits subtitle-only tasks without AV formats and preserves explicit languages', async () => {
    let payload: Record<string, unknown> | undefined;
    installTauriMock({
      media_download: (_cmd, args) => {
        payload = args as Record<string, unknown>;
        return 'ok';
      },
    });
    const group = createPlaylistGroup('subtitles-only', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    useMediaOptionsStore().setOverrides(group.id, { subtitles: { enabled: false, languages: ['ja'], embedSubtitles: true } });
    await useMediaStore().downloadSubtitles(group.id);
    expect(payload).toMatchObject({ items: [{
      subtitlesOnly: true,
      format: { subtitlesOnly: true },
      overrides: { subtitles: { enabled: true, languages: ['ja'], embedSubtitles: false } },
    }] });
    expect(useMediaOptionsStore().getOptions(group.id)).toBeUndefined();
    expect(useMediaOptionsStore().getOverrides(group.id)?.subtitles?.embedSubtitles).toBe(true);
  });

  it('keeps a subsequent ordinary download in AV mode after a subtitle-only action', async () => {
    const payloads: unknown[] = [];
    installTauriMock({
      media_download: (_cmd, args) => {
        payloads.push(args);
        return 'ok';
      },
    });
    const group = createPlaylistGroup('subtitle-then-video', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOptions(group.id, { trackType: TrackType.both, height: 720 });
    optionsStore.setOverrides(group.id, { subtitles: { embedSubtitles: true } });
    const mediaStore = useMediaStore();

    await mediaStore.downloadSubtitles(group.id);
    useMediaStateStore().setGroupState(group.id, MediaState.configure);
    await mediaStore.downloadGroup(group.id, optionsStore.getOptions(group.id)!);

    expect(payloads[0]).toMatchObject({ items: [{ subtitlesOnly: true }] });
    expect(payloads[1]).toMatchObject({ items: [{
      subtitlesOnly: false,
      format: { trackType: TrackType.both, height: 720 },
      overrides: { subtitles: { embedSubtitles: true } },
    }] });
    expect(optionsStore.getOptions(group.id)).toEqual({ trackType: TrackType.both, height: 720 });
  });

  it.each(['single', 'all'])('resumes subtitle-only execution via %s without changing saved AV options', async (mode) => {
    const payloads: unknown[] = [];
    installTauriMock({ media_download: (_cmd, args) => {
      payloads.push(args);
      return 'ok';
    } });
    const group = createPlaylistGroup('resume-subtitles', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOptions(group.id, { trackType: TrackType.both, height: 720 });
    optionsStore.setEncodings(group.id, { audio: 'aac' });
    const mediaStore = useMediaStore();
    await mediaStore.downloadSubtitles(group.id);
    await mediaStore.pauseGroup(group.id);
    optionsStore.setEncodings(group.id, { audio: 'opus' });
    if (mode === 'single') await mediaStore.resumeGroup(group.id);
    else mediaStore.resumeAllGroups();
    await flushAsync();
    expect(payloads).toHaveLength(2);
    expect(payloads[1]).toMatchObject({ items: [{ subtitlesOnly: true, format: { audioEncoding: 'aac', height: 720 } }] });
    expect(optionsStore.getOptions(group.id)).toEqual({ trackType: TrackType.both, height: 720 });
  });

  it.each([true, false])('retries a failed execution with its resolved mode (%s) and cloned overrides before metadata arrives', async (subtitlesOnly) => {
    const group = createPlaylistGroup('retry-mode', 1);
    delete group.entries;
    delete group.items.leader.entries;
    useMediaGroupStore().createGroup(group);
    const optionsStore = useMediaOptionsStore();
    optionsStore.setOptions(group.id, { trackType: TrackType.both, height: 720 });
    optionsStore.setEncodings(group.id, { audio: 'aac' });
    optionsStore.setOverrides(group.id, { subtitles: { languages: ['ja'], embedSubtitles: true }, network: { proxy: 'http://old' } });
    const payloads: unknown[] = [];
    const mediaStore = useMediaStore();
    installTauriMock({
      media_download: (_cmd, args) => {
        payloads.push(args);
        return 'ok';
      },
      media_info: (_cmd, args) => {
        const { groupId, id, overrides } = args as { groupId: string; id: string; overrides: unknown };
        expect(overrides).toEqual({ subtitles: { languages: ['ja'], embedSubtitles: true }, network: { proxy: 'http://old' } });
        optionsStore.getOverrides(group.id)!.subtitles!.languages!.push('en');
        mediaStore.processMediaAddPayload({ groupId, total: 1, item: { ...group.items.leader, id } });
        return groupId;
      },
    });
    if (subtitlesOnly) await mediaStore.downloadSubtitles(group.id);
    else await mediaStore.downloadGroup(group.id, optionsStore.getOptions(group.id)!);
    useMediaStateStore().setGroupState(group.id, MediaState.error);
    optionsStore.setEncodings(group.id, { audio: 'opus' });
    await mediaStore.retryGroup(group.id);
    expect(payloads).toHaveLength(2);
    expect(payloads[1]).toMatchObject({ items: [{
      subtitlesOnly,
      format: { height: 720, audioEncoding: 'aac' },
      overrides: { subtitles: { languages: ['ja'], embedSubtitles: !subtitlesOnly }, network: { proxy: 'http://old' } },
    }] });
    expect(optionsStore.getOptions(group.id)?.subtitlesOnly).toBeUndefined();
  });

  it('retries every ready child after the original URL expands and splits', async () => {
    const group = createPlaylistGroup('retry-playlist', 2);
    group.items.child = { ...group.items.leader, id: 'child', entries: undefined, isLeader: false, url: 'https://example.com/playlist?v=1' };
    useMediaGroupStore().createGroup(group);
    useSettingsStore().settings.performance.splitPlaylistThreshold = 10;
    useMediaOptionsStore().setOverrides(group.id, { subtitles: { languages: ['ja'] } });
    const mediaStore = useMediaStore();
    const downloads: unknown[] = [];
    installTauriMock({
      media_download: (_cmd, args) => {
        downloads.push(args);
        return 'ok';
      },
      media_info: (_cmd, args) => {
        const { groupId, id } = args as { groupId: string; id: string };
        setTimeout(() => mediaStore.processMediaAddPayload({ groupId, total: 2, item: { ...group.items.leader, id } }), 0);
        return groupId;
      },
      media_playlist_expand: (_cmd, args) => {
        const { groupId } = args as { groupId: string };
        for (const [index, entry] of group.entries!.entries()) {
          mediaStore.processMediaAddPayload({ groupId, total: 2, item: { ...group.items.child, id: `retry-child-${index}`, url: entry.videoUrl } });
        }
        return groupId;
      },
    });
    await mediaStore.downloadSubtitles(group.id);
    await mediaStore.retryGroup(group.id);
    expect(downloads).toHaveLength(3);
    for (const payload of downloads.slice(1)) {
      expect(payload).toMatchObject({ items: [{ subtitlesOnly: true, overrides: { subtitles: { languages: ['ja'], enabled: true, embedSubtitles: false } } }] });
    }
  });

  it('clears execution snapshots when a group is deleted', async () => {
    const payloads: unknown[] = [];
    installTauriMock({ media_download: (_cmd, args) => {
      payloads.push(args);
      return 'ok';
    } });
    const group = createPlaylistGroup('deleted-snapshot', 1);
    delete group.entries;
    delete group.items.leader.entries;
    const mediaStore = useMediaStore();
    useMediaGroupStore().createGroup(group);
    await mediaStore.downloadSubtitles(group.id);
    mediaStore.deleteGroup(group.id);
    useMediaGroupStore().createGroup(group);
    useMediaOptionsStore().setOptions(group.id, { trackType: TrackType.both });
    await mediaStore.resumeGroup(group.id);
    expect(payloads[1]).toMatchObject({ items: [{ subtitlesOnly: false }] });
  });

  it('applies input filter overrides when dispatching media info fetches', async () => {
    const settingsStore = useSettingsStore();
    Object.assign(settingsStore.settings, structuredClone(defaultSettings));
    settingsStore.settings.inputFilters = {
      ...settingsStore.settings.inputFilters,
      minSize: { value: 50, unit: 'MB' },
      dateFilter: { mode: 'after', value: '2026-06-01' },
    };

    const calls: Array<Record<string, unknown>> = [];
    installTauriMock({
      media_info: (_cmd, args) => {
        const payload = args as Record<string, unknown>;
        calls.push(payload);
        return payload.groupId as string;
      },
    });

    const mediaStore = useMediaStore();
    const groupId = await mediaStore.dispatchMediaInfoFetch('https://example.com/video');

    expect(groupId).toBeTypeOf('string');
    expect(calls).toHaveLength(1);
    expect(calls[0].overrides).toEqual({
      inputFilters: {
        minFilesize: '50M',
        dateafter: '20260601',
      },
    });
  });

  it('expands playlists with the selected subset and updates overrides', async () => {
    let expandArgs: Record<string, unknown> | undefined;
    installTauriMock({
      media_playlist_expand: (_cmd, args) => {
        expandArgs = args as Record<string, unknown>;
        return 'group-1';
      },
    });

    const groupStore = useMediaGroupStore();
    const optionsStore = useMediaOptionsStore();
    const stateStore = useMediaStateStore();
    const mediaStore = useMediaStore();

    const group = createPlaylistGroup('group-1', 3);
    groupStore.createGroup(group);
    optionsStore.setOverrides('group-1', {
      inputFilters: {
        matchFilters: '!is_live',
      },
    });

    await mediaStore.expandPlaylistGroup('group-1', {
      rows: [{ id: 'row-1', type: 'range', start: 1, end: 2 }],
    });

    expect(group.entries?.map(entry => entry.videoUrl)).toEqual([
      'https://example.com/playlist?v=1',
      'https://example.com/playlist?v=2',
    ]);
    expect(stateStore.getGroupState('group-1')).toBe(MediaState.fetchingList);
    expect(optionsStore.getOverrides('group-1')).toEqual({
      inputFilters: {
        matchFilters: '!is_live',
        playlistItems: '1:2',
      },
    });
    expect(expandArgs).toMatchObject({
      groupId: 'group-1',
      entries: group.entries,
      overrides: {
        inputFilters: {
          matchFilters: '!is_live',
          playlistItems: '1:2',
        },
      },
    });
  });

  it('removes playlist item overrides when selecting the full playlist', async () => {
    installTauriMock({
      media_playlist_expand: () => 'group-1',
    });

    const groupStore = useMediaGroupStore();
    const optionsStore = useMediaOptionsStore();
    const mediaStore = useMediaStore();

    const group = createPlaylistGroup('group-1', 2);
    groupStore.createGroup(group);
    optionsStore.setOverrides('group-1', {
      inputFilters: {
        playlistItems: '1',
      },
    });

    await mediaStore.expandPlaylistGroup('group-1', { rows: [] });

    expect(optionsStore.getOverrides('group-1')).toBeUndefined();
    expect(group.entries).toHaveLength(2);
  });

  it('falls back to playlist selection when auto-expanding a skipped playlist fails', async () => {
    installTauriMock({
      media_playlist_expand: () => {
        throw new Error('expand failed');
      },
    });

    const groupStore = useMediaGroupStore();
    const stateStore = useMediaStateStore();
    const mediaStore = useMediaStore();

    groupStore.createGroup({
      id: 'group-1',
      url: 'https://example.com/playlist',
      total: 1,
      processed: 0,
      errored: 0,
      isCombined: false,
      audioCodecs: [],
      videoCodecs: [],
      audioTracks: [],
      videoTracks: [],
      formats: [],
      filesize: 0,
      skipPlaylistSelection: true,
      items: {
        placeholder: {
          id: 'placeholder',
          isLeader: true,
          url: 'https://example.com/playlist',
          audioCodecs: [],
          formats: [],
          filesize: 0,
        },
      },
    });

    const payload: MediaAddPayload = {
      groupId: 'group-1',
      total: 2,
      item: {
        id: 'leader',
        url: 'https://example.com/playlist',
        title: 'Playlist',
        audioCodecs: [],
        formats: [],
        filesize: 0,
        entries: createEntries(2),
      },
    };

    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});

    mediaStore.processMediaAddPayload(payload);
    await flushAsync();

    expect(stateStore.getState('leader')).toBe(MediaState.playlistSelection);

    consoleError.mockRestore();
  });
});
