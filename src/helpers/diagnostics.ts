export function isErrorForFailure(diagnostic: MediaDiagnostic, fatal: MediaFatal): boolean {
  return diagnostic.level === 'error' && diagnostic.groupId === fatal.groupId
    && diagnostic.id === fatal.id && diagnostic.timestamp <= fatal.timestamp;
}

export function firstSentence(text: string): string {
  if (!text) return '';
  const match = text.match(/^(.*?[.!?])(\s|$)/);
  return match ? match[1].trim() : text.trim();
}
