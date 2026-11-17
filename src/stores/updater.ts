import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { UpdaterStatusPayload, UpdaterCheckPayload, UpdaterDownloadProgressPayload, UpdaterPhase } from '../tauri/types/updater';
import { useSettingsStore } from './settings';

export const useUpdaterStore = defineStore('updater', () => {
  const checkResult = ref<UpdaterCheckPayload | undefined>();
  const downloadProgress = ref<UpdaterDownloadProgressPayload>({ received: 0, total: 0 });
  const isUpdating = ref(false);
  const isIgnored = ref(false);
  const isNeedingRestart = ref(false);
  const lastError = ref<string | undefined>();
  const phase = ref<UpdaterPhase>('idle');
  const settingsStore = useSettingsStore();
  const configuration = ref<UpdaterStatusPayload>();
  let running = false;

  function clearPending() {
    checkResult.value = undefined;
    isNeedingRestart.value = false;
    downloadProgress.value = { received: 0, total: 0 };
  }

  async function readConfiguration() {
    try {
      configuration.value = await invoke<UpdaterStatusPayload>('updater_status');
      if (!configuration.value.configured) clearPending();
    } catch (error) {
      configuration.value = undefined;
      processUpdaterError(String(error));
    }
  }

  async function loadConfiguration() {
    if (running || isUpdating.value) return;
    await readConfiguration();
  }

  async function performCheck(manual: boolean) {
    clearPending();
    lastError.value = undefined;
    phase.value = 'checking';
    await readConfiguration();
    if (!configuration.value?.configured || (!manual && !settingsStore.settings.update.updateApp)) {
      if (!lastError.value) phase.value = 'idle';
      return;
    }
    try {
      checkResult.value = await invoke<UpdaterCheckPayload>('updater_check');
      configuration.value = checkResult.value;
      isIgnored.value = false;
      phase.value = 'idle';
    } catch (error) {
      processUpdaterError(String(error));
    }
  }

  async function check(manual = false) {
    if (running || isUpdating.value) return;
    running = true;
    isUpdating.value = true;
    try {
      await performCheck(manual);
    } finally {
      running = false;
      isUpdating.value = false;
    }
  }

  async function performDownload() {
    isNeedingRestart.value = false;
    lastError.value = undefined;
    downloadProgress.value = { received: 0, total: 0 };
    phase.value = 'downloading';
    try {
      await invoke('updater_download');
      if (lastError.value) return false;
      isNeedingRestart.value = true;
      phase.value = 'ready';
      return true;
    } catch (error) {
      processUpdaterError(String(error));
      return false;
    }
  }

  async function download() {
    if (running || isUpdating.value) return;
    running = true;
    isUpdating.value = true;
    try {
      await performDownload();
    } finally {
      running = false;
      isUpdating.value = false;
    }
  }

  async function performInstall() {
    isNeedingRestart.value = false;
    lastError.value = undefined;
    phase.value = 'preparing-restart';
    try {
      await invoke('updater_install');
    } catch (error) {
      processUpdaterError(String(error));
    }
  }

  async function install() {
    if (running || isUpdating.value || !isNeedingRestart.value) return;
    running = true;
    isUpdating.value = true;
    try {
      await performInstall();
    } finally {
      running = false;
      isUpdating.value = phase.value === 'preparing-restart';
    }
  }

  async function updateAndRestart() {
    if (running || isUpdating.value) return;
    running = true;
    isUpdating.value = true;
    isIgnored.value = false;
    try {
      if (!checkResult.value?.available) await performCheck(true);
      if (!checkResult.value?.available || lastError.value) return;
      if (await performDownload()) await performInstall();
    } finally {
      running = false;
      isUpdating.value = phase.value === 'preparing-restart';
    }
  }

  function ignore() {
    if (!isUpdating.value) isIgnored.value = true;
  }

  function processUpdaterDownloadProgress(payload: UpdaterDownloadProgressPayload) {
    if (phase.value !== 'downloading' && phase.value !== 'verifying') return;
    Object.assign(downloadProgress.value, payload);
    if (payload.total && payload.received >= payload.total) phase.value = 'verifying';
  }

  function processUpdaterFinished() {
    // Completion is not installation proof: wait for the install IPC and process exit.
    if (phase.value !== 'preparing-restart') return;
  }

  function processUpdaterError(payload: string) {
    clearPending();
    isIgnored.value = false;
    lastError.value = payload;
    phase.value = 'error';
  }

  return { configuration, loadConfiguration, checkResult, downloadProgress, isUpdating, isIgnored, isNeedingRestart, lastError, phase, ignore, check, download, install, updateAndRestart, processUpdaterDownloadProgress, processUpdaterFinished, processUpdaterError };
});
