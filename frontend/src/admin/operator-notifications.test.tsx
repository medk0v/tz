import { afterEach, describe, expect, it, vi } from "vitest";
import {
  OPERATOR_NOTIFICATION_SOUNDS,
  currentBrowserNotificationPermission,
  enableOperatorNotifications,
  isOperatorNotificationSound,
  showOperatorBrowserNotification,
} from "./operator-notifications";

function stubNotification(
  permission: NotificationPermission,
  requestedPermission: NotificationPermission = permission,
) {
  const instances: FakeNotification[] = [];
  const requestPermission = vi.fn().mockResolvedValue(requestedPermission);

  class FakeNotification {
    static permission = permission;
    static requestPermission = requestPermission;

    readonly close = vi.fn();
    onclick: ((event: Event) => void) | null = null;

    constructor(
      readonly title: string,
      readonly options?: NotificationOptions,
    ) {
      instances.push(this);
    }
  }

  vi.stubGlobal("Notification", FakeNotification);
  return { instances, requestPermission };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("operator notification sounds", () => {
  it("recognizes every available preset and rejects unknown values", () => {
    expect(OPERATOR_NOTIFICATION_SOUNDS).toEqual([
      "glass_chime",
      "pearl_chime",
      "warm_keys",
      "droplet_chime",
      "dawn_chime",
      "bell",
      "double_bell",
      "urgent_bell",
      "soft_chime",
      "crystal_ping",
      "rising_chime",
      "deep_bell",
    ]);
    for (const sound of OPERATOR_NOTIFICATION_SOUNDS) {
      expect(isOperatorNotificationSound(sound)).toBe(true);
    }
    expect(isOperatorNotificationSound("unknown"), "unknown presets must not reach playback").toBe(false);
    expect(isOperatorNotificationSound(null)).toBe(false);
  });
});

describe("operator browser notifications", () => {
  it("reports browsers without the Notification API as unsupported", async () => {
    vi.stubGlobal("Notification", undefined);

    expect(currentBrowserNotificationPermission()).toBe("unsupported");
    await expect(enableOperatorNotifications()).resolves.toBe("unsupported");
    expect(() => showOperatorBrowserNotification("Title", "Body")).not.toThrow();
  });

  it("requests permission synchronously from the user action", async () => {
    const { requestPermission } = stubNotification("default", "granted");

    const permission = enableOperatorNotifications();

    expect(requestPermission).toHaveBeenCalledOnce();
    await expect(permission).resolves.toBe("granted");
  });

  it("does not request permission again after the user has decided", async () => {
    const { requestPermission } = stubNotification("denied");

    expect(currentBrowserNotificationPermission()).toBe("denied");
    await expect(enableOperatorNotifications()).resolves.toBe("denied");
    expect(requestPermission).not.toHaveBeenCalled();
  });

  it("creates a system notification only after permission is granted", () => {
    const { instances } = stubNotification("granted");
    const focus = vi.spyOn(window, "focus").mockImplementation(() => undefined);
    vi.spyOn(Date, "now").mockReturnValue(1234);

    showOperatorBrowserNotification("New customer message", "A customer sent a new message.");

    expect(instances).toHaveLength(1);
    expect(instances[0]).toMatchObject({
      title: "New customer message",
      options: {
        body: "A customer sent a new message.",
        tag: "tz-1234",
      },
    });
    instances[0]?.onclick?.(new Event("click"));
    expect(focus).toHaveBeenCalledOnce();
    expect(instances[0]?.close).toHaveBeenCalledOnce();
  });

  it.each(["default", "denied"] as const)(
    "does not create a system notification while permission is %s",
    (permission) => {
      const { instances } = stubNotification(permission);

      showOperatorBrowserNotification("New customer message", "A customer sent a new message.");

      expect(instances).toHaveLength(0);
    },
  );
});
