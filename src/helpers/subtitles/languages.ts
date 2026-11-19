import ISO6391 from 'iso-639-1';
import type { MediaItem } from '../../tauri/types/media';

export function getAvailableSubtitleLanguages(items: Record<string, MediaItem>) {
  const manual = new Set<string>();
  const auto = new Set<string>();
  for (const item of Object.values(items)) {
    if (item.entries) continue;
    item.subtitleInventory?.manualLanguages?.forEach(language => manual.add(language));
    item.subtitleInventory?.autoLanguages?.forEach(language => auto.add(language));
  }
  return {
    manual: Array.from(manual).sort(),
    auto: getPreferredAutoSubtitleLanguages(Array.from(auto)),
  };
}

export interface SubtitleLanguageOption {
  code: string;
  englishName: string;
  nativeName: string;
}

const codes = ISO6391.getAllCodes();

const expandedCodes: Record<string, SubtitleLanguageOption> = {
  'pt-PT': { code: 'pt-PT', englishName: 'Portuguese (Portugal)', nativeName: 'Português (Portugal)' },
  'pt-BR': { code: 'pt-BR', englishName: 'Portuguese (Brazil)', nativeName: 'Português (Brasil)' },
  'zh-TW': { code: 'zh-TW', englishName: 'Traditional Chinese (Taiwan)', nativeName: '繁體中文（台灣）' },
  'zh-CN': { code: 'zh-CN', englishName: 'Simplified Chinese', nativeName: '简体中文' },
};

export const languageOptions: SubtitleLanguageOption[] = [...codes, ...Object.keys(expandedCodes)]
  .map((code) => {
    if (expandedCodes[code]) return expandedCodes[code];

    return {
      code,
      englishName: ISO6391.getName(code),
      nativeName: ISO6391.getNativeName(code),
    };
  })
  .sort((a, b) => a.englishName.localeCompare(b.englishName));

export const languageOptionsLookup = new Map(
  languageOptions.map(option => [option.code, option] as const),
);

function normalizeLocale(candidate: string): string {
  const parts = candidate.replace(/_/g, '-').split('-');
  const language = parts[0]?.toLowerCase() ?? '';
  const region = parts[1]?.toUpperCase();

  return region ? `${language}-${region}` : language;
}

export function getSubtitleLanguageLabel(code: string, locale = 'en'): string {
  const trimmed = code.trim();
  if (!trimmed) {
    return code;
  }

  const normalized = trimmed.replace(/_/g, '-');
  const lower = normalized.toLowerCase();
  const isOrig = lower.endsWith('-orig');
  const baseRaw = isOrig ? normalized.slice(0, -5) : normalized;
  const base = normalizeLocale(baseRaw);

  const exact = languageOptionsLookup.get(base)
    || languageOptionsLookup.get(base.toLowerCase());

  const isSimplifiedChinese = locale === 'zh-CN';
  const label = isSimplifiedChinese
    ? new Intl.DisplayNames(['zh-CN'], { type: 'language' }).of(base) ?? base.toUpperCase()
    : exact?.englishName ?? base.toUpperCase();

  return isOrig
    ? isSimplifiedChinese ? `${label}（原始音频）` : `${label} (Original audio)`
    : label;
}

export function getPreferredAutoSubtitleLanguages(codes: string[]): string[] {
  return [...new Set(codes.map(code => code.trim()).filter(Boolean))].sort();
}

export function detectBrowserLanguageCodes(): string[] {
  let candidates: readonly string[] = [];

  if (typeof navigator !== 'undefined') {
    candidates = navigator.languages?.length
      ? navigator.languages
      : navigator.language
        ? [navigator.language]
        : [];
  }

  const normalized = new Set<string>();

  for (const candidate of candidates) {
    if (!candidate) continue;

    const locale = normalizeLocale(candidate);

    if (expandedCodes[locale]) {
      normalized.add(locale);
    }

    const isoCode = locale.split('-')[0];
    if (isoCode && ISO6391.validate(isoCode)) {
      normalized.add(isoCode);
    }
  }

  return Array.from(normalized);
}
