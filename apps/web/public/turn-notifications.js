// Turn-notification service worker (#158). It only ever shows a generic "your turn"
// message: push payloads carry no rack, draft, credential, link or move.
const TARGET_CACHE = "scrabble-turn-target";
const TARGET_KEY = "turn-target";

const messages = {
  en: ["Scrabble", "It is your turn."],
  de: ["Scrabble", "Du bist am Zug."],
  es: ["Scrabble", "Es tu turno."],
};

async function target() {
  try {
    const cache = await caches.open(TARGET_CACHE);
    const response = await cache.match(new URL(TARGET_KEY, self.registration.scope));
    const stored = response === undefined ? undefined : new URL(await response.text());
    // Only ever navigate within this application's own origin and scope.
    if (stored && stored.href.startsWith(self.registration.scope)) {
      return stored.href;
    }
  } catch {
    // Fall back to the application root.
  }
  return self.registration.scope;
}

self.addEventListener("push", (event) => {
  const language = (self.navigator.language || "en").slice(0, 2);
  const [title, body] = messages[language] ?? messages.en;
  event.waitUntil(
    self.registration.showNotification(title, {
      body,
      tag: "scrabble-turn",
      renotify: true,
    }),
  );
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(
    (async () => {
      const url = await target();
      const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      const open = windows.find((client) => client.url === url) ?? windows[0];
      if (open !== undefined) {
        return open.focus();
      }
      return self.clients.openWindow(url);
    })(),
  );
});
