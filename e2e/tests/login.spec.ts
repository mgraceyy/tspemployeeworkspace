import { test, expect } from "@playwright/test";

test("login page is reachable", async ({ page }) => {
  await page.goto("/login");
  await expect(page.locator('input[name="employee_code"]')).toBeVisible();
  await expect(page.locator('input[name="pin"]')).toBeVisible();
});

test("invalid credentials show an error", async ({ page }) => {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', "NOPE1234");
  await page.fill('input[name="pin"]', "000000");
  await page.getByRole("button", { name: /sign in|log in/i }).click();
  await expect(page.getByText(/invalid employee code or pin/i)).toBeVisible();
});