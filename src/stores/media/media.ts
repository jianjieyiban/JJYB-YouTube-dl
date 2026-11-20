import { defineStore } from 'pinia';

import { invoke } from '@tauri-apps/api/core';
import { v4 as uuidv4 } from 'uuid';
import { MediaState, useMediaStateStore } from './state';
import { useMediaGroupStore } from './group';
import { useMediaDestinationStore } from './destination';
import { useMediaProgressStore } from './progress';
import { useMediaOptionsStore } from './options';
import { DownloadOptions, DownloadOverrides, MediaAddPayload, MediaItem, TrackType } from '../../tauri/types/media';
import { useMediaSizeStore } from './size.ts';
import { useMediaDiagnosticsStore } from './diagnostics.ts';
import { useSettingsStore } from '../settings.ts';
import { Group } from '../../tauri/types/group.ts';
import { notify, notifyGroup } from '../../tauri/notifications';
import { NotificationKind } from '../../tauri/types/app';
import { resolvePlaylistIndex } from '../../helpers/playlistNumbering';
import { useWatchClipboardStore } from '../watchClipboard.ts';
import { settingsToInputFilterOverride } from '../../helpers/inputFilters.ts';
import {
  applyPlaylistSelectionToEntries,
  buildPlaylistItemsSpec,
  type PlaylistSelection,
} from '../../helpers/playlistSelection.ts';

type PendingReadyGroup = {
  resolve: (groupIds: string[]) => void;
  reject: (reason?: unknown) => void;
};

type DownloadExecution = {
  options: DownloadOptions;
  overrides?: DownloadOverrides;
};

function cloneDownloadOverrides(overrides?: DownloadOverrides): DownloadOverrides | undefined {
  if (!overrides) return undefined;
  return structuredClone(overrides);
}

