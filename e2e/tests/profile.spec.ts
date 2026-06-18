import { test, expect } from "@playwright/test";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";

const EMPLOYEE_CODE = "E2E001";
const EMPLOYEE_PIN = "482915";

// Valid 1×1 PNG
const MINI_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64"
);

test("employee can upload a profile photo", async ({ page }) => {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
  await page.fill('input[name="pin"]', EMPLOYEE_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");

  await page.goto("/me/profile");
  await expect(page.getByRole("heading", { name: /my profile/i })).toBeVisible();

  const tmpFile = path.join(os.tmpdir(), `dtr-e2e-photo-${Date.now()}.png`);
  fs.writeFileSync(tmpFile, MINI_PNG);

  await page.locator('input[name="photo"]').setInputFiles(tmpFile);
  await page.getByRole("button", { name: /upload photo/i }).click();

  await expect(page.getByText(/profile photo updated/i)).toBeVisible();
  await expect(page.locator('img.profile-photo[src="/me/profile/photo"]')).toBeVisible();

  const photoResponse = await page.request.get("/me/profile/photo");
  expect(photoResponse.ok()).toBeTruthy();
  expect(photoResponse.headers()["content-type"]).toMatch(/image\//);

  fs.unlinkSync(tmpFile);
});

test("logout everywhere invalidates the current session", async ({ page }) => {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
  await page.fill('input[name="pin"]', EMPLOYEE_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");

  page.once("dialog", (dialog) => dialog.accept());

  await page.goto("/me/profile");
  await page.getByRole("button", { name: /log out everywhere/i }).click();
  await expect(page).toHaveURL(/\/login/);

  await page.goto("/");
  await expect(page).toHaveURL(/\/login/);
});