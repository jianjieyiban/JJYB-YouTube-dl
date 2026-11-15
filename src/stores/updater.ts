import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { UpdaterStatusPayload, UpdaterCheckPayload, UpdaterDownloadProgressPayload } from '../tauri/types/updater';
import { useSettingsStore } from './settings';

export const useUpdaterStore = defineStore('updater', () => {
  const checkResult = ref<UpdaterCheckPayload | undefined>();
  const downloadProgress = ref<UpdaterDownloadProgressPayload>({ received: 0, total: 0 });
  const isUpdating = ref(false);
  const isIgnored = ref(false);
  const isNeedingRestart = ref(false);
  const lastError = ref<string | undefined>();
  const settingsStore = useSettingsStore();

  const configuration = ref<UpdaterStatusPayload>();

  function clearPending() {
    checkResult.value = undefined;
    isNeedingRestart.value = false;
    downloadProgress.value = { received: 0, total: 0 };
  }

  async function loadConfiguration() {
    try {
      configuration.value = await invoke<UpdaterStatusPayload>('updater_status');
      if (!configuration.value.configured) clearPending();
    } catch (error) {
      configuration.value = undefined;
      clearPending();
      lastError.value = String(error);
    }
  }

  async function check() {
    clearPending();
    lastError.value = undefined;
    await loadConfiguration();
    if (!configuration.value?.configured || !settingsStore.settings.update.updateApp) return;
    try {
      checkResult.value = await invoke<UpdaterCheckPayload>('updater_check');
      isIgnored.value = false;
    } catch (error) {
      clearPending();
      lastError.value = String(error);
    }
  }

  async function download() {
    isNeedingRestart.value = false;
    lastError.value = undefined;
    downloadProgress.value = { received: 0, total: 0 };
    try {
      isUpdating.value = true;
      await invoke('updater_download');
      isNeedingRestart.value = true;
    } catch (e) {
      clearPending();
      lastError.value = String(e);
    } finally {
      isUpdating.value = false;
    }
  }

  async function install() {
    isNeedingRestart.value = false;
    try {
      await invoke('updater_install');
    } catch (e) {
      clearPending();
      lastError.value = String(e);
    }
  }

  function ignore() {
    isIgnored.value = true;
  }

  function processUpdaterDownloadProgress(payload: UpdaterDownloadProgressPayload) {
    Object.assign(downloadProgress.value, payload);
  }

  function processUpdaterFinished() {
    isUpdating.value = false;
    isIgnored.value = true;
    isNeedingRestart.value = false;
    downloadProgress.value = { received: 0, total: 0 };
  }

  function processUpdaterError(payload: string) {
    processUpdaterFinished();
    lastError.value = payload;
  }

  return { configuration, loadConfiguration, checkResult, downloadProgress, isUpdating, isIgnored, isNeedingRestart, lastError, ignore, check, download, install, processUpdaterDownloadProgress, processUpdaterFinished, processUpdaterError };
});
