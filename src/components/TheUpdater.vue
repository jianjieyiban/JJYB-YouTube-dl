<template>
  <div class="fixed bottom-30 right-0 z-40 pointer-events-none">
    <Transition
        appear
        enter-active-class="transition duration-250 ease-out"
        enter-from-class="translate-y-4 opacity-0"
        enter-to-class="translate-y-0 opacity-100"
        leave-active-class="transition duration-200 ease-in"
        leave-from-class="translate-y-0 opacity-100"
        leave-to-class="translate-y-4 opacity-0"
    >
      <div
          v-if="!updater.isIgnored && (updater.checkResult?.available || updater.lastError || updater.isUpdating)"
          role="alert"
          data-testid="updater-alert"
          :aria-busy="updater.isUpdating"
          class="m-4 max-w-sm alert alert-horizontal alert-soft alert-info pointer-events-auto will-change-transform"
      >
        <arrow-path-icon class="w-6 h-6" :class="{ 'animate-spin': updater.isUpdating }" />
        <div class="w-full min-w-0">
          <h3 class="font-bold">{{ t(updater.isUpdating ? `updater.${updater.phase}` : updater.lastError ? 'updater.failed' : 'updater.available') }}</h3>
          <p v-if="updater.lastError" class="break-words">{{ updater.lastError }}</p>
          <template v-if="updater.phase === 'downloading' || updater.phase === 'verifying'">
            <progress class="progress progress-info w-full" :value="progress" max="100" :aria-label="t('updater.downloading')"></progress>
            <p v-if="progress !== undefined">{{ t('common.percentage', { percent: progress.toFixed() }) }}</p>
          </template>
          <p v-if="!updater.isUpdating && updater.checkResult?.available && !updater.lastError">{{ t('updater.install.subtitle') }}</p>
        </div>
        <template v-if="!updater.isUpdating">
          <button class="btn btn-sm" @click="updater.ignore()">{{ t('common.later') }}</button>
          <button v-if="updater.checkResult?.available && !updater.lastError" class="btn btn-info btn-sm" @click="updater.updateAndRestart()">{{ t('updater.updateAndRestart') }}</button>
          <button v-else class="btn btn-info btn-sm" @click="updater.check(true)">{{ t('updater.check') }}</button>
        </template>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import { ArrowPathIcon } from '@heroicons/vue/24/solid';
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useUpdaterStore } from '../stores/updater';

const updater = useUpdaterStore();
const { t } = useI18n();
const progress = computed(() => {
  const { received, total } = updater.downloadProgress;
  return total && total > 0 ? Math.min(100, Math.max(0, received / total * 100)) : undefined;
});
</script>
