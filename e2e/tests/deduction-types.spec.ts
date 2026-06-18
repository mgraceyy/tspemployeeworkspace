import { test, expect } from "@playwright/test";

const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";

test("admin can create a deduction type", async ({ page }) => {
  const typeCode = `E2E${Date.now().toString().slice(-5)}`;

  await page.goto("/login");
  await page.fill('input[name="employee_code"]', ADMIN_CODE);
  await page.fill('input[name="pin"]', ADMIN_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");

  await page.goto("/admin/deduction-types");
  await expect(page.getByRole("heading", { name: /deduction types/i })).toBeVisible();

  await page.fill('input[name="code"]', typeCode);
  await page.fill('input[name="name"]', "E2E Voluntary Deduction");
  await page.getByRole("button", { name: /^create$/i }).click();

  await expect(page.getByText(/deduction type created/i)).toBeVisible();
  await expect(page.getByText(typeCode)).toBeVisible();
  await expect(page.getByText("E2E Voluntary Deduction")).toBeVisible();
});