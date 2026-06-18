import { test, expect } from "@playwright/test";

const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";

test("admin can filter archived employees", async ({ page }) => {
  const newCode = `ARC${Date.now().toString().slice(-6)}`;

  await page.goto("/login");
  await page.fill('input[name="employee_code"]', ADMIN_CODE);
  await page.fill('input[name="pin"]', ADMIN_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");

  await page.goto("/admin/employees");
  await page.fill('input[name="employee_code"]', newCode);
  await page.fill('input[name="full_name"]', "E2E Archive Test");
  await page.fill('input[name="pin"]', "482915");
  await page.selectOption('select[name="role"]', "employee");
  await page.getByRole("button", { name: /create employee/i }).click();
  await expect(page.getByText(newCode.toUpperCase())).toBeVisible();

  const row = page.locator("tr", { hasText: newCode.toUpperCase() });
  await row.getByRole("link", { name: /edit/i }).click();
  await page.getByRole("button", { name: /deactivate employee/i }).click();

  await page.goto("/admin/employees?status=archived");
  await expect(page.getByRole("link", { name: /^archived$/i })).toHaveClass(/active/);
  await expect(page.getByText(newCode.toUpperCase())).toBeVisible();
  await expect(page.getByText(/inactive/i)).toBeVisible();
});