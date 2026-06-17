import { test, expect } from "@playwright/test";

const E2E_EMPLOYEE_CODE = "E2E001";
const E2E_EMPLOYEE_PIN = "482915";
const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";

test.describe("authenticated flows", () => {
  test("employee can log in and clock in", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', E2E_EMPLOYEE_CODE);
    await page.fill('input[name="pin"]', E2E_EMPLOYEE_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await expect(page).toHaveURL("/");
    await expect(page.getByRole("heading", { name: /clock in \/ out/i })).toBeVisible();

    const clockIn = page.getByRole("button", { name: /clock in/i });
    if (await clockIn.isVisible()) {
      await clockIn.click();
      await expect(page.getByText(/clocked in|clock out/i)).toBeVisible();
    }
  });

  test("employee leave page loads after login", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', E2E_EMPLOYEE_CODE);
    await page.fill('input[name="pin"]', E2E_EMPLOYEE_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await page.goto("/me/leave");
    await expect(page.getByRole("heading", { name: /leave requests/i })).toBeVisible();
    await expect(page.locator('select[name="leave_type"]')).toBeVisible();
  });

  test("admin can open reports", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', ADMIN_CODE);
    await page.fill('input[name="pin"]', ADMIN_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await expect(page).toHaveURL("/");
    await page.goto("/admin/reports");
    await expect(page.getByRole("heading", { name: /payroll reports/i })).toBeVisible();
    await expect(page.getByRole("button", { name: /close this period/i })).toBeVisible();
  });
});