/**
 * Opt-in background turn notifications (#158; Decisions 2.11 B, 2.14 A). The subscription is
 * registered over the transport-authenticated control stream, so the server binds it to the
 * native player identity. Each device opts in and out independently.
 */
export type ControlExchange = (
  payload: Uint8Array,
) => Promise<{ accepted: boolean; payload: Uint8Array }>;
export type TurnNotificationResult = "enabled" | "disabled" | "unsupported" | "denied" | "failed";

const TARGET_CACHE = "scrabble-turn-target";
const encoder = new TextEncoder();
const decoder = new TextDecoder();

export function turnNotificationsSupported(): boolean {
  return (
    typeof window !== "undefined" &&
    window.isSecureContext &&
    "serviceWorker" in navigator &&
    "PushManager" in window &&
    "Notification" in window
  );
}

function storageKey(gameId: string, playerId: string) {
  return `scrabble:turn-notifications:${gameId}:${playerId}`;
}
export function turnNotificationsEnabled(gameId: string, playerId: string): boolean {
  try {
    return localStorage.getItem(storageKey(gameId, playerId)) !== null;
  } catch {
    return false;
  }
}

async function exchange(control: ControlExchange, request: unknown): Promise<unknown> {
  const response = await control(encoder.encode(JSON.stringify(request)));
  if (!response.accepted) {
    throw new Error("Turn notification request rejected");
  }
  return JSON.parse(decoder.decode(response.payload)) as unknown;
}

function base64Url(bytes: ArrayBuffer | null): string {
  if (bytes === null) {
    throw new Error("Push subscription lacks keys");
  }
  let binary = "";
  for (const byte of new Uint8Array(bytes)) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}
function fromBase64Url(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text.replaceAll("-", "+").replaceAll("_", "/"));
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
function sameKey(subscription: PushSubscription, key: Uint8Array): boolean {
  const current = subscription.options.applicationServerKey;
  return current !== null && base64Url(current) === base64Url(key.buffer as ArrayBuffer);
}

/** Must run from a deliberate user action: it may show the browser permission prompt. */
export async function enableTurnNotifications(
  control: ControlExchange,
  gameId: string,
  playerId: string,
): Promise<TurnNotificationResult> {
  if (!turnNotificationsSupported()) {
    return "unsupported";
  }
  try {
    const permission = await Notification.requestPermission();
    if (permission !== "granted") {
      return "denied";
    }
    const base = new URL(import.meta.env.BASE_URL, location.href);
    const registration = await navigator.serviceWorker.register(
      new URL("turn-notifications.js", base),
      { scope: base.pathname },
    );
    const answer = (await exchange(control, { kind: "push_key" })) as { public_key?: unknown };
    if (typeof answer.public_key !== "string") {
      return "failed";
    }
    const key = fromBase64Url(answer.public_key);
    let subscription = await registration.pushManager.getSubscription();
    if (subscription !== null && !sameKey(subscription, key)) {
      await subscription.unsubscribe();
      subscription = null;
    }
    subscription ??= await registration.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: key,
    });
    await exchange(control, {
      kind: "push_subscribe",
      subscription: {
        endpoint: subscription.endpoint,
        p256dh: base64Url(subscription.getKey("p256dh")),
        auth: base64Url(subscription.getKey("auth")),
      },
    });
    // The click-through target stays on this device; it is never part of a push payload.
    const cache = await caches.open(TARGET_CACHE);
    await cache.put(new URL("turn-target", base), new Response(location.href));
    localStorage.setItem(storageKey(gameId, playerId), subscription.endpoint);
    return "enabled";
  } catch {
    return "failed";
  }
}

/** Removes only this device for this match; other devices keep their notifications. */
export async function disableTurnNotifications(
  control: ControlExchange,
  gameId: string,
  playerId: string,
): Promise<TurnNotificationResult> {
  try {
    const endpoint = localStorage.getItem(storageKey(gameId, playerId));
    if (endpoint !== null) {
      await exchange(control, { kind: "push_unsubscribe", endpoint });
    }
    localStorage.removeItem(storageKey(gameId, playerId));
    return "disabled";
  } catch {
    return "failed";
  }
}
