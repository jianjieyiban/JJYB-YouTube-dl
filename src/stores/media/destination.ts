import { ref } from 'vue';
import { defineStore } from 'pinia';
import { MediaDestinationPayload } from '../../tauri/types/destination';
import { useMediaGroupStore } from './group';

export type MediaDestination = { path: string; confidence: number };

export const useMediaDestinationStore = defineStore('media-destination', () => {
  const destinations = ref<Record<string, MediaDestination[]>>({});
  const groupStore = useMediaGroupStore();

  const addDestination = (groupId: string, destination: MediaDestination) => {
    const paths = destinations.value[groupId] ??= [];
    const existing = paths.find(path => path.path === destination.path);
    if (existing) existing.confidence = Math.max(existing.confidence, destination.confidence);
    else paths.push(destination);
  };

  const getDestinations = (groupId: string): MediaDestination[] => {
    return destinations.value[groupId] ?? [];
  };

  const getPrimaryDestination = (groupId: string): MediaDestination | undefined => {
    const destinations = getDestinations(groupId);
    destinations.sort((a, b) => b.confidence - a.confidence);
    return destinations[0];
  };

  const deleteDestinations = (groupId: string) => {
    delete destinations.value[groupId];
  };

  function processMediaDestinationPayload(payload: MediaDestinationPayload) {
    if (!groupStore.acceptsMediaEvent(payload)) return;
    addDestination(payload.groupId, payload.destination);
  }

  return {
    destinations,
    addDestination,
    getDestinations,
    getPrimaryDestination,
    deleteDestinations,
    processMediaDestinationPayload,
  };
});
