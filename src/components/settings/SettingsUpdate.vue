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
      {{ updater.configuration?.reason ?? updater.lastError ?? '正在读取应用更新配置' }}
    </p>
    <a class="link" :href="updater.configuration?.releasesUrl ?? 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases'" target="_blank" rel="noopener noreferrer">{{ updater.configuration?.releasesUrl ?? 'https://github.com/jianjieyiban/JJYB-YouTube-dl/releases' }}</a>
  </base-fieldset>
</template>
<script setup lang="ts">
import BaseFieldset from '../base/BaseFieldset.vue';
import { Settings } from '../../tauri/types/config.ts';
import { useI18n } from 'vue-i18n';
import { onMounted } from 'vue';

import { useUpdaterStore } from '../../stores/updater';

const updater = useUpdaterStore();
onMounted(() => updater.loadConfiguration());
const { t } = useI18n();
const settings = defineModel<Settings>({ required: true });
</script>
