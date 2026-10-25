import { test, expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { startManager } from "../../scripts/manager-process.mjs";

const password = "🔬".repeat(128);

async function signIn(page: Page, origin: string) {
  await page.goto(origin);
  await page.getByLabel("Username", { exact: true }).fill("owner");
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Your bots." })).toBeVisible();
}

test("setup, durable bots, independent sessions, disconnection, and sign-out", async ({
  browser,
  page,
}, testInfo) => {
  const root = await mkdtemp(join(tmpdir(), "botstral-browser-"));
  const binary = resolve("../bin/botstral");
  let manager = await startManager({ binary, dataDir: root, setup: true });
  const second = await browser.newContext();
  const anonymous = await browser.newContext();
  try {
    if (!manager.setupURL) throw new Error("Setup listener was not opened.");
    const setupOrigin = new URL(manager.setupURL).origin;
    await page.goto(manager.setupURL);
    await expect(page).toHaveURL(`${setupOrigin}/`);
    const username = page.getByLabel("Username", { exact: true });
    await username.fill("invalid!");
    expect(
      await username.evaluate(
        (input) => input instanceof HTMLInputElement && input.checkValidity(),
      ),
    ).toBe(false);
    await username.fill("valid-owner");
    expect(
      await username.evaluate(
        (input) => input instanceof HTMLInputElement && input.checkValidity(),
      ),
    ).toBe(true);
    await page.getByLabel("Username", { exact: true }).fill("Owner");
    await page.getByLabel("Password", { exact: true }).fill(password);
    await page.getByLabel("Confirm password").fill(password);
    await page.getByRole("button", { name: "Create owner" }).click();
    await expect(
      page.getByRole("link", { name: "Continue to sign in" }),
    ).toBeVisible();
    expect((await page.context().cookies()).length).toBe(0);
    await signIn(page, manager.mainURL);
    await expect(page.getByText("Start with a name.")).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath("bots-empty.png"),
      fullPage: true,
    });
    await page.getByLabel("Bot name").fill("Research");
    await page.getByLabel("Bot name").press("Enter");
    const card = page.getByTestId("bot-card");
    await expect(card).toHaveCount(1);
    await expect(card.getByRole("heading", { name: "Research" })).toBeVisible();
    const botID = await card.getAttribute("data-bot-id");
    expect(botID).toMatch(/^[a-f0-9]{32}$/);
    await page.screenshot({
      path: testInfo.outputPath("bots-desktop.png"),
      fullPage: true,
    });

    const otherPage = await second.newPage();
    await signIn(otherPage, manager.mainURL);
    await expect(otherPage.getByTestId("bot-card")).toHaveCount(1);
    const originalCookie = (await page.context().cookies()).find(
      (cookie) => cookie.name === "botstral_session",
    );
    expect(originalCookie?.httpOnly).toBe(true);
    expect(originalCookie?.sameSite).toBe("Strict");
    expect(originalCookie?.path).toBe("/api/v1");
    expect(await page.evaluate(() => localStorage.length)).toBe(0);
    const mainURL = manager.mainURL;
    const port = Number(new URL(mainURL).port);
    await page.getByLabel("Bot name").fill("A preserved draft");
    await manager.stop();
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(
      page.getByRole("status").filter({ hasText: "Manager disconnected" }),
    ).toBeVisible();
    await expect(page.getByLabel("Bot name")).toHaveValue("A preserved draft");
    await expect(
      page.getByRole("button", { name: "Create bot", exact: true }),
    ).toBeDisabled();
    await expect(card).toHaveCount(1);
    manager = await startManager({ binary, dataDir: root, port });
    await page.evaluate(() => window.dispatchEvent(new Event("online")));
    await expect(
      page.getByRole("status").filter({ hasText: "Manager connected" }),
    ).toBeVisible();
    await expect(card).toHaveAttribute("data-bot-id", botID ?? "missing");
    await expect(page.getByLabel("Bot name")).toHaveValue("A preserved draft");
    expect(
      (await page.context().cookies()).find(
        (cookie) => cookie.name === "botstral_session",
      )?.value === originalCookie?.value,
    ).toBe(true);

    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
    await page.screenshot({
      path: testInfo.outputPath("sign-in.png"),
      fullPage: true,
    });
    await signIn(page, mainURL);
    await expect(card).toHaveAttribute("data-bot-id", botID ?? "missing");
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.getByLabel("Bot name")).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath("bots-mobile.png"),
      fullPage: true,
    });
    await page.getByRole("button", { name: "Sessions", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Active sessions", exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("listitem")).toHaveCount(2);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath("sessions-mobile.png"),
      fullPage: true,
    });
    const otherSession = page
      .getByRole("listitem")
      .filter({ hasNot: page.getByText("This session", { exact: true }) });
    await expect(otherSession).toHaveCount(1);
    await otherSession.getByRole("button", { name: /Revoke/ }).click();
    await page.getByRole("button", { name: "Confirm revocation" }).click();
    await expect(page.getByRole("listitem")).toHaveCount(1);
    await otherPage.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(
      otherPage.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
    await expect(otherPage.getByTestId("bot-card")).toHaveCount(0);
    await page.getByRole("button", { name: "Revoke this session" }).click();
    await page.getByRole("button", { name: "Confirm revocation" }).click();
    await expect(
      page.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
    await expect(page.getByTestId("bot-card")).toHaveCount(0);
    const anonymousPage = await anonymous.newPage();
    await anonymousPage.goto(mainURL);
    await expect(
      anonymousPage.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
    expect(
      (await anonymous.request.get(`${mainURL}/api/v1/bots`)).status(),
    ).toBe(401);
  } finally {
    await second.close();
    await anonymous.close();
    try {
      await manager.stop();
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
});

test("late private response cannot restore data after sign-out", async ({
  page,
}) => {
  const root = await mkdtemp(join(tmpdir(), "botstral-late-"));
  const manager = await startManager({
    binary: resolve("../bin/botstral"),
    dataDir: root,
    setup: true,
  });
  try {
    if (!manager.setupURL) throw new Error("Setup listener was not opened.");
    const setup = new URL(manager.setupURL);
    const response = await page.request.post(`${setup.origin}/owner`, {
      headers: { Origin: setup.origin, "X-Setup-Token": setup.hash.slice(1) },
      data: {
        username: "owner",
        password,
        confirmPassword: password,
      },
    });
    expect(response.status()).toBe(201);
    await signIn(page, manager.mainURL);
    const privateName = "🔬".repeat(80);
    await page.getByLabel("Bot name").fill(privateName);
    await page.getByRole("button", { name: "Create bot", exact: true }).click();
    await expect(page.getByTestId("bot-card")).toHaveCount(1);
    await expect(
      page.getByRole("heading", { name: privateName }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Sessions", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Active sessions", exact: true }),
    ).toBeVisible();
    let release: (() => void) | undefined;
    const blocked = new Promise<void>((resolve) => {
      release = resolve;
    });
    let admitted: (() => void) | undefined;
    const started = new Promise<void>((resolve) => {
      admitted = resolve;
    });
    await page.route("**/api/v1/bots", async (route) => {
      const result = await route.fetch();
      admitted?.();
      await blocked;
      await route.fulfill({ response: result });
    });
    await page.getByRole("button", { name: "Your bots", exact: true }).click();
    await started;
    await page.getByRole("button", { name: "Sign out", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
    release?.();
    await page.unrouteAll({ behavior: "wait" });
    await expect(page.getByTestId("bot-card")).toHaveCount(0);
    await expect(
      page.getByRole("heading", { name: "Sign in to Botstral" }),
    ).toBeVisible();
  } finally {
    try {
      await manager.stop();
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  }
});
