import { test, expect } from "@playwright/test";

test("health endpoint returns ok", async ({ request }) => {
  const response = await request.get("/health");
  expect(response.ok()).toBeTruthy();
  const body = await response.json();
  expect(body.status).toBe("ok");
  expect(body.database).toBe("ok");
});

test("metrics endpoint exposes counters", async ({ request }) => {
  await request.get("/health");
  await request.get("/login");
  const response = await request.get("/metrics");
  expect(response.ok()).toBeTruthy();
  const text = await response.text();
  expect(text).toContain("dtr_http_requests_total");
  expect(text).toContain("dtr_http_request_duration_seconds");
  const count = Number(
    text
      .split("\n")
      .find((line) => line.startsWith("dtr_http_requests_total "))
      ?.split(/\s+/)[1] ?? "0",
  );
  expect(count).toBeGreaterThanOrEqual(3);
});