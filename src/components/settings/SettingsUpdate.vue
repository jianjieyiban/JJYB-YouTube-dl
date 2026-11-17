<template>
  <base-fieldset
      :legend="t('settings.update.legend')"
      :label="t('settings.update.legendLabel')"
  >
    <label class="font-semibold mt-2" for="updateBinaries">
      {{ t('settings.update.updateBinaries.label') }}
    </label>
    <input
        id="updateBinaries"
        type="checkbox"
        v-model="settings.update.updateBinaries"
        class="toggle toggle-primary"
    />
    <p class="label">{{ t('settings.update.updateBinaries.hint') }}</p>
    <button type="button" class="btn btn-subtle" :disabled="checkingBinaries" @click="checkBinaryUpdates">
      {{ checkingBinaries ? t('common.pleaseWait') : '检查工具更新' }}
    </button>
    <p v-if="binaryUpdateMessage" role="status" class="label">{{ binaryUpdateMessage }}</p>
    <label class="font-semibold mt-2" for="updateApp">
      {{ t('settings.update.updateApp.label') }}
    </label>
    <input
        id="updateApp"
        type="checkbox"
        v-model="settings.update.updateApp"
        :disabled="!updater.configuration?.configured"
        class="toggle toggle-primary"
    />
    <p class="label">{{ t('settings.update.updateApp.hint') }}</p>
    <p v-if="!updater.configuration?.configured" class="label">
      {{ updater.configuration?.reason ?? updater.lastError ?? t('updater.loadingConfiguration') }}
      {{ t('updater.manualUpdate') }}
    </p>
    <div v-else class="flex flex-wrap items-center gap-2">
      <button class="btn btn-sm" :disabled="updater.isUpdating" @click="updater.check(true)">{{ t('updater.check') }}</button>
      <button v-if="updater.checkResult?.available || updater.lastError" class="btn btn-info btn-sm" :disabled="updater.isUpdating" @click="updater.updateAndRestart()">{{ t('updater.updateAndRestart') }}</button>
      <p role="status" aria-live="polite">
        <template v-if="updater.isUpdating">{{ t(`updater.${updater.phase}`) }}</template>
        <template v-else-if="updater.lastError">{{ t('updater.failed') }} {{ updater.lastError }}</template>
        <template v-else-if="updater.checkResult && !updater.checkResult.available">{{ t('updater.upToDate') }}</template>
      </p>
    </div>
    <a class="link" :href="updater.configuration?.releasesUrl ?? 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases'" target="_blank" rel="noopener noreferrer">{{ updater.configuration?.releasesUrl ?? 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases' }}</a>
  </base-fieldset>
</template>
<script setup lang="ts">
import BaseFieldset from '../base/BaseFieldset.vue';
import { Settings } from '../../tauri/types/config.ts';
import { useI18n } from 'vue-i18n';
import { onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useBinariesStore } from '../../stores/binaries';

import { useUpdaterStore } from '../../stores/updater';

const binariesStore = useBinariesStore();
const router = useRouter();
const checkingBinaries = ref(false);
const binaryUpdateMessage = ref('');
const checkBinaryUpdates = async () => {
  checkingBinaries.value = true;
  binaryUpdateMessage.value = '';
  try {
    const tools = await binariesStore.checkUpdates();
    if (tools.length > 0) {
      await router.push({ path: '/install', query: { mode: 'update' } });
    } else {
      binaryUpdateMessage.value = '工具已是最新版本';
    }
  } catch (error) {
    binaryUpdateMessage.value = `检查工具更新失败：${String(error)}`;
  } finally {
    checkingBinaries.value = false;
  }
};

const updater = useUpdaterStore();
onMounted(() => updater.loadConfiguration());
const { t } = useI18n();
const settings = defineModel<Settings>({ required: true });
</script>
