import { expect } from '@playwright/test';
import { test } from './utils/fixtures';

test('shows media_info IPC rejection without leaving the URL fetching', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  await page.goto('/');
  await page.evaluate(() => {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__: { invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> };
    }).__TAURI_INTERNALS__;
    const invoke = internals.invoke;
    internals.invoke = (cmd, args) => {
      if (cmd === 'media_info' && args?.url === 'https://example.com/ipc-rejected') {
        return Promise.reject('Updater lock busy');
      }
      return invoke(cmd, args);
    };
  });

  await page.fill('input[type="text"]', 'https://example.com/ipc-rejected');
  await page.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.getByText('Updater lock busy', { exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: 'View logs' })).toBeVisible();
  expect(pageErrors).toEqual([]);

  await page.fill('input[type="text"]', 'https://example.com/video');
  await page.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.getByText('Test Video')).toBeVisible();
  expect(pageErrors).toEqual([]);
});

test('it can have a new video added', async ({ page }) => {
  await page.goto('/');
  await page.fill('input[type="text"]', 'https://example.com/video');
  await page.getByRole('button', { name: 'Add', exact: true }).click();
  await expect(page.getByText('Test Video')).toBeVisible();
});
