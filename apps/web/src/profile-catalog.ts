import { gameApiUrl, readResponse } from "./create-game";
import { profileCatalog, type ProfileCatalog } from "./profile-model";

export async function fetchProfiles(api: string, signal: AbortSignal): Promise<ProfileCatalog> {
  const url = gameApiUrl(api);
  url.pathname = url.pathname.replace(/\/games$/, "/profiles");
  const response = await fetch(url, {
    credentials: "omit",
    signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
  });
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error("Profile catalog unavailable");
  }
  return profileCatalog.parse(await readResponse(response, 128 * 1024));
}
