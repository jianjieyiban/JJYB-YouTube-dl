import { expect } from '@playwright/test';
import { test } from './utils/fixtures';

test.describe('when it has an update available', () => {
  test.use({
    mockData: [{
      settings: {
        appearance: { language: 'en' },
        update: { updateApp: true },
      },
      updater: {
        configured: true,
        available: true,
        availableVersion: '1.2.3',
        currentVersion: '1.2.2',
      },
    }, { scope: 'test' }],
  });

  test('it shows the alert', async ({ page }) => {
    await page.goto('/');
    const updateAlert = page.getByTestId('updater-alert');
    await expect(updateAlert).toBeVisible();
  });

  test('it hides the alert when the update gets ignored', async ({ page }) => {
    await page.goto('/');
    const updateAlert = page.getByTestId('updater-alert');
    await expect(updateAlert).toBeVisible();
    await updateAlert.getByText('Later').click();
    await expect(updateAlert).not.toBeVisible();
  });

  test('one click downloads, verifies and installs without hiding the restart status', async ({ page }) => {
    await page.clock.install({ time: new Date('2025-09-15T21:30:00') });

    await page.goto('/');
    const updateAlert = page.getByTestId('updater-alert');

    await page.clock.pauseAt(new Date('2025-09-15T21:45:00'));
    await updateAlert.getByRole('button', { name: 'Update and restart' }).click();
    await expect(updateAlert).toHaveAttribute('aria-busy', 'true');
    await expect(updateAlert.getByText('Downloading update…')).toBeVisible();
    await expect(updateAlert.getByRole('button')).toHaveCount(0);
    await page.clock.runFor(1000);

    await expect(updateAlert.getByText('50%')).toBeVisible();
    await expect(updateAlert.getByRole('progressbar')).toHaveAttribute('value', '50');

    await page.clock.runFor(1000);

    await expect(updateAlert.getByText('100%')).toBeVisible();
    await expect(updateAlert.getByText('Verifying signature and file hash…')).toBeVisible();
    expect(await page.evaluate(() => window.E2E.updater?.installCalls ?? 0)).toBe(0);

    await page.clock.runFor(1000);

    await expect(updateAlert.getByText('Preparing to restart…')).toBeVisible();
    expect(await page.evaluate(() => window.E2E.updater?.installCalls)).toBe(1);
    await page.clock.runFor(5000);
    await expect(updateAlert).toBeVisible();
    await expect(updateAlert).toHaveAttribute('aria-busy', 'true');
    await expect(updateAlert.getByRole('button')).toHaveCount(0);
  });

  test('rapid repeated clicks start only one download and install', async ({ page }) => {
    await page.clock.install({ time: new Date('2025-09-15T21:30:00') });
    await page.goto('/');
    const updateAlert = page.getByTestId('updater-alert');
    await page.clock.pauseAt(new Date('2025-09-15T21:45:00'));

    await updateAlert.getByRole('button', { name: 'Update and restart' }).evaluate((button: HTMLButtonElement) => {
      button.click();
      button.click();
      button.click();
    });
    await expect(updateAlert.getByText('Downloading update…')).toBeVisible();
    expect(await page.evaluate(() => window.E2E.updater?.downloadCalls)).toBe(1);
    await page.clock.runFor(3000);
    await expect(updateAlert.getByText('Preparing to restart…')).toBeVisible();
    expect(await page.evaluate(() => window.E2E.updater?.downloadCalls)).toBe(1);
    expect(await page.evaluate(() => window.E2E.updater?.installCalls)).toBe(1);
  });

  for (const stage of ['download', 'install'] as const) {
    test(`it keeps the ${stage} error visible and offers retry`, async ({ page }) => {
      await page.clock.install({ time: new Date('2025-09-15T21:30:00') });
      await page.goto('/');
      const updateAlert = page.getByTestId('updater-alert');
      await page.evaluate((stage) => {
        window.E2E.updater![`${stage}Error`] = `${stage} failed in updater`;
      }, stage);
      await page.clock.pauseAt(new Date('2025-09-15T21:45:00'));
      await updateAlert.getByRole('button', { name: 'Update and restart' }).click();
      await page.clock.runFor(3000);

      await expect(updateAlert.getByText(`${stage} failed in updater`, { exact: true })).toBeVisible();
      await expect(updateAlert).toHaveAttribute('aria-busy', 'false');
      await expect(updateAlert.getByRole('button', { name: 'Update and restart' })).toHaveCount(0);
      await expect(updateAlert.getByRole('button', { name: 'Check for updates' })).toBeVisible();
      expect(await page.evaluate(() => window.E2E.updater?.installCalls ?? 0)).toBe(stage === 'download' ? 0 : 1);
      await page.clock.runFor(5000);
      await expect(updateAlert.getByText(`${stage} failed in updater`, { exact: true })).toBeVisible();
      await expect(updateAlert.getByRole('progressbar')).toHaveCount(0);
    });
  }
});

test.describe('when application updates are not configured', () => {
  test.use({
    mockData: [{
      settings: {
        appearance: { language: 'en' },
        update: { updateApp: true },
      },
      updater: { configured: false, available: true },
    }, { scope: 'test' }],
  });

  test('it shows no update alert even when availability is stale', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByTestId('updater-alert')).not.toBeVisible();
  });
});

test.describe('when it has no update available', () => {
  test.use({
    mockData: [{
      settings: {
        appearance: { language: 'en' },
        update: { updateApp: true },
      },
      updater: { configured: true, available: false },
    }, { scope: 'test' }],
  });

  test('it shows no alert', async ({ page }) => {
    await page.goto('/');
    const updateAlert = page.getByTestId('updater-alert');
    await expect(updateAlert).not.toBeVisible();
  });
});
