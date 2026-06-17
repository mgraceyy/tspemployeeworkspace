import { test, expect } from "@playwright/test";

const EMPLOYEE_CODE = "E2E001";
const EMPLOYEE_PIN = "482915";
const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";

test.describe("authorization boundaries", () => {
  test("employee cannot access admin reports", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
    await page.fill('input[name="pin"]', EMPLOYEE_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    const response = await page.goto("/admin/reports");
    expect(response?.status()).toBe(403);
    await expect(page.getByText(/forbidden/i)).toBeVisible();
  });

  test("employee cannot access manager dashboard", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
    await page.fill('input[name="pin"]', EMPLOYEE_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    const response = await page.goto("/manager");
    expect(response?.status()).toBe(403);
    await expect(page.getByText(/forbidden/i)).toBeVisible();
  });

  test("unauthenticated user is redirected from home", async ({ page }) => {
    await page.goto("/");
    await expect(page).toHaveURL(/\/login/);
  });

  test("closed pay period blocks employee EOD edits", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', ADMIN_CODE);
    await page.fill('input[name="pin"]', ADMIN_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();
    await expect(page).toHaveURL("/");

    await page.goto("/admin/reports");
    const closeButton = page.getByRole("button", { name: /close this period/i });
    if (await closeButton.isVisible()) {
      await closeButton.click();
      await expect(page.getByText(/pay period closed|time edits in this range are now blocked/i)).toBeVisible();
    }

    await page.goto("/logout");

    await page.goto("/login");
    await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
    await page.fill('input[name="pin"]', EMPLOYEE_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();
    await expect(page).toHaveURL("/");

    await page.goto("/me/eod");
    const completed = page.locator('textarea[name="completed"]');
    if (await completed.isVisible()) {
      page.once("dialog", (dialog) => dialog.accept());
      await completed.fill("Blocked by closed period");
      await page.getByRole("button", { name: /submit eod/i }).click();
      await expect(page.getByText(/closed pay period/i)).toBeVisible();
    } else {
      const clockOut = page.getByRole("button", { name: /clock out/i });
      if (await clockOut.isVisible()) {
        await clockOut.click();
        await expect(page.getByText(/closed pay period/i)).toBeVisible();
      }
    }
  });
});