import { test, expect } from "@playwright/test";

const MANAGER_CODE = "E2MGR";
const MANAGER_PIN = "482915";
const EMPLOYEE_CODE = "E2E001";

test.describe("manager flows", () => {
  test("manager dashboard loads and can mark team absence", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', MANAGER_CODE);
    await page.fill('input[name="pin"]', MANAGER_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await expect(page).toHaveURL("/");
    await page.goto("/manager");
    await expect(page.getByRole("heading", { name: /manager dashboard/i })).toBeVisible();
    await expect(page.getByText(EMPLOYEE_CODE)).toBeVisible();

    const absenceRow = page.locator("tr", { hasText: EMPLOYEE_CODE });
    const markButton = absenceRow.getByRole("button", { name: /^mark$/i });
    if (await markButton.isVisible()) {
      await absenceRow.locator('select[name="absence_type"]').selectOption("vacation");
      await markButton.click();
      await expect(page.getByText(/marked as vacation/i)).toBeVisible();
    }
  });

  test("manager can approve pending overtime", async ({ page }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', MANAGER_CODE);
    await page.fill('input[name="pin"]', MANAGER_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await page.goto("/manager");
    await expect(page.getByRole("heading", { name: /pending ot approvals/i })).toBeVisible();

    const otRow = page.locator("tr", { hasText: EMPLOYEE_CODE });
    const approveButton = otRow.getByRole("button", { name: /^approve$/i });
    if (await approveButton.isVisible()) {
      await approveButton.click();
      await expect(page.getByText(/overtime approved/i)).toBeVisible();
    }
  });

  test("manager can open team member timesheet export", async ({ page, request }) => {
    await page.goto("/login");
    await page.fill('input[name="employee_code"]', MANAGER_CODE);
    await page.fill('input[name="pin"]', MANAGER_PIN);
    await page.getByRole("button", { name: /sign in/i }).click();

    await page.goto("/manager/team");
    await expect(page.getByText(EMPLOYEE_CODE)).toBeVisible();

    const exportLink = page.locator(`a[href*="/manager/team/"][href$="/export.csv"]`).first();
    const href = await exportLink.getAttribute("href");
    expect(href).toBeTruthy();

    const response = await request.get(href!);
    expect(response.ok()).toBeTruthy();
    const body = await response.text();
    expect(body).toContain(EMPLOYEE_CODE);
  });
});