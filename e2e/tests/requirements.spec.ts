import { test, expect } from "@playwright/test";
import path from "node:path";
import os from "node:os";
import fs from "node:fs";

const EMPLOYEE_CODE = "E2E001";
const EMPLOYEE_PIN = "482915";

test("employee can upload a requirement document", async ({ page }) => {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', EMPLOYEE_CODE);
  await page.fill('input[name="pin"]', EMPLOYEE_PIN);
  await page.getByRole("button", { name: /sign in/i }).click();

  await page.goto("/me/requirements");
  await expect(page.getByRole("heading", { name: /my requirements/i })).toBeVisible();
  await expect(page.getByText("E2E Test Document")).toBeVisible();

  const row = page.locator("tr", { hasText: "E2E Test Document" });
  const fileInput = row.locator('input[type="file"]');
  const tmpFile = path.join(os.tmpdir(), `dtr-e2e-${Date.now()}.pdf`);
  fs.writeFileSync(tmpFile, "%PDF-1.4 e2e requirement upload");

  await fileInput.setInputFiles(tmpFile);
  await row.getByRole("button", { name: /upload.*submit/i }).click();

  await expect(page.getByText(/requirement submitted|uploaded/i)).toBeVisible();
  await expect(row.locator('a[href*="/me/requirements/"]')).toBeVisible();

  fs.unlinkSync(tmpFile);
});