export const useMediaStore = defineStore('media', () => {
  const groupStore = useMediaGroupStore();
  const stateStore = useMediaStateStore();
  const progressStore = useMediaProgressStore();
  const destinationStore = useMediaDestinationStore();
  const optionsStore = useMediaOptionsStore();
  const sizeStore = useMediaSizeStore();
  const diagnosticsStore = useMediaDiagnosticsStore();
  const settingsStore = useSettingsStore();
  const watchClipboardStore = useWatchClipboardStore();
  const pendingReadyGroups = new Map<string, PendingReadyGroup>();
  const executions = new Map<string, DownloadExecution>();
  const pendingSubmissions = new Map<string, Promise<string>>();
  const pendingPauses = new Map<string, Promise<void>>();

  function resolvePendingReadyGroup(groupId: string, resolvedIds: string[]) {
    const pending = pendingReadyGroups.get(groupId);
    if (!pending) return;
    pending.resolve(resolvedIds);
    pendingReadyGroups.delete(groupId);
  }

  function rejectPendingReadyGroup(groupId: string, reason?: unknown) {
    const pending = pendingReadyGroups.get(groupId);
    if (!pending) return;
    pending.reject(reason);
    pendingReadyGroups.delete(groupId);
  }

  function finalizePlaylistGroup(group: Group) {
    if (!Object.values(group.items).some(item => !item.entries)) {
      stateStore.setGroupState(group.id, MediaState.error);
      rejectPendingReadyGroup(group.id, diagnosticsStore.findLastFatalByGroupId(group.id)?.message);
      return;
    }
    const splitThreshold = settingsStore.settings.performance.splitPlaylistThreshold;
    const overrides = optionsStore.getOverrides(group.id);
    if (group.total < splitThreshold) {
      const newGroups = groupStore.splitGroup(group);
      migrateOverrides(group.id, newGroups.map(newGroup => newGroup.id), overrides);
      void notifyGroup(NotificationKind.PlaylistReady, group, {}, newGroups.length);
      resolvePendingReadyGroup(group.id, newGroups.map(newGroup => newGroup.id));
      if (newGroups.length === 0) {
        stateStore.setGroupState(group.id, MediaState.configure);
        return;
      }
      for (const newGroup of newGroups) {
        stateStore.setGroupState(newGroup.id, MediaState.configure);
      }
    } else {
      groupStore.consolidateGroup(group);
      void notifyGroup(NotificationKind.PlaylistReady, group, {}, group.entries?.length ?? 1);
      stateStore.setGroupState(group.id, MediaState.configure);
      resolvePendingReadyGroup(group.id, [group.id]);
    }
  }

  function processMediaAddPayload(payload: MediaAddPayload) {
    const { item, groupId, total } = payload;
    item.groupId = groupId;

    const group = groupStore.findGroupById(groupId);
    if (!group) throw new Error('Orphaned media item found.');

    if (item.entries) {
      const existingId = Object.keys(group.items)[0];
      if (existingId && existingId !== item.id) {
        delete group.items[existingId];
      }

      item.isLeader = true;
      group.items[item.id] = item;
      const { id, groupId: gid, isLeader, ...meta } = item;
      void id;
      void gid;
      void isLeader;
      Object.assign(group, meta);
      group.total = total;
      group.processed = 0;
      if (group.skipPlaylistSelection) {
        void expandPlaylistGroup(groupId, { rows: [] }).catch((error) => {
          console.error(error);
          stateStore.setState(item.id, MediaState.playlistSelection);
        });
      } else {
        stateStore.setState(item.id, MediaState.playlistSelection);
      }
      return;
    }

    const leader = groupStore.findGroupLeader(groupId);
    const hasPlaylistLeader = !!leader?.entries;
    const isFirst = group.total === 1 && !hasPlaylistLeader;
    if (isFirst) {
      const existingId = Object.keys(group.items)[0];
      if (existingId) delete group.items[existingId];
    }
    item.isLeader = isFirst;
    group.items[item.id] = item;

    if (isFirst) {
      const { id, groupId: gid, isLeader, ...meta } = item;
      void id;
      void gid;
      void isLeader;
      Object.assign(group, meta);
    }

    group.total = total;
    if (!isFirst) group.processed++;

    if (group.processed === total) {
      finalizePlaylistGroup(group);
    } else if (group.total === 1 && !hasPlaylistLeader) {
      void notifyGroup(NotificationKind.VideoReady, group);
      resolvePendingReadyGroup(group.id, [group.id]);
    }

    const next = total > 1 && isFirst
      ? MediaState.fetchingList
      : MediaState.configure;
    stateStore.setState(item.id, next);
  }

  async function expandPlaylistGroup(groupId: string, selection: PlaylistSelection) {
    const group = groupStore.findGroupById(groupId);
    const leader = groupStore.findGroupLeader(groupId);
    const entries = group?.entries;
    if (!group || !leader?.entries || !entries) {
      throw new Error(`Playlist group ${groupId} is missing entry metadata.`);
    }

    const selectedEntries = applyPlaylistSelectionToEntries(entries, selection);
    if (selectedEntries.length === 0) {
      throw new Error('No playlist entries match the selected range.');
    }
    const spec = buildPlaylistItemsSpec(selection);
    const previousOverrides = optionsStore.getOverrides(groupId);
    const nextOverrides = cloneDownloadOverrides(previousOverrides) ?? {};

    if (spec) {
      nextOverrides.inputFilters = {
        ...(nextOverrides.inputFilters ?? {}),
        playlistItems: spec,
      };
    } else if (nextOverrides.inputFilters) {
      delete nextOverrides.inputFilters.playlistItems;
      if (Object.keys(nextOverrides.inputFilters).length === 0) {
        delete nextOverrides.inputFilters;
      }
    }

    if (Object.keys(nextOverrides).length > 0) {
      optionsStore.setOverrides(groupId, nextOverrides);
    } else {
      optionsStore.removeOverrides(groupId);
    }

    group.entries = [...selectedEntries];
    leader.entries = [...selectedEntries];
    group.total = selectedEntries.length;
    group.processed = 0;
    stateStore.setGroupState(groupId, MediaState.fetchingList);

    await invoke<string>('media_playlist_expand', {
      groupId,
      entries: selectedEntries,
      overrides: cloneDownloadOverrides(optionsStore.getOverrides(groupId)),
    });
  }

  async function dispatchMediaInfoFetch(
    url: string,
    fromShortcut: boolean = false,
    skipPlaylistSelection: boolean = false,
    overrides?: DownloadOverrides,
  ) {
    const id = uuidv4();
    const groupId = uuidv4();

    stateStore.setState(id, MediaState.fetching);
    const newGroup: Group = {
      id: groupId,
      total: 1,
      processed: 0,
      errored: 0,
      isCombined: false,
      url,
      audioCodecs: [],
      videoCodecs: [],
      audioTracks: [],
      videoTracks: [],
      formats: [],
      filesize: 0,
      fromShortcut,
      skipPlaylistSelection,
      items: {
        [id]: {
          id,
          isLeader: true,
          url,
          audioCodecs: [],
          videoCodecs: [],
          audioTracks: [],
          videoTracks: [],
          formats: [],
          filesize: 0,
        },
      },
    };
    groupStore.createGroup(newGroup);

    const inputFiltersOverride = settingsToInputFilterOverride(settingsStore.settings);
    if (overrides) {
      optionsStore.setOverrides(groupId, cloneDownloadOverrides(overrides)!);
    } else if (inputFiltersOverride) {
      optionsStore.setOverrides(groupId, { inputFilters: inputFiltersOverride });
    }

    await invoke('media_info', {
      url,
      id,
      groupId,
      overrides: optionsStore.getOverrides(groupId),
    });
    await notifyGroup(NotificationKind.QueueAdded, newGroup);
    return groupId;
  }

  function migrateOverrides(
    previousGroupId: string,
    nextGroupIds: string[],
    overrides?: DownloadOverrides,
  ) {
    if (!overrides) return;
    for (const nextGroupId of nextGroupIds) {
      const clonedOverrides = cloneDownloadOverrides(overrides);
      if (clonedOverrides) {
        optionsStore.setOverrides(nextGroupId, clonedOverrides);
      }
    }
    optionsStore.removeOverrides(previousGroupId);
  }

  function waitForGroupReady(groupId: string): Promise<string[]> {
    const group = groupStore.findGroupById(groupId);
    const state = stateStore.getGroupState(groupId);

    if (group && state === MediaState.configure) {
      return Promise.resolve([groupId]);
    }
    if (group && state === MediaState.error) {
      return Promise.reject(diagnosticsStore.findLastFatalByGroupId(groupId)?.message);
    }

    return new Promise<string[]>((resolve, reject) => {
      pendingReadyGroups.set(groupId, { resolve, reject });
    });
  }

  function getDownloadOptions(groupId: string): DownloadOptions {
    return optionsStore.getOptions(groupId) ?? optionsStore.getGlobalOptions() ?? { trackType: TrackType.both };
  }

  async function addAndDownload(url: string, fromShortcut: boolean = false) {
    await addUrlBatchAndDownload([url], fromShortcut, true);
  }

  async function addUrlBatch(
    urls: string[],
    fromShortcut: boolean = false,
    skipPlaylistSelection: boolean = false,
  ): Promise<string[]> {
    return await Promise.all(
      urls.map(url => dispatchMediaInfoFetch(url, fromShortcut, skipPlaylistSelection)),
    );
  }

  async function addUrlBatchAndDownload(
    urls: string[],
    fromShortcut: boolean = false,
    skipPlaylistSelection: boolean = false,
  ): Promise<string[]> {
    const initialGroupIds = await addUrlBatch(urls, fromShortcut, skipPlaylistSelection);
    const readyGroupIds = [...new Set(
      (await Promise.all(initialGroupIds.map(groupId => waitForGroupReady(groupId)))).flat(),
    )];

    await notify(NotificationKind.QueueDownloading, { n: readyGroupIds.length.toString() }, fromShortcut);
    await Promise.all(
      readyGroupIds.map(groupId => downloadGroup(groupId, getDownloadOptions(groupId))),
    );

    return readyGroupIds;
  }

  async function downloadSubtitles(groupId: string) {
    const options = { ...getDownloadOptions(groupId), subtitlesOnly: true, thumbnailOnly: false };
    await downloadGroup(groupId, options);
  }

  async function downloadThumbnail(groupId: string) {
    await downloadGroup(groupId, { ...getDownloadOptions(groupId), subtitlesOnly: false, thumbnailOnly: true });
  }

  async function downloadGroup(
    groupId: string,
    options: DownloadOptions,
    execution?: DownloadExecution,
  ) {
    watchClipboardStore.disable();
    const pause = pendingPauses.get(groupId);
    if (pause) await pause.catch(() => undefined);
    const group = groupStore.findGroupById(groupId);
    if (!group) return;
    const currentState = stateStore.getGroupState(groupId);
    if (currentState === MediaState.downloading || currentState === MediaState.downloadingList) return;

    try {
      const items: MediaItem[] = Object.values(group.items);
      const includeCompleted = !execution && (options.subtitlesOnly || options.thumbnailOnly);
      const itemsWithoutLeader: MediaItem[] = items.filter(item => !item.entries && (includeCompleted || stateStore.getState(item.id) !== MediaState.done));
      const encodings = optionsStore.getEncodings(groupId);
      const tracks = optionsStore.getTracks(groupId);
      const existingOverrides = cloneDownloadOverrides(execution ? execution.overrides : optionsStore.getOverrides(groupId));
      const overrides = options.subtitlesOnly
        ? { ...existingOverrides, subtitles: { ...existingOverrides?.subtitles, enabled: true, embedSubtitles: false } }
        : existingOverrides;
      const resolvedOptions: DownloadOptions = {
        ...options,
        videoCodecs: options.videoCodecs?.map(codec => typeof codec === 'string' ? codec : { ...codec }),
        audioTrackIds: options.audioTrackIds ? [...options.audioTrackIds] : undefined,
        videoTrackIds: options.videoTrackIds ? [...options.videoTrackIds] : undefined,
        ...(!execution && {
          audioEncoding: encodings?.audio,
          videoEncoding: encodings?.video,
          audioTrack: tracks?.audio,
          videoTrack: tracks?.video,
        }),
      };

      if (itemsWithoutLeader.length === 0) return;
      const restartedIds = new Set([groupId, ...itemsWithoutLeader.map(item => item.id)]);
      for (const id of restartedIds) diagnosticsStore.removeDiagnostics(id);
      executions.set(groupId, { options: resolvedOptions, overrides: existingOverrides });

      const newState = group.isCombined ? MediaState.downloadingList : MediaState.downloading;
      items
        .filter(item => includeCompleted || stateStore.getState(item.id) !== MediaState.done)
        .forEach(item => stateStore.setState(item.id, newState));

      const attemptId = uuidv4();
      groupStore.setDownloadAttempt(groupId, attemptId);
      const submission = invoke<string>('media_download', {
        groupId,
        attemptId,
        items: itemsWithoutLeader.map(item => ({
          id: item.id,
          url: item.url,
          format: resolvedOptions,
          subtitlesOnly: options.subtitlesOnly ?? false,
          thumbnailOnly: options.thumbnailOnly ?? false,
          subtitleInventory: item.subtitleInventory,
          overrides,
          templateContext: {
            values: buildTemplateContext(item, group, overrides),
          },
        })),
      });
      pendingSubmissions.set(groupId, submission);
      try {
        await submission;
      } finally {
        if (pendingSubmissions.get(groupId) === submission) pendingSubmissions.delete(groupId);
      }
    } catch (e) {
      if (pendingPauses.has(groupId) || !groupStore.findGroupById(groupId)) return;
      for (const item of Object.values(group.items)) {
        if (stateStore.getState(item.id) !== MediaState.done) stateStore.setState(item.id, MediaState.error);
      }
      stateStore.setGroupState(groupId, MediaState.error);
      diagnosticsStore.processMediaFatalPayload({
        groupId,
        id: groupId,
        exitCode: 1,
        internal: true,
        message: `${e}`,
        details: null,
        timestamp: Date.now(),
      });
      console.error(e);
    }
  }

  async function pauseGroup(groupId: string) {
    if (pendingPauses.has(groupId)) return;
    const group = groupStore.findGroupById(groupId);
    if (!group) return;

    const items: MediaItem[] = Object.values(group.items);
    const itemsWithoutLeader: MediaItem[] = items.filter(item => !item.entries);

    if (itemsWithoutLeader.length === 0) return;

    const newState = group.isCombined ? MediaState.pausedList : MediaState.paused;
    items
      .filter(item => stateStore.getState(item.id) !== MediaState.done)
      .forEach(item => stateStore.setState(item.id, newState));

    const attemptId = groupStore.getDownloadAttempt(groupId);
    const submission = pendingSubmissions.get(groupId);
    const pause = (async () => {
      if (submission) await submission.catch(() => undefined);
      const receipt = await groupStore.cancelGroup(groupId, true);
      if (!receipt || !groupStore.findGroupById(groupId)) return;
      for (const { id, destination } of receipt.completed) {
        destinationStore.addDestination(groupId, { path: destination, confidence: 100 });
        progressStore.processMediaCompletePayload({ groupId, id });
      }
      for (const { id, message, publishedDestinations } of receipt.failed) {
        for (const destination of publishedDestinations) destinationStore.addDestination(groupId, { path: destination, confidence: 100 });
        if (stateStore.getState(id) !== newState) continue;
        diagnosticsStore.processMediaFatalPayload({
          groupId,
          id,
          attemptId,
          exitCode: 1,
          internal: true,
          message,
          details: null,
          timestamp: Date.now(),
        });
      }
    })();
    pendingPauses.set(groupId, pause);
    try {
      await pause;
    } catch (error) {
      if (groupStore.findGroupById(groupId)) {
        const downloading = group.isCombined ? MediaState.downloadingList : MediaState.downloading;
        items
          .filter(item => stateStore.getState(item.id) === newState)
          .forEach(item => stateStore.setState(item.id, downloading));
      }
      console.error(error);
    } finally {
      pendingPauses.delete(groupId);
    }
  }

  function buildTemplateContext(
    item: MediaItem,
    group: Group,
    overrides?: DownloadOverrides,
  ): Record<string, string | undefined> {
    const reversePlaylistNumbering = overrides?.output?.reversePlaylistNumbering === true;
    const playlistIndex = resolvePlaylistIndex(item.playlistIndex, group.playlistCount, reversePlaylistNumbering);

    return {
      playlist_index: playlistIndex?.toString(),
      playlist_id: group.playlistId ?? undefined,
      playlist_title: group.playlistTitle ?? undefined,
      playlist: group.playlistTitle ?? undefined,
      playlist_uploader: group.uploader ?? undefined,
      playlist_uploader_id: group.uploaderId ?? undefined,
      playlist_count: group.playlistCount?.toString(),
      n_entries: group.entries?.length?.toString(),
    };
  }

  async function downloadAllGroups(fromShortcut: boolean = false) {
    watchClipboardStore.disable();
    const group_ids = groupStore.groupOrder
      .filter(gid => stateStore.getGroupState(gid) === MediaState.configure);

    await Promise.all(group_ids.map((gid) => {
      const opts = getDownloadOptions(gid);
      return downloadGroup(gid, opts);
    }));
    await notify(NotificationKind.QueueDownloading, { n: group_ids.length.toString() }, fromShortcut);
  }

  function pauseAllGroups() {
    for (const groupId of groupStore.groupOrder) {
      const state = stateStore.getGroupState(groupId);
      if (state === MediaState.downloading || state === MediaState.downloadingList) {
        void pauseGroup(groupId);
      }
    }
  }

  function resumeAllGroups() {
    for (const groupId of groupStore.groupOrder) {
      const state = stateStore.getGroupState(groupId);
      if (state !== MediaState.paused && state !== MediaState.pausedList) continue;
      void resumeGroup(groupId);
    }
  }

  async function resumeGroup(groupId: string) {
    const execution = executions.get(groupId);
    await downloadGroup(groupId, execution?.options ?? getDownloadOptions(groupId), execution);
  }

  async function retryGroup(groupId: string) {
    const group = groupStore.findGroupById(groupId);
    const execution = executions.get(groupId);
    const overrides = execution ? execution.overrides : optionsStore.getOverrides(groupId);
    const nextGroupId = await dispatchMediaInfoFetch(group.url, group.fromShortcut, !!execution, overrides ?? {});
    if (!execution) return;
    const readyGroupIds = await waitForGroupReady(nextGroupId);
    await Promise.all(readyGroupIds.map(id => downloadGroup(id, execution.options, {
      ...execution,
      overrides: optionsStore.getOverrides(id),
    })));
  }

  function deleteGroup(id: string) {
    const group = groupStore.findGroupById(id);
    for (const itemId of Object.keys(group.items)) {
      stateStore.removeState(itemId);
      progressStore.deleteProgress(itemId);
      sizeStore.removeSizes(itemId);
      diagnosticsStore.removeDiagnostics(itemId);
    }
    diagnosticsStore.removeDiagnostics(group.id);
    destinationStore.deleteDestinations(group.id);
    optionsStore.removeOptions(group.id);
    executions.delete(group.id);
    const pending = pendingPauses.get(id) ?? pendingSubmissions.get(id);
    groupStore.deleteGroup(id);
    const cancel = () => groupStore.cancelGroup(id);
    void (pending ? pending.then(cancel, cancel) : cancel()).catch(console.error);
  }

  function deleteAllGroups() {
    for (const groupId of [...groupStore.groupOrder]) {
      deleteGroup(groupId);
    }
  }

  function deleteGroupsByState(states: MediaState[]) {
    for (const groupId of [...groupStore.groupOrder]) {
      const state = stateStore.getGroupState(groupId);
      if (state && states.includes(state)) {
        deleteGroup(groupId);
      }
    }
  }

  return {
    processMediaAddPayload,
    finalizePlaylistGroup,
    dispatchMediaInfoFetch,
    expandPlaylistGroup,
    rejectPendingReadyGroup,
    addAndDownload,
    addUrlBatch,
    addUrlBatchAndDownload,
    downloadGroup,
    downloadSubtitles,
    downloadThumbnail,
    downloadAllGroups,
    pauseAllGroups,
    pauseGroup,
    resumeAllGroups,
    resumeGroup,
    retryGroup,
    deleteGroup,
    deleteAllGroups,
    deleteGroupsByState,
  };
});
