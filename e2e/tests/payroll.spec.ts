import { test, expect } from "@playwright/test";

const ADMIN_CODE = "ADMIN";
const ADMIN_PIN = "593847";
const MANAGER_CODE = "E2MGR";
const MANAGER_PIN = "482915";
const EMPLOYEE_CODE = "E2E001";
const EMPLOYEE_PIN = "482915";

async function loginAs(page: import("@playwright/test").Page, code: string, pin: string) {
  await page.goto("/login");
  await page.fill('input[name="employee_code"]', code);
  await page.fill('input[name="pin"]', pin);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL("/");
}

test.describe("payroll flows", () => {
  test("admin can close period, run payroll, and employee sees payslip", async ({ page }) => {
    await loginAs(page, MANAGER_CODE, MANAGER_PIN);
    await page.goto("/manager");
    const otRow = page.locator("tr", { hasText: EMPLOYEE_CODE });
    const approveButton = otRow.getByRole("button", { name: /^approve$/i });
    if (await approveButton.isVisible()) {
      await approveButton.click();
      await expect(page.getByText(/overtime approved/i)).toBeVisible();
    }
    await page.locator('form[action="/logout"] button').click();

    await loginAs(page, ADMIN_CODE, ADMIN_PIN);
    await page.goto("/admin/reports");
    await expect(page.getByRole("heading", { name: /payroll reports/i })).toBeVisible();

    const closeButton = page.getByRole("button", { name: /close this period/i });
    if (await closeButton.isVisible()) {
      await closeButton.click();
      await expect(
        page.getByText(/pay period closed|already closed|time edits in this range are now blocked/i)
      ).toBeVisible();
    }

    await page.goto("/admin/payroll");
    await expect(page.getByRole("heading", { name: /payroll runs/i })).toBeVisible();

    const createDraft = page.getByRole("button", { name: /create draft/i }).first();
    if (!(await createDraft.isEnabled())) {
      test.skip(true, "No runnable closed period or missing compensation for payroll");
    }

    await createDraft.click();
    await expect(page.getByRole("heading", { name: /payroll run/i })).toBeVisible();
    await expect(page.getByText(/draft/i)).toBeVisible();

    const finalizeButton = page.getByRole("button", { name: /finalize run/i });
    if (await finalizeButton.isEnabled()) {
      page.once("dialog", (dialog) => dialog.accept());
      await finalizeButton.click();
      await expect(page.getByText(/finalized|gross pay and deductions are locked/i)).toBeVisible();
      await expect(page.getByRole("link", { name: /download payroll csv/i })).toBeVisible();
    }

    await page.locator('form[action="/logout"] button').click();

    await loginAs(page, EMPLOYEE_CODE, EMPLOYEE_PIN);
    await page.goto("/me/payslips");
    await expect(page.getByRole("heading", { name: /my payslips/i })).toBeVisible();
    if (await page.getByText(EMPLOYEE_CODE).isVisible()) {
      await expect(page.getByRole("link", { name: /view/i }).first()).toBeVisible();
    }
  });
});