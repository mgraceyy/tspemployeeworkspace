import { test, expect } from "@playwright/test";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";

const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";
const EMPLOYEE_CODE = "E2E001";
const IMPORT_SALARY = "27123.45";
const EFFECTIVE_FROM = "2026-01-01";

async function loginAsAdmin(page: import("@playwright/test").Page) {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', ADMIN_CODE);
  await page.fill('input[name="pin"]', ADMIN_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");
}

test("admin can preview and apply compensation CSV import", async ({ page }) => {
  const csv = [
    "employee_code,monthly_salary,ot_rate_percent,transport_allowance,meal_allowance,effective_from",
    `${EMPLOYEE_CODE},${IMPORT_SALARY},132,1000,500,${EFFECTIVE_FROM}`,
  ].join("\n");

  const tmpFile = path.join(os.tmpdir(), `dtr-e2e-comp-${Date.now()}.csv`);
  fs.writeFileSync(tmpFile, csv, "utf8");

  await loginAsAdmin(page);
  await page.goto("/admin/compensation/import");
  await expect(page.getByRole("heading", { name: /import compensation/i })).toBeVisible();

  await page.locator('input[name="csv_file"]').setInputFiles(tmpFile);
  await page.getByRole("button", { name: /preview import/i }).click();

  await expect(page.getByText(/1 valid row/i)).toBeVisible();
  await expect(page.getByText(IMPORT_SALARY)).toBeVisible();
  await expect(page.getByText(EMPLOYEE_CODE)).toBeVisible();

  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: /apply import/i }).click();
  await expect(page.getByText(/applied compensation for 1 employee/i)).toBeVisible();

  await page.goto("/admin/employees");
  const row = page.locator("tr", { hasText: EMPLOYEE_CODE });
  await row.getByRole("link", { name: /edit/i }).click();
  await page.getByRole("link", { name: /compensation/i }).click();

  await expect(page.getByRole("heading", { name: /employee compensation/i })).toBeVisible();
  await expect(page.locator('input[name="monthly_salary"]')).toHaveValue(IMPORT_SALARY);
  await expect(page.locator('input[name="transport_allowance"]')).toHaveValue("1000.00");
  await expect(page.locator('input[name="meal_allowance"]')).toHaveValue("500.00");

  fs.unlinkSync(tmpFile);
});