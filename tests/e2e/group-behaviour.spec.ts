import { expect, Page } from '@playwright/test';
import { test } from './utils/fixtures';

async function addPlaylistAndApply(page: Page, url: string) {
  await page.goto('/');
  await page.fill('input[type="text"]', url);
  await page.getByRole('button', { name: 'Add', exact: true }).click();
  await page.getByRole('button', { name: 'Download all items' }).click();
}

test('it splits small playlists into multiple groups', async ({ page }) => {
  await addPlaylistAndApply(page, 'https://example.com/playlist-small');
  const cards = page.locator('article.card');
  await expect(cards).toHaveCount(2);
  await expect(page.getByText('Playlist')).toHaveCount(0);
});

test('all card actions stay inside the card at compact window sizes', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 544 });
  await addPlaylistAndApply(page, 'https://example.com/playlist-small');
  for (const card of await page.locator('article.card').all()) {
    const bounds = await card.boundingBox();
    expect(bounds).not.toBeNull();
    const actions = card.locator('aside').getByRole('button').or(card.locator('aside').getByRole('link'));
    await expect(actions).toHaveCount(6);
    for (const action of await actions.all()) {
      const actionBounds = await action.boundingBox();
      expect(actionBounds).not.toBeNull();
      expect(actionBounds!.x).toBeGreaterThanOrEqual(bounds!.x);
      expect(actionBounds!.y).toBeGreaterThanOrEqual(bounds!.y);
      expect(actionBounds!.y + actionBounds!.height).toBeLessThanOrEqual(bounds!.y + bounds!.height);
      expect(actionBounds!.x + actionBounds!.width).toBeLessThanOrEqual(bounds!.x + bounds!.width);
    }
  }
});

test('it consolidates large playlists into a single group', async ({ page }) => {
  await addPlaylistAndApply(page, 'https://example.com/playlist-large');
  const cards = page.locator('article.card');
  await expect(cards).toHaveCount(1);
  await expect(page.getByText('Playlist')).toBeVisible();
});
