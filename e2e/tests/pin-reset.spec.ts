import { test, expect } from "@playwright/test";

const EMPLOYEE_CODE = "E2E001";
const EMPLOYEE_PIN = "482915";
const MANAGER_CODE = "E2MGR";
const MANAGER_PIN = "482915";
const TEMP_PIN = "887766";

async function loginAs(page: import("@playwright/test").Page, code: string, pin: string) {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', code);
  await page.fill('input[name="pin"]', pin);
  await page.getByRole("button", { name: /sign in/i }).click();
}

async function logout(page: import("@playwright/test").Page) {
  await page.locator('form[action="/logout"] button').click();
  await expect(page).toHaveURL(/\/login/);
}

async function cancelPendingPinResetIfAny(page: import("@playwright/test").Page) {
  const cancel = page.getByRole("button", { name: /cancel pin reset request/i });
  if (await cancel.isVisible()) {
    await cancel.click();
    await expect(page.getByText(/cancelled/i)).toBeVisible();
  }
}

test.describe("PIN reset", () => {
  test("employee requests reset, manager approves, employee sets new PIN", async ({ page }) => {
    await loginAs(page, EMPLOYEE_CODE, EMPLOYEE_PIN);
    await expect(page).toHaveURL("/");

    await page.goto("/me/profile");
    await expect(page.getByRole("heading", { name: /my profile/i })).toBeVisible();
    await cancelPendingPinResetIfAny(page);

    await page.fill('textarea[name="reason"]', "E2E forgot PIN");
    await page.getByRole("button", { name: /request pin reset/i }).click();
    await expect(page.getByText(/pin reset request submitted/i)).toBeVisible();

    await logout(page);

    await loginAs(page, MANAGER_CODE, MANAGER_PIN);
    await page.goto("/manager/pin-resets");
    await expect(page.getByRole("heading", { name: /pin reset requests/i })).toBeVisible();
    await expect(page.getByText(EMPLOYEE_CODE)).toBeVisible();

    const card = page.locator("article.panel", { hasText: EMPLOYEE_CODE });
    await card.locator('input[name="temp_pin"]').fill(TEMP_PIN);
    await card.getByRole("button", { name: /approve.*set pin/i }).click();
    await expect(page.getByText(/pin reset approved/i)).toBeVisible();

    await logout(page);

    await loginAs(page, EMPLOYEE_CODE, TEMP_PIN);
    await expect(page).toHaveURL("/change-pin");
    await page.fill('input[name="new_pin"]', EMPLOYEE_PIN);
    await page.fill('input[name="confirm_pin"]', EMPLOYEE_PIN);
    await page.getByRole("button", { name: /save pin/i }).click();
    await expect(page).toHaveURL("/");

    await logout(page);
    await loginAs(page, EMPLOYEE_CODE, EMPLOYEE_PIN);
    await expect(page).toHaveURL("/");
  });

  test("unauthenticated employee can submit PIN reset from login page", async ({ page }) => {
    await page.goto("/login/request-pin-reset");
    await expect(page.getByRole("heading", { name: /request pin reset/i })).toBeVisible();

    await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
    await page.fill('textarea[name="reason"]', "E2E login-page reset request");
    await page.getByRole("button", { name: /submit request/i }).click();

    await expect(
      page.getByText(/if your employee code is valid, your manager or admin will review/i)
    ).toBeVisible();
  });
});