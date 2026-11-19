<template>
  <div class="card-body py-0 pr-0 grow w-full min-w-0 overflow-hidden">
    <h2
      :title="group.title ?? group.url"
      class="card-title w-full block shrink-0 leading-8 overflow-hidden whitespace-nowrap text-ellipsis text-base"
    >
      {{ group.title ?? group.url }}
    </h2>
    <base-progress
      :id="`${group.id}-progress`"
      :max="100"
      :value="100"
      variant="error"
      class="shrink-0"
    >
      {{ t('media.steps.error.errorPrefix', { message: diagnosticDisplay.shortMessage }) }}
    </base-progress>
    <p
      class="min-h-0 leading-5"
      :class="isOutputPathConflict ? 'shrink-0 truncate' : 'overflow-y-auto break-words'"
      :title="isOutputPathConflict ? diagnosticDisplay.message : undefined"
    >{{ diagnosticDisplay.message }}</p>
    <div class="w-full flex shrink-0 gap-4">
      <button @click="report" v-if="isReportable" :disabled="hasReported" class="btn btn-subtle">
        <template v-if="isReporting">
          <span class="sr-only">{{ t('common.loading') }}</span>
          <span class="loading loading-spinner loading-sm"></span>
        </template>
        <template v-else>
          <span>{{ t('media.steps.error.report') }}</span>
        </template>
      </button>
      <router-link :to="{ name: 'group.logs', params: { groupId: group.id } }" class="btn btn-subtle">{{ t('media.steps.error.showFull') }}</router-link>
      <router-link v-if="errorAction" :to="errorAction.to" class="btn btn-subtle">{{ errorAction.label }}</router-link>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, PropType } from 'vue';
import BaseProgress from '../../base/BaseProgress.vue';
import { Group } from '../../../tauri/types/group';
import { useI18n } from 'vue-i18n';
import { useMediaDiagnosticsStore } from '../../../stores/media/diagnostics.ts';
import { useDiagnostic } from '../../../composables/useDiagnostic.ts';
import { isErrorForFailure } from '../../../helpers/diagnostics.ts';

const i18n = useI18n();
const t = i18n.t;

const { group } = defineProps({
  group: {
    type: Object as PropType<Group>,
    required: true,
  },
});

const diagnosticsStore = useMediaDiagnosticsStore();

const lastFatal = computed(() => diagnosticsStore.findLastFatalByGroupId(group.id));
const lastDiagnostic = computed(() => {
  const fatal = lastFatal.value;
  if (!fatal) {
    const diagnostic = diagnosticsStore.findLastDiagnosticByGroupId(group.id);
    if (diagnostic?.level === 'error' && diagnostic.code !== 'outputPathConflict') return diagnostic;
    return;
  }
  if (fatal.internal || fatal.exitCode === 101) return;

  return [...diagnosticsStore.findDiagnosticsByGroupId(group.id)].reverse()
    .find(diagnostic => isErrorForFailure(diagnostic, fatal));
});

const error = computed(() => {
  let diagnostic: MediaDiagnostic = {
    id: 'unknown',
    code: 'unknown',
    groupId: group.id,
    raw: '',
    message: '',
    level: 'error',
    component: null,
    timestamp: 0,
  };

  if (!lastFatal.value && !lastDiagnostic.value) {
    return diagnostic;
  } else if (lastFatal.value) {
    if (lastFatal.value.internal) {
      diagnostic.id = lastFatal.value.id;
      diagnostic.message = lastFatal.value.message;
      diagnostic.code = lastFatal.value.code ?? 'unknown';
      diagnostic.component = 'runner';
      diagnostic.raw = lastFatal.value.details ?? '';
      diagnostic.level = 'error';
      diagnostic.timestamp = lastFatal.value.timestamp;
    } else if (lastDiagnostic.value) {
      diagnostic = lastDiagnostic.value;
    } else if (lastFatal.value.exitCode === 101) {
      diagnostic.id = lastFatal.value.id;
      diagnostic.message = lastFatal.value.message;
      diagnostic.code = 'rejectedByFilter';
      diagnostic.component = 'runner';
      diagnostic.raw = lastFatal.value.details ?? '';
      diagnostic.level = 'error';
      diagnostic.timestamp = lastFatal.value.timestamp;
    } else {
      diagnostic.id = lastFatal.value.id;
      diagnostic.message = lastFatal.value.message;
      diagnostic.raw = lastFatal.value.details ?? '';
      diagnostic.timestamp = lastFatal.value.timestamp;
    }
  } else if (lastDiagnostic.value) {
    diagnostic = lastDiagnostic.value;
  }

  return diagnostic;
});

const { diagnosticDisplay, report, isReportable, isReporting, hasReported } = useDiagnostic(error);
const isOutputPathConflict = computed(() => error.value.code === 'outputPathConflict');
const errorAction = computed(() => {
  if (error.value.code.includes('signIn')) {
    return {
      label: t('media.steps.error.signIn'),
      to: { name: 'authentication' as const },
    };
  }

  if (error.value.code === 'rejectedByFilter') {
    return {
      label: t('media.steps.error.filters'),
      to: { name: 'input-filters' as const },
    };
  }

  return null;
});

</script>
