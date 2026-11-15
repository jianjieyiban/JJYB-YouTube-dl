import { describe, expect, it } from 'vitest';
import {
  getPreferredAutoSubtitleLanguages,
  getSubtitleLanguageLabel,
  languageOptions,
} from '../../src/helpers/subtitles/languages';
import {
  DEFAULT_SUBTITLE_FORMAT_ORDER,
  sanitizeSubtitleFormats,
  sanitizeSubtitleLanguages,
} from '../../src/helpers/subtitles/sanitize';

describe('subtitle helpers', () => {
  it('provides a comprehensive ISO-639-1 language list', () => {
    expect(languageOptions.length).toBeGreaterThan(150);

    const english = languageOptions.find(option => option.code === 'en');
    expect(english?.englishName).toBe('English');
    expect(english?.nativeName.length).toBeGreaterThan(0);

    const uniqueCodes = new Set(languageOptions.map(option => option.code));
    expect(uniqueCodes.size).toEqual(languageOptions.length);
  });

  it('formats orig subtitle language codes for display', () => {
    expect(getSubtitleLanguageLabel('nl-orig')).toBe('Dutch (Original audio)');
  });

  it('displays subtitle languages and original audio in Simplified Chinese', () => {
    expect(getSubtitleLanguageLabel('en', 'zh-CN')).toBe('英语');
    expect(getSubtitleLanguageLabel('en-orig', 'zh-CN')).toBe('英语（原始音频）');
  });

  it('preserves distinct plain and original auto subtitle track IDs', () => {
    expect(getPreferredAutoSubtitleLanguages(['en-orig', 'en', 'nl-orig'])).toEqual(['en', 'en-orig', 'nl-orig']);
  });

  it('keeps platform translation candidates and original track IDs', () => {
    expect(getPreferredAutoSubtitleLanguages(['en', 'en-orig', 'de', 'fr'])).toEqual(['de', 'en', 'en-orig', 'fr']);
  });

  it('sanitizes subtitle formats with sensible fallbacks', () => {
    expect(sanitizeSubtitleFormats(['ass'])).toEqual(['ass', ...DEFAULT_SUBTITLE_FORMAT_ORDER.filter(format => format !== 'ass')]);
    expect(sanitizeSubtitleFormats(['srt', 'SRT', ''])).toEqual([...DEFAULT_SUBTITLE_FORMAT_ORDER]);
    expect(sanitizeSubtitleFormats(['json', 'json3'])).toEqual(['json3', 'srt', 'vtt', 'ass', 'ttml']);
    expect(sanitizeSubtitleFormats([])).toEqual([...DEFAULT_SUBTITLE_FORMAT_ORDER]);
  });

  it('sanitizes subtitle languages and respects the all option', () => {
    expect(sanitizeSubtitleLanguages(['en', 'EN', ''])).toEqual(['en']);
    expect(sanitizeSubtitleLanguages(['all', 'en'])).toEqual(['all']);
    expect(sanitizeSubtitleLanguages([])).toEqual([]);
  });
});
