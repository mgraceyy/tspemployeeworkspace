import { test, expect } from "@playwright/test";

const ADMIN_CODE = "ADMIN";
const ADMIN_INITIAL_PIN = "1234";
const ADMIN_PIN = "593847";

test("admin can change PIN and create a new employee", async ({ page }) => {
  const newCode = `E2E${Date.now().toString().slice(-6)}`;

  await page.goto("/login");
  await page.fill('input[name="employee_code"]', ADMIN_CODE);
  await page.fill('input[name="pin"]', ADMIN_INITIAL_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();

  await expect(page).toHaveURL("/change-pin");
  await page.fill('input[name="new_pin"]', ADMIN_PIN);
  await page.fill('input[name="confirm_pin"]', ADMIN_PIN);
  await page.getByRole("button", { name: /save pin/i }).click();
  await expect(page).toHaveURL("/");

  await page.goto("/admin/employees");
  await expect(page.getByRole("heading", { name: /^employees$/i })).toBeVisible();

  await page.fill('input[name="employee_code"]', newCode);
  await page.fill('input[name="full_name"]', "E2E Created Employee");
  await page.fill('input[name="pin"]', "482915");
  await page.selectOption('select[name="role"]', "employee");
  await page.getByRole("button", { name: /create employee/i }).click();

  await expect(page.getByText(/employee created/i)).toBeVisible();
  await expect(page.getByText(newCode.toUpperCase())).toBeVisible();
